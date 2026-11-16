//! Small IPv4 network stack: DHCP client, UDP echo and basic TCP handshake.

use crate::diagnostics;
use crate::e1000;

const ETH_HEADER: usize = 14;
const IP_HEADER: usize = 20;
const UDP_HEADER: usize = 8;
const TCP_MIN_HEADER: usize = 20;

const PROTO_UDP: u8 = 17;
const PROTO_TCP: u8 = 6;

const DHCP_CLIENT_PORT: u16 = 68;
const DHCP_SERVER_PORT: u16 = 67;
const DHCP_MAGIC: [u8; 4] = [0x63, 0x82, 0x53, 0x63];

static mut OUR_IP: [u8; 4] = [0, 0, 0, 0];
static mut GATEWAY: [u8; 4] = [0, 0, 0, 0];

/// Start DHCP negotiation.
pub fn init(mac: [u8; 6]) {
    if mac == [0; 6] {
        return;
    }
    match send_dhcp(mac, 1, [0, 0, 0, 0], 0) {
        Ok(_) => diagnostics::debug("[net] DHCP DISCOVER sent"),
        Err(_) => diagnostics::debug("[net] DHCP DISCOVER failed"),
    }
}

/// Build and transmit a DHCP message (1 = DISCOVER, 3 = REQUEST).
fn send_dhcp(mac: [u8; 6], message_type: u8, requested_ip: [u8; 4], server_id: u32) -> Result<(), &'static str> {
    let xid = 0x1234_5678u32;
    let mut dhcp = [0u8; 300];
    dhcp[0] = 1; // BOOTREQUEST
    dhcp[1] = 1; // Ethernet
    dhcp[2] = 6; // hardware address length
    dhcp[4..8].copy_from_slice(&xid.to_be_bytes());
    dhcp[8..10].copy_from_slice(&0x8000u16.to_be_bytes()); // broadcast flag
    dhcp[28..34].copy_from_slice(&mac);
    dhcp[236..240].copy_from_slice(&DHCP_MAGIC);

    let mut options = 240usize;
    dhcp[options] = 53;
    dhcp[options + 1] = 1;
    dhcp[options + 2] = message_type;
    options += 3;

    dhcp[options] = 55;
    dhcp[options + 1] = 3;
    dhcp[options + 2] = 1;
    dhcp[options + 3] = 3;
    dhcp[options + 4] = 6;
    options += 5;

    if message_type == 3 {
        dhcp[options] = 50;
        dhcp[options + 1] = 4;
        dhcp[options + 2..options + 6].copy_from_slice(&requested_ip);
        options += 6;
        dhcp[options] = 54;
        dhcp[options + 1] = 4;
        dhcp[options + 2..options + 6].copy_from_slice(&server_id.to_be_bytes());
        options += 6;
    }
    dhcp[options] = 255;
    let dhcp_length = options + 1;

    send_udp(mac, [0, 0, 0, 0], [255, 255, 255, 255],
             DHCP_CLIENT_PORT, DHCP_SERVER_PORT, &dhcp[..dhcp_length]).map(|_| ())
}

/// Build an IPv4/UDP datagram and hand it to the Ethernet driver.
fn send_udp(mac: [u8; 6], source: [u8; 4], destination: [u8; 4],
            source_port: u16, destination_port: u16, payload: &[u8]) -> Result<usize, &'static str> {
    let total_length = IP_HEADER + UDP_HEADER + payload.len();
    let mut packet = [0u8; 2048];

    packet[0..6].copy_from_slice(&[0xFF; 6]);
    packet[6..12].copy_from_slice(&mac);
    packet[12..14].copy_from_slice(&[0x08, 0x00]);

    packet[14] = 0x45;
    packet[16..18].copy_from_slice(&(total_length as u16).to_be_bytes());
    packet[18..20].copy_from_slice(&0x1234u16.to_be_bytes());
    packet[20] = 64;
    packet[21] = PROTO_UDP;
    packet[26..30].copy_from_slice(&source);
    packet[30..34].copy_from_slice(&destination);
    let ip_checksum = e1000::checksum(&packet[14..34]);
    packet[24..26].copy_from_slice(&ip_checksum.to_be_bytes());

    let udp_offset = ETH_HEADER + IP_HEADER;
    packet[udp_offset..udp_offset + 2].copy_from_slice(&source_port.to_be_bytes());
    packet[udp_offset + 2..udp_offset + 4].copy_from_slice(&destination_port.to_be_bytes());
    packet[udp_offset + 4..udp_offset + 6].copy_from_slice(&((UDP_HEADER + payload.len()) as u16).to_be_bytes());
    packet[udp_offset + 8..udp_offset + 8 + payload.len()].copy_from_slice(payload);

    e1000::send_frame(&packet[..ETH_HEADER + total_length])
}

