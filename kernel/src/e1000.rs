//! Intel PRO/1000 (e1000) driver: descriptor rings, EEPROM MAC and frame TX/RX.

use crate::diagnostics;
use crate::pci;

const CTRL: usize = 0x0000;
const STATUS: usize = 0x0008;
const EERD: usize = 0x0014;
const RXCTRL: usize = 0x0100;
const TXCTRL: usize = 0x0400;
const RDBAL: usize = 0x2800;
const RDBAH: usize = 0x2804;
const RDLEN: usize = 0x2808;
const RDH: usize = 0x2810;
const RDT: usize = 0x2818;
const TDBAL: usize = 0x3800;
const TDBAH: usize = 0x3804;
const TDLEN: usize = 0x3808;
const TDH: usize = 0x3810;
const TDT: usize = 0x3818;

const RX_COUNT: usize = 32;
const TX_COUNT: usize = 8;
const FRAME_SIZE: usize = 2048;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct RxDescriptor {
    address: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct TxDescriptor {
    address: u64,
    length: u16,
    cso: u8,
    command: u8,
    status: u8,
    css: u8,
}

#[repr(C, align(16))]
struct RxRing([RxDescriptor; RX_COUNT]);
#[repr(C, align(16))]
struct TxRing([TxDescriptor; TX_COUNT]);
#[repr(C, align(2048))]
struct RxBuffers([[u8; FRAME_SIZE]; RX_COUNT]);
#[repr(C, align(2048))]
struct TxBuffers([[u8; FRAME_SIZE]; TX_COUNT]);

static mut RX_RING: RxRing = RxRing(
    [RxDescriptor { address: 0, length: 0, checksum: 0, status: 0, errors: 0 }; RX_COUNT]);
static mut TX_RING: TxRing = TxRing(
    [TxDescriptor { address: 0, length: 0, cso: 0, command: 0, status: 0, css: 0 }; TX_COUNT]);
static mut RX_DATA: RxBuffers = RxBuffers([[0; FRAME_SIZE]; RX_COUNT]);
static mut TX_DATA: TxBuffers = TxBuffers([[0; FRAME_SIZE]; TX_COUNT]);

struct Controller {
    mmio: usize,
    #[allow(dead_code)]
    mac: [u8; 6],
}

impl Controller {
    unsafe fn read(&self, offset: usize) -> u32 {
        core::ptr::read_volatile((self.mmio + offset) as *const u32)
    }

    unsafe fn write(&self, offset: usize, value: u32) {
        core::ptr::write_volatile((self.mmio + offset) as *mut u32, value);
    }

    unsafe fn eeprom_read(&self, address: u8) -> u16 {
        self.write(EERD, ((address as u32) << 8) | 1);
        for _ in 0..100_000 {
            if self.read(EERD) & 1 == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        (self.read(EERD) >> 16) as u16
    }

    fn physical(address: usize) -> u64 {
        crate::limine::physical(address)
    }

    /// Reset the controller and program both descriptor rings.
    #[allow(static_mut_refs)]
    unsafe fn setup(&self) {
        // Software reset (CTRL.RST) then wait for self-clear.
        self.write(CTRL, self.read(CTRL) | (1 << 26));
        for _ in 0..100_000 {
            if self.read(CTRL) & (1 << 26) == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        // Receive ring.
        for index in 0..RX_COUNT {
            let descriptor = core::ptr::addr_of_mut!(RX_RING.0[index]);
            let data = core::ptr::addr_of_mut!(RX_DATA.0[index]);
            (*descriptor).address = Self::physical(data as usize);
            (*descriptor).length = FRAME_SIZE as u16;
            (*descriptor).status = 0;
        }
        let rx_phys = Self::physical(RX_RING.0.as_ptr() as usize);
        self.write(RDBAL, rx_phys as u32);
        self.write(RDBAH, (rx_phys >> 32) as u32);
        self.write(RDLEN, (RX_COUNT * 16) as u32);
        self.write(RDH, 0);
        self.write(RDT, (RX_COUNT - 1) as u32);
        self.write(RXCTRL, 1);

        // Transmit ring.
        for index in 0..TX_COUNT {
            let descriptor = core::ptr::addr_of_mut!(TX_RING.0[index]);
            (*descriptor).status = 0;
            (*descriptor).command = 0;
        }
        let tx_phys = Self::physical(TX_RING.0.as_ptr() as usize);
        self.write(TDBAL, tx_phys as u32);
        self.write(TDBAH, (tx_phys >> 32) as u32);
        self.write(TDLEN, (TX_COUNT * 16) as u32);
        self.write(TDH, 0);
        self.write(TDT, 0);
        self.write(TXCTRL, 1);
    }

    /// Transmit one Ethernet frame. Returns the ring index used.
    unsafe fn transmit(&self, frame: &[u8], mac: [u8; 6]) -> usize {
        let descriptor_index = (self.read(TDT) as usize) % TX_COUNT;

        // Build a minimal Ethernet header if the caller did not provide one.
        let mut frame_buffer = [0u8; FRAME_SIZE];
        if frame.len() >= 14 {
            frame_buffer[..frame.len()].copy_from_slice(frame);
        } else {
            let payload = frame;
            frame_buffer[0..6].copy_from_slice(&[0xFF; 6]);
            frame_buffer[6..12].copy_from_slice(&mac);
            frame_buffer[12..14].copy_from_slice(&[0x08, 0x06]);
            frame_buffer[14..14 + payload.len()].copy_from_slice(payload);
        }

        let length = if frame.len() >= 14 { frame.len() } else { frame.len() + 14 };
        let data_ptr = TX_DATA.0[descriptor_index].as_mut_ptr();
        core::ptr::copy_nonoverlapping(frame_buffer.as_ptr(), data_ptr, length);

        let descriptor = core::ptr::addr_of_mut!(TX_RING.0[descriptor_index]);
        (*descriptor).address = Self::physical(TX_DATA.0[descriptor_index].as_ptr() as usize);
        (*descriptor).length = length as u16;
        (*descriptor).cso = 0;
        (*descriptor).css = 0;
        (*descriptor).command = (1 << 0) | (1 << 3); // EOP | report status
        (*descriptor).status = 0;

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.write(TDT, ((descriptor_index + 1) % TX_COUNT) as u32);
        descriptor_index
    }

    /// Return the first received frame, if any descriptor has completed.
    #[allow(dead_code)]
    unsafe fn receive(&self) -> Option<&'static [u8]> {
        let head = self.read(RDH) as usize;
        let descriptor = core::ptr::addr_of!(RX_RING.0[head]);
        if (*descriptor).status & 1 == 0 {
            return None;
        }
        let length = (*descriptor).length as usize;
        let data = RX_DATA.0[head].as_ptr();
        let frame = core::slice::from_raw_parts(data as *const u8, length.min(FRAME_SIZE));
        self.write(RDH, ((head + 1) % RX_COUNT) as u32);
        Some(frame)
    }
}

/// Reply to ICMP echo requests (ping).
fn handle_ipv4(controller: &Controller, frame: &[u8]) {
    let ip = &frame[14..];
    if ip[0] >> 4 != 4 || ip[9] != 1 {
        return; // not IPv4 ICMP
    }
    if ip[12..16] != OUR_IP {
        return; // not addressed to us
    }

    let total_length = u16::from_be_bytes([ip[2], ip[3]]) as usize;
    if total_length < 28 || total_length > frame.len() - 14 {
        return;
    }
    let icmp = &ip[20..total_length];
    if icmp[0] != 8 {
        return; // only echo requests
    }

    let mut reply = [0u8; 128];
    let reply_len = 14 + total_length;
    if reply_len > reply.len() {
        return;
    }

    // Ethernet: swap MAC addresses.
    reply[0..6].copy_from_slice(&frame[6..12]);
    reply[6..12].copy_from_slice(&controller.mac);
    reply[12..14].copy_from_slice(&[0x08, 0x00]);

    // IPv4 header: swap addresses, reset TTL, recompute checksum.
    reply[14..14 + total_length].copy_from_slice(&ip[..total_length]);
    reply[14 + 12..14 + 16].copy_from_slice(&OUR_IP);
    reply[14 + 16..14 + 20].copy_from_slice(&ip[12..16]);
    reply[14 + 8] = 64; // TTL
    reply[14 + 10..14 + 12].copy_from_slice(&[0, 0]);
    let ip_checksum = checksum(&reply[14..14 + 20]);
    reply[14 + 10..14 + 12].copy_from_slice(&ip_checksum.to_be_bytes());

    // ICMP: echo reply (type 0), recompute checksum.
    let icmp_offset = 14 + 20;
    reply[icmp_offset] = 0;
    reply[icmp_offset + 2..icmp_offset + 4].copy_from_slice(&[0, 0]);
    let icmp_length = total_length - 20;
    let icmp_checksum = checksum(&reply[icmp_offset..icmp_offset + icmp_length]);
    reply[icmp_offset + 2..icmp_offset + 4].copy_from_slice(&icmp_checksum.to_be_bytes());

    unsafe { controller.transmit(&reply[..reply_len], controller.mac) };
    diagnostics::debug("[e1000] ICMP echo reply sent");
}

static mut CONTROLLER: Option<Controller> = None;

pub const OUR_IP: [u8; 4] = [10, 0, 2, 15];

/// Internet checksum (RFC 1071).
pub fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut chunks = data.chunks_exact(2);
    for pair in &mut chunks {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        sum += (*last as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Return this controller's MAC address.
#[allow(static_mut_refs)]
pub fn mac() -> [u8; 6] {
    unsafe { CONTROLLER.as_ref().map(|controller| controller.mac).unwrap_or([0; 6]) }
}

/// Return the current link status (STATUS.LU, bit 1).
#[allow(static_mut_refs)]
pub fn link_up() -> bool {
    unsafe { CONTROLLER.as_ref().map(|controller| controller.read(STATUS) & 0x2 != 0).unwrap_or(false) }
}

/// Send a raw Ethernet frame.
#[allow(static_mut_refs)]
pub fn send_frame(frame: &[u8]) -> Result<usize, &'static str> {
    let controller = unsafe { CONTROLLER.as_ref() }.ok_or("no e1000 controller")?;
    if frame.len() > 2048 {
        return Err("frame too large");
    }
    Ok(unsafe { controller.transmit(frame, controller.mac) })
}

/// Poll the receive ring. Called periodically by the scheduler.
#[allow(static_mut_refs)]
pub fn poll() {
    let controller = unsafe { CONTROLLER.as_ref() };
    let controller = match controller {
        Some(controller) => controller,
        None => return,
    };

    if let Some(frame) = unsafe { controller.receive() } {
        if frame.len() >= 42 && frame[12] == 0x08 && frame[13] == 0x06 {
            handle_arp(controller, frame);
        } else if frame.len() >= 34 && frame[12] == 0x08 && frame[13] == 0x00 {
            handle_ipv4(controller, frame);
            crate::net::process_frame(frame);
        }
    }
}

/// Answer ARP requests for our configured address.
fn handle_arp(controller: &Controller, frame: &[u8]) {
    // ARP operation is at Ethernet (14) + 6.
    if frame[20] != 0 || frame[21] != 1 {
        return; // not a request
    }
    if frame[38..42] != OUR_IP {
        return; // not for us
    }

    let mut reply = [0u8; 42];
    reply[0..6].copy_from_slice(&frame[6..12]);       // destination = requester
    reply[6..12].copy_from_slice(&controller.mac);    // source = us
    reply[12..14].copy_from_slice(&[0x08, 0x06]);

    reply[14..16].copy_from_slice(&[0x00, 0x01]);     // Ethernet
    reply[16..18].copy_from_slice(&[0x08, 0x00]);     // IPv4
    reply[18] = 6;                                    // hardware address length
    reply[19] = 4;                                    // protocol address length
    reply[20..22].copy_from_slice(&[0x00, 0x02]);     // ARP reply
    reply[22..28].copy_from_slice(&controller.mac);   // sender hardware
    reply[28..32].copy_from_slice(&OUR_IP);           // sender protocol
    reply[32..38].copy_from_slice(&frame[22..28]);    // target hardware
    reply[38..42].copy_from_slice(&frame[28..32]);    // target protocol

    unsafe { controller.transmit(&reply, controller.mac) };
    diagnostics::debug("[e1000] ARP reply sent");
}

/// Find, initialise and test the first e1000 controller.
pub fn init() {
    for slot in 0..32u8 {
        let ids = pci::read(0, slot, 0, 0);
        if ids & 0xFFFF != 0x8086 || (ids >> 16) != 0x100E {
            continue;
        }

        let command = pci::read(0, slot, 0, 4);
        pci::write(0, slot, 0, 4, command | 0x06); // memory + bus master
        let bar0 = pci::read(0, slot, 0, 0x10) & 0xFFFF_FFF0;
        if bar0 == 0 || bar0 >= 0xFFF0_0000 {
            diagnostics::debug("[e1000] unsupported BAR0; skipping");
            return;
        }

        let controller = Controller {
            mmio: bar0 as usize,
            mac: [0; 6],
        };
        unsafe {
            let low = controller.eeprom_read(0);
            let mid = controller.eeprom_read(1);
            let high = controller.eeprom_read(2);
            let mut mac = [0u8; 6];
            mac[0] = low as u8;
            mac[1] = (low >> 8) as u8;
            mac[2] = mid as u8;
            mac[3] = (mid >> 8) as u8;
            mac[4] = high as u8;
            mac[5] = (high >> 8) as u8;
            let status = controller.read(STATUS);
            controller.setup();

            // Send a gratuitous ARP request to verify the TX path.
            let mut arp = [0u8; 28];
            arp[0..2].copy_from_slice(&[0x00, 0x01]); // Ethernet
            arp[2..4].copy_from_slice(&[0x08, 0x00]); // IPv4
            arp[6..10].copy_from_slice(&[0x00, 0x01, 0x08, 0x00]); // ARP request
            arp[10..16].copy_from_slice(&mac);
            arp[18..24].copy_from_slice(&[0, 0, 0, 0, 0, 0]);
            let index = controller.transmit(&arp, mac);

            diagnostics::puts("[e1000] mmio=");
            diagnostics::put_hex(bar0 as u64);
            diagnostics::puts(" mac=");
            for byte in mac {
                diagnostics::put_hex(byte as u64);
                diagnostics::putc(b' ');
            }
            diagnostics::puts(" link=");
            diagnostics::puts(if status & 0x2 != 0 { "up" } else { "down" });
            diagnostics::puts(" tx_index=");
            diagnostics::put_hex(index as u64);
            diagnostics::puts(" tdt=");
            diagnostics::put_hex(controller.read(TDT) as u64);
            diagnostics::puts("\n");

            CONTROLLER = Some(Controller { mmio: bar0 as usize, mac });
        }
        return;
    }

    diagnostics::debug("[e1000] no controller found");
}