/// Parse DHCP options and act on OFFER/ACK packets.
fn handle_dhcp(mac: [u8; 6], payload: &[u8]) {
    if payload.len() < 240 || payload[236..240] != DHCP_MAGIC {
        return;
    }
    let mut message_type = 0u8;
    let mut server_id = 0u32;
    let mut index = 240usize;
    while index + 2 <= payload.len() {
        let code = payload[index];
        if code == 255 {
            break;
        }
        let length = payload[index + 1] as usize;
        if index + 2 + length > payload.len() {
            break;
        }
        let value = &payload[index + 2..index + 2 + length];
        match code {
            53 if length >= 1 => message_type = value[0],
            54 if length >= 4 => {
                server_id = u32::from_be_bytes([value[0], value[1], value[2], value[3]]);
            }
            _ => {}
        }
        index += 2 + length;
    }

    match message_type {
        2 => {
            let offered = [payload[16], payload[17], payload[18], payload[19]];
            diagnostics::puts("[net] DHCP OFFER: ");
            for byte in offered {
                diagnostics::put_hex(byte as u64);
                diagnostics::putc(b' ');
            }
            diagnostics::puts("\n");
            let _ = send_dhcp(mac, 3, offered, server_id);
        }
        5 => {
            let address = [payload[16], payload[17], payload[18], payload[19]];
            unsafe {
                OUR_IP = address;
                GATEWAY = [address[0], address[1], address[2], 1];
            }
            diagnostics::puts("[net] DHCP ACK, IP assigned\n");
        }
        _ => {}
    }
}

/// Reply to UDP echo requests (port 7).
fn handle_udp(mac: [u8; 6], frame: &[u8]) {
    let udp = ETH_HEADER + IP_HEADER;
    if frame.len() < udp + UDP_HEADER {
        return;
    }
    let destination_port = u16::from_be_bytes([frame[udp + 2], frame[udp + 3]]);
    let source_port = u16::from_be_bytes([frame[udp], frame[udp + 1]]);
    if destination_port == DHCP_CLIENT_PORT {
        handle_dhcp(mac, &frame[udp + UDP_HEADER..]);
        return;
    }
    if destination_port != 7 && destination_port != 9999 {
        return;
    }

    let udp_length = u16::from_be_bytes([frame[udp + 4], frame[udp + 5]]) as usize;
    if udp_length < UDP_HEADER || udp + udp_length > frame.len() {
        return;
    }
    let payload = &frame[udp + UDP_HEADER..udp + udp_length];

    let mut response = [0u8; 2048];
    let total_length = IP_HEADER + UDP_HEADER + payload.len();
    response[0..6].copy_from_slice(&frame[6..12]);
    response[6..12].copy_from_slice(&mac);
    response[12..14].copy_from_slice(&[0x08, 0x00]);
    response[14] = 0x45;
    response[16..18].copy_from_slice(&(total_length as u16).to_be_bytes());
    response[20] = 64;
    response[21] = PROTO_UDP;
    response[26..30].copy_from_slice(&frame[30..34]);
    response[30..34].copy_from_slice(&frame[26..30]);
    let ip_checksum = e1000::checksum(&response[14..34]);
    response[24..26].copy_from_slice(&ip_checksum.to_be_bytes());

    let out = ETH_HEADER + IP_HEADER;
    response[out..out + 2].copy_from_slice(&destination_port.to_be_bytes());
    response[out + 2..out + 4].copy_from_slice(&source_port.to_be_bytes());
    response[out + 4..out + 6].copy_from_slice(&((UDP_HEADER + payload.len()) as u16).to_be_bytes());
    response[out + 8..out + 8 + payload.len()].copy_from_slice(payload);

    let _ = e1000::send_frame(&response[..ETH_HEADER + total_length]);
}

/// Minimal TCP handling: complete a three-way handshake for port 7 and echo.
fn handle_tcp(mac: [u8; 6], frame: &[u8]) {
    let tcp = ETH_HEADER + IP_HEADER;
    if frame.len() < tcp + TCP_MIN_HEADER {
        return;
    }
    let destination_port = u16::from_be_bytes([frame[tcp + 2], frame[tcp + 3]]);
    if destination_port != 7 && destination_port != 9999 {
        return;
    }
    let source_port = u16::from_be_bytes([frame[tcp], frame[tcp + 1]]);
    let sequence = u32::from_be_bytes([frame[tcp + 4], frame[tcp + 5], frame[tcp + 6], frame[tcp + 7]]);
    let acknowledgement = u32::from_be_bytes([frame[tcp + 8], frame[tcp + 9], frame[tcp + 10], frame[tcp + 11]]);
    let data_offset = ((frame[tcp + 12] >> 4) as usize) * 4;
    let flags = frame[tcp + 13];
    const SYN: u8 = 1 << 1;
    const ACK: u8 = 1 << 4;
    const PSH: u8 = 1 << 3;

    let payload_start = tcp + data_offset;
    let payload = if payload_start < frame.len() { &frame[payload_start..] } else { &[] };
    let is_syn = flags & SYN != 0;
    let _is_ack = flags & ACK != 0;

    let mut response = [0u8; 2048];
    let total_length = IP_HEADER + TCP_MIN_HEADER + payload.len();
    response[0..6].copy_from_slice(&frame[6..12]);
    response[6..12].copy_from_slice(&mac);
    response[12..14].copy_from_slice(&[0x08, 0x00]);
    response[14] = 0x45;
    response[16..18].copy_from_slice(&(total_length as u16).to_be_bytes());
    response[20] = 64;
    response[21] = PROTO_TCP;
    response[26..30].copy_from_slice(&frame[30..34]);
    response[30..34].copy_from_slice(&frame[26..30]);
    let ip_checksum = e1000::checksum(&response[14..34]);
    response[24..26].copy_from_slice(&ip_checksum.to_be_bytes());

    let out = ETH_HEADER + IP_HEADER;
    response[out..out + 2].copy_from_slice(&source_port.to_be_bytes());
    response[out + 2..out + 4].copy_from_slice(&destination_port.to_be_bytes());
    let reply_sequence = if is_syn { 1 } else { acknowledgement };
    let reply_ack = if is_syn { sequence.wrapping_add(1) } else { sequence.wrapping_add(payload.len() as u32) };
    response[out + 4..out + 8].copy_from_slice(&reply_sequence.to_be_bytes());
    response[out + 8..out + 12].copy_from_slice(&reply_ack.to_be_bytes());
    response[out + 12] = (TCP_MIN_HEADER / 4) as u8;
    response[out + 13] = if is_syn { SYN | ACK } else { ACK | PSH };
    response[out + 14..out + 16].copy_from_slice(&8192u16.to_be_bytes());
    response[out + 16..out + 18].copy_from_slice(&[0, 0]);
    response[out + 18..out + 20].copy_from_slice(&[0, 0]);
    response[out + TCP_MIN_HEADER..out + TCP_MIN_HEADER + payload.len()].copy_from_slice(payload);

    let _ = e1000::send_frame(&response[..ETH_HEADER + total_length]);
}

/// Process an incoming Ethernet/IPv4 frame.
pub fn process_frame(frame: &[u8]) {
    if frame.len() < ETH_HEADER + IP_HEADER {
        return;
    }
    let mac = e1000::mac();
    match frame[ETH_HEADER + 9] {
        PROTO_UDP => handle_udp(mac, frame),
        PROTO_TCP => handle_tcp(mac, frame),
        _ => {}
    }
}

/// Assigned IP address (0.0.0.0 until DHCP completes).
pub fn our_ip() -> [u8; 4] {
    unsafe { OUR_IP }
}

/// Default gateway.
pub fn gateway() -> [u8; 4] {
    unsafe { GATEWAY }
}
