//! USB host controller (UHCI) support and HID boot-mouse input.
//!
//! The driver performs a real port reset, enumerates the device through
//! control transfers on endpoint 0, selects the HID boot protocol, and polls
//! the interrupt IN endpoint from the shell loop. Reports are passed to the
//! mouse module, which merges them with PS/2 input for the window manager.

use core::sync::atomic::{fence, Ordering};

use crate::diagnostics;
use crate::io::{inw, outl, outw};
use crate::pci;
use crate::timer;

const CLASS_SERIAL: u8 = 0x0C;
const SUBCLASS_USB: u8 = 0x03;

const USBCMD: u16 = 0x00;
const USBSTS: u16 = 0x02;
const USBINTR: u16 = 0x04;
const FRNUM: u16 = 0x06;
const FRBASEADD: u16 = 0x08;
const SOFMOD: u16 = 0x0C;
const PORTSC1: u16 = 0x10;
const PORTSC2: u16 = 0x12;

const CMD_RUN: u16 = 1 << 0;
const CMD_HCRESET: u16 = 1 << 1;

const STS_HCH: u16 = 1 << 5;

const PORT_CCS: u16 = 1 << 0;
const PORT_CSC: u16 = 1 << 1;
const PORT_PED: u16 = 1 << 2;
const PORT_PEDC: u16 = 1 << 3;
const PORT_LSDA: u16 = 1 << 8;
const PORT_PR: u16 = 1 << 9;
const PORT_RWC: u16 = PORT_CSC | PORT_PEDC;

const TD_TERMINATE: u32 = 1;
const TD_DEPTH: u32 = 1 << 2;
const TD_ACTIVE: u32 = 1 << 23;
const TD_LOW_SPEED: u32 = 1 << 26;
const TD_ERROR: u32 = (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20) | (1 << 22);
const TD_ACTUAL_LEN: u32 = 0x7FF;
const TD_NAK: u32 = 1 << 19;
const TD_ERROR_COUNTER: u32 = 3 << 27;

const PID_IN: u32 = 0x69;
const PID_OUT: u32 = 0xE1;
const PID_SETUP: u32 = 0x2D;

const MAX_TDS: usize = 40;
const MAX_DATA: usize = 256;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct TransferDescriptor {
    link: u32,
    status: u32,
    token: u32,
    buffer: u32,
}

const EMPTY_TD: TransferDescriptor = TransferDescriptor {
    link: TD_TERMINATE,
    status: 0,
    token: 0,
    buffer: 0,
};

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct QueueHead {
    horizontal: u32,
    element: u32,
    // QEMU (and real hardware) mask link pointers with ~0xF, so queue heads
    // must occupy a full 16 bytes or the next queue head aliases this one.
    reserved: [u32; 2],
}

#[repr(C, align(64))]
struct Aligned8([u8; 8]);

#[repr(C, align(64))]
struct Aligned16([u8; 16]);

impl Aligned8 {
    const fn new() -> Self {
        Aligned8([0; 8])
    }
}

impl Aligned16 {
    const fn new() -> Self {
        Aligned16([0; 16])
    }
}

#[repr(C, align(4096))]
struct ControllerData {
    frame_list: [u32; 1024],
    control_qh: QueueHead,
    interrupt_qh: QueueHead,
    control_tds: [TransferDescriptor; MAX_TDS],
    mouse_td: TransferDescriptor,
    setup: Aligned8,
    status: Aligned8,
    report: Aligned16,
    data: [u8; MAX_DATA],
}

static mut CONTROLLER: ControllerData = ControllerData {
    frame_list: [TD_TERMINATE; 1024],
    control_qh: QueueHead {
        horizontal: TD_TERMINATE,
        element: TD_TERMINATE,
        reserved: [0; 2],
    },
    interrupt_qh: QueueHead {
        horizontal: TD_TERMINATE,
        element: TD_TERMINATE,
        reserved: [0; 2],
    },
    control_tds: [EMPTY_TD; MAX_TDS],
    mouse_td: EMPTY_TD,
    setup: Aligned8::new(),
    status: Aligned8::new(),
    report: Aligned16::new(),
    data: [0; MAX_DATA],
};

#[derive(Clone, Copy)]
struct UsbMouse {
    ready: bool,
    base: u16,
    address: u8,
    endpoint: u8,
    max_packet: usize,
    toggle: bool,
    low_speed: bool,
    pending_error: bool,
    reports: u32,
}

static mut USB_MOUSE: UsbMouse = UsbMouse {
    ready: false,
    base: 0,
    address: 0,
    endpoint: 0,
    max_packet: 0,
    toggle: false,
    low_speed: false,
    pending_error: false,
    reports: 0,
};

static mut CONTROLLER_COUNT: u32 = 0;

fn physical(address: usize) -> u64 {
    crate::limine::physical(address)
}

fn class_name(prog_if: u8) -> &'static str {
    match prog_if {
        0x00 => "UHCI",
        0x10 => "OHCI",
        0x20 => "EHCI",
        0x30 => "xHCI",
        _ => "unknown",
    }
}

unsafe fn wait_ticks(count: u64) {
    let target = timer::ticks() + count;
    while timer::ticks() < target {
        core::hint::spin_loop();
    }
}

/// Scan all PCI functions for USB controllers and initialise the first UHCI
/// controller that has a device attached.
pub fn detect() {
    let mut count = 0u32;
    for slot in 0..32u8 {
        for function in 0..8u8 {
            let ids = pci::read(0, slot, function, 0);
            if ids == 0xFFFF_FFFF || ids & 0xFFFF == 0xFFFF {
                continue;
            }
            let class_reg = pci::read(0, slot, function, 8);
            let class = (class_reg >> 24) as u8;
            let subclass = (class_reg >> 16) as u8;
            let prog_if = (class_reg >> 8) as u8;
            if class != CLASS_SERIAL || subclass != SUBCLASS_USB {
                continue;
            }

            count += 1;
            diagnostics::puts("[usb] controller ");
            diagnostics::put_hex(slot as u64);
            diagnostics::puts(".");
            diagnostics::put_hex(function as u64);
            diagnostics::puts(" ");
            diagnostics::puts(class_name(prog_if));

            if prog_if == 0x00 {
                // Enable I/O space and bus mastering; both are required for
                // the UHCI register window and DMA transfers.
                let command = pci::read(0, slot, function, 4) & 0xFFFF;
                pci::write(0, slot, function, 4, command | 0x07);
                // Disable PS/2 legacy emulation so HID reports reach the
                // driver. Bits 0-3 enable legacy routing; the upper bits are
                // write-1-to-clear status flags.
                let legsup = pci::read16(0, slot, function, 0xC0);
                pci::write16(0, slot, function, 0xC0, (legsup & !0x000F) | 0x8F00);
                let io_base = (pci::read(0, slot, function, 0x20) & 0xFFE0) as u16;
                diagnostics::puts(" io=");
                diagnostics::put_hex(io_base as u64);
                diagnostics::puts("\n");
                if io_base != 0 {
                    let initialized = unsafe { init_uhci(io_base) };
                    if initialized {
                        diagnostics::println("[usb] UHCI controller running");
                    }
                }
            } else {
                diagnostics::puts("\n");
            }
        }
    }

    diagnostics::puts("[usb] controllers found: ");
    diagnostics::put_hex(count as u64);
    diagnostics::puts("\n");
}

/// Reset and initialise a UHCI controller, then enumerate both root ports.
unsafe fn init_uhci(base: u16) -> bool {
    if CONTROLLER_COUNT != 0 {
        diagnostics::println("[usb] extra UHCI controller ignored");
        return false;
    }

    // Host controller reset (USBCMD bit 1; bit 2 is a global port reset).
    outw(base + USBCMD, 0);
    outw(base + USBCMD, CMD_HCRESET);
    for _ in 0..100 {
        if inw(base + USBCMD) & CMD_HCRESET == 0 {
            break;
        }
        wait_ticks(1);
    }
    outw(base + USBCMD, 0);
    outw(base + USBINTR, 0);
    outw(base + USBSTS, 0xFFFF);

    let data = core::ptr::addr_of_mut!(CONTROLLER);
    let control_qh_phys =
        physical(core::ptr::addr_of_mut!((*data).control_qh) as usize) as u32;
    let interrupt_qh_phys =
        physical(core::ptr::addr_of_mut!((*data).interrupt_qh) as usize) as u32;
    let list_phys = physical(core::ptr::addr_of_mut!((*data).frame_list) as usize);

    for entry in core::ptr::addr_of_mut!((*data).frame_list)
        .as_mut()
        .unwrap()
        .iter_mut()
    {
        *entry = control_qh_phys | 0x2; // queue head
    }
    (*core::ptr::addr_of_mut!((*data).control_qh)).horizontal = interrupt_qh_phys | 0x2;
    (*core::ptr::addr_of_mut!((*data).control_qh)).element = TD_TERMINATE;
    (*core::ptr::addr_of_mut!((*data).interrupt_qh)).horizontal = TD_TERMINATE;
    (*core::ptr::addr_of_mut!((*data).interrupt_qh)).element = TD_TERMINATE;

    outw(base + FRNUM, 0);
    outl(base + FRBASEADD, list_phys as u32);
    outw(base + SOFMOD, 0x40);
    outw(base + USBCMD, CMD_RUN);
    wait_ticks(2);

    CONTROLLER_COUNT = 1;
    (*core::ptr::addr_of_mut!(USB_MOUSE)).base = base;

    let status = inw(base + USBSTS);
    if status & STS_HCH != 0 {
        diagnostics::puts("[usb] controller halted, status=");
        diagnostics::put_hex(status as u64);
        diagnostics::puts("\n");
    }

    for (index, port) in [PORTSC1, PORTSC2].iter().enumerate() {
        diagnostics::puts("[usb] port ");
        diagnostics::put_dec(index as u64);
        diagnostics::puts(" initial=");
        diagnostics::put_hex(inw(base + *port) as u64);
        diagnostics::puts("\n");

        let Some(status) = reset_port(base, *port) else {
            continue;
        };
        if status & PORT_CCS == 0 {
            continue;
        }
        let low_speed = status & PORT_LSDA != 0;
        diagnostics::puts("[usb] port ");
        diagnostics::put_dec(index as u64);
        diagnostics::puts(" enabled status=");
        diagnostics::put_hex(status as u64);
        diagnostics::puts(" speed=");
        diagnostics::puts(if low_speed { "low" } else { "full" });
        diagnostics::puts("\n");

        let address = (index + 1) as u8;
        if enumerate_mouse(base, address, low_speed) {
            return true;
        }
    }
    false
}

/// Clear write-1-to-clear port bits, preserving the rest.
unsafe fn port_set(base: u16, port: u16, bits: u16) {
    let mut status = inw(base + port);
    status |= bits;
    status &= !PORT_RWC;
    outw(base + port, status);
}

unsafe fn port_clr(base: u16, port: u16, bits: u16) {
    let mut status = inw(base + port);
    status &= !PORT_RWC;
    status &= !bits;
    status |= PORT_RWC & bits;
    outw(base + port, status);
}

unsafe fn port_clear_change(base: u16, port: u16) {
    let status = inw(base + port);
    outw(base + port, (status & !PORT_RWC) | (status & PORT_RWC));
}

/// Reset a root port and wait for it to enable.
unsafe fn reset_port(base: u16, port: u16) -> Option<u16> {
    let status = inw(base + port);
    if status & PORT_CCS == 0 {
        return None;
    }
    port_clear_change(base, port);

    port_set(base, port, PORT_PR);
    wait_ticks(5); // 50 ms reset
    port_clr(base, port, PORT_PR);
    wait_ticks(2); // 20 ms recovery

    let mut status = inw(base + port);
    for _ in 0..10 {
        if status & PORT_CCS == 0 {
            return None;
        }
        if status & PORT_RWC != 0 {
            port_clear_change(base, port);
        }
        if status & PORT_PED != 0 {
            return Some(status);
        }
        // Some controllers need the port enabled explicitly after reset.
        port_set(base, port, PORT_PED);
        wait_ticks(1);
        status = inw(base + port);
    }

    if status & PORT_PED != 0 {
        Some(status)
    } else {
        diagnostics::puts("[usb] port reset did not enable, status=");
        diagnostics::put_hex(status as u64);
        diagnostics::puts("\n");
        None
    }
}

unsafe fn td_ptr(index: usize) -> *mut TransferDescriptor {
    core::ptr::addr_of_mut!(CONTROLLER.control_tds[index])
}

unsafe fn mouse_td_ptr() -> *mut TransferDescriptor {
    core::ptr::addr_of_mut!(CONTROLLER.mouse_td)
}

unsafe fn init_td(
    index: usize,
    pid: u32,
    address: u8,
    endpoint: u8,
    toggle: bool,
    length: usize,
    buffer: u64,
    low_speed: bool,
) {
    let td = td_ptr(index);
    let encoded_length = if length == 0 {
        0x7FF
    } else {
        (length - 1) as u32
    };
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).link),
        TD_TERMINATE,
    );
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).status),
        TD_ERROR_COUNTER | TD_ACTIVE | if low_speed { TD_LOW_SPEED } else { 0 },
    );
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).token),
        pid | ((address as u32) << 8)
            | ((endpoint as u32) << 15)
            | ((toggle as u32) << 19)
            | (encoded_length << 21),
    );
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).buffer),
        buffer as u32,
    );
}

unsafe fn link_td(previous: usize, next: usize) {
    let next_phys = physical(td_ptr(next) as usize) as u32;
    let td = td_ptr(previous);
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).link),
        next_phys | TD_DEPTH,
    );
}

/// Submit a control transfer on endpoint 0. Data always travels through the
/// shared `CONTROLLER.data` buffer, so callers fill it for OUT stages and read
/// it after IN stages.
unsafe fn control_transfer(
    _base: u16,
    address: u8,
    low_speed: bool,
    setup: &[u8; 8],
    data_in: bool,
    data_len: usize,
    ep0_max: usize,
) -> Option<usize> {
    if data_len > MAX_DATA {
        return None;
    }

    for (index, byte) in setup.iter().enumerate() {
        *core::ptr::addr_of_mut!(CONTROLLER.setup.0[index]) = *byte;
    }
    let setup_phys = physical(core::ptr::addr_of_mut!(CONTROLLER.setup.0) as usize);
    let data_phys = physical(core::ptr::addr_of_mut!(CONTROLLER.data) as usize);
    let status_phys = physical(core::ptr::addr_of_mut!(CONTROLLER.status.0) as usize);

    let mut index = 0usize;
    init_td(index, PID_SETUP, address, 0, false, 8, setup_phys, low_speed);
    let setup_index = index;
    let mut previous = setup_index;
    index += 1;

    let packet = ep0_max.clamp(8, 64);
    let mut offset = 0usize;
    let mut toggle = true;
    while offset < data_len {
        if index + 1 >= MAX_TDS {
            return None;
        }
        let chunk = (data_len - offset).min(packet);
        init_td(
            index,
            if data_in { PID_IN } else { PID_OUT },
            address,
            0,
            toggle,
            chunk,
            data_phys + offset as u64,
            low_speed,
        );
        link_td(previous, index);
        previous = index;
        index += 1;
        offset += chunk;
        toggle = !toggle;
    }

    if index >= MAX_TDS {
        return None;
    }
    // The status stage runs in the opposite direction to the data stage. A
    // control transfer with no data stage uses an IN status stage.
    let status_pid = if data_in { PID_OUT } else { PID_IN };
    init_td(index, status_pid, address, 0, true, 0, status_phys, low_speed);
    link_td(previous, index);
    let status_index = index;

    let control_qh = core::ptr::addr_of_mut!(CONTROLLER.control_qh);
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*control_qh).element),
        physical(td_ptr(setup_index) as usize) as u32,
    );
    fence(Ordering::SeqCst);

    let deadline = timer::ticks() + 200;
    loop {
        let status = core::ptr::read_volatile(core::ptr::addr_of!((*td_ptr(status_index)).status));
        if status & TD_ACTIVE == 0 {
            break;
        }
        // A stall or a timeout stops the queue at the failing TD, leaving the
        // status TD active. Detect that instead of waiting for the timeout.
        let mut stopped = false;
        for td_index in setup_index..status_index {
            let status =
                core::ptr::read_volatile(core::ptr::addr_of!((*td_ptr(td_index)).status));
            if status & TD_ERROR != 0 && status & TD_NAK == 0 {
                stopped = true;
                break;
            }
        }
        if stopped || timer::ticks() >= deadline {
            break;
        }
        core::hint::spin_loop();
    }

    let status_td_status =
        core::ptr::read_volatile(core::ptr::addr_of!((*td_ptr(status_index)).status));
    let mut failed = status_td_status & TD_ACTIVE != 0 || status_td_status & TD_ERROR != 0;
    let mut actual = 0usize;
    for td_index in setup_index..=status_index {
        let status = core::ptr::read_volatile(core::ptr::addr_of!((*td_ptr(td_index)).status));
        if status & TD_ERROR != 0 {
            failed = true;
        }
        if td_index > setup_index && td_index < status_index && data_in {
            // UHCI stores (actual length - 1) in the status field.
            actual += (status & TD_ACTUAL_LEN) as usize + 1;
        }
    }

    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*control_qh).element),
        TD_TERMINATE,
    );
    fence(Ordering::SeqCst);

    if failed {
        diagnostics::puts("[usb] control transfer failed status=");
        diagnostics::put_hex(status_td_status as u64);
        diagnostics::puts("\n");
        None
    } else if data_in {
        Some(actual.min(data_len))
    } else {
        Some(data_len)
    }
}

unsafe fn get_descriptor(
    base: u16,
    address: u8,
    low_speed: bool,
    kind: u8,
    length: usize,
    ep0_max: usize,
) -> Option<usize> {
    // wValue holds the descriptor type in the high byte and the descriptor
    // index in the low byte.
    let setup = [
        0x80,
        0x06,
        0x00,
        kind,
        0x00,
        0x00,
        (length & 0xFF) as u8,
        ((length >> 8) & 0xFF) as u8,
    ];
    control_transfer(base, address, low_speed, &setup, true, length, ep0_max)
}

unsafe fn set_address(
    base: u16,
    address: u8,
    low_speed: bool,
    ep0_max: usize,
) -> bool {
    let setup = [0x00, 0x05, address, 0x00, 0x00, 0x00, 0x00, 0x00];
    control_transfer(base, 0, low_speed, &setup, false, 0, ep0_max).is_some()
}

unsafe fn set_configuration(
    base: u16,
    address: u8,
    low_speed: bool,
    value: u8,
    ep0_max: usize,
) -> bool {
    let setup = [0x00, 0x09, value, 0x00, 0x00, 0x00, 0x00, 0x00];
    control_transfer(base, address, low_speed, &setup, false, 0, ep0_max).is_some()
}

unsafe fn hid_request(
    base: u16,
    address: u8,
    low_speed: bool,
    interface: u8,
    request: u8,
    value: u16,
    ep0_max: usize,
) -> bool {
    let setup = [
        0x21,
        request,
        (value & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        interface,
        0x00,
        0x00,
        0x00,
    ];
    control_transfer(base, address, low_speed, &setup, false, 0, ep0_max).is_some()
}

unsafe fn enumerate_mouse(base: u16, address: u8, low_speed: bool) -> bool {
    // First read the 8-byte prefix to learn the endpoint-0 packet size.
    let Some(prefix_len) = get_descriptor(base, 0, low_speed, 0x01, 8, 8) else {
        diagnostics::println("[usb] device descriptor failed");
        return false;
    };
    if prefix_len < 8 {
        diagnostics::println("[usb] short device descriptor");
        return false;
    }
    let ep0_max = (CONTROLLER.data[7] as usize).clamp(8, 64);

    let Some(device_len) = get_descriptor(base, 0, low_speed, 0x01, 18, ep0_max) else {
        diagnostics::println("[usb] full device descriptor failed");
        return false;
    };
    if device_len < 18 {
        diagnostics::println("[usb] short full device descriptor");
        return false;
    }
    let vendor = CONTROLLER.data[8] as u16 | ((CONTROLLER.data[9] as u16) << 8);
    let product = CONTROLLER.data[10] as u16 | ((CONTROLLER.data[11] as u16) << 8);
    diagnostics::puts("[usb] device ");
    diagnostics::put_hex(vendor as u64);
    diagnostics::puts(":");
    diagnostics::put_hex(product as u64);
    diagnostics::puts(" ep0=");
    diagnostics::put_dec(ep0_max as u64);
    diagnostics::puts("\n");

    if !set_address(base, address, low_speed, ep0_max) {
        diagnostics::println("[usb] SET_ADDRESS failed");
        return false;
    }
    wait_ticks(1);

    let Some(config_len) = get_descriptor(base, address, low_speed, 0x02, 9, ep0_max) else {
        diagnostics::println("[usb] configuration prefix failed");
        return false;
    };
    if config_len < 9 {
        diagnostics::println("[usb] short configuration prefix");
        return false;
    }
    let total = (CONTROLLER.data[2] as usize) | ((CONTROLLER.data[3] as usize) << 8);
    let total = total.clamp(9, MAX_DATA);
    let Some(full_len) = get_descriptor(base, address, low_speed, 0x02, total, ep0_max) else {
        diagnostics::println("[usb] full configuration failed");
        return false;
    };
    if full_len < 9 {
        diagnostics::println("[usb] short configuration");
        return false;
    }

    let config_value = CONTROLLER.data[5];
    let mut offset = 0usize;
    let mut interface = 0u8;
    let mut endpoint = 0u8;
    let mut max_packet = 0usize;
    let mut interval = 0u8;
    let mut hid_mouse = false;
    let mut found_endpoint = false;

    while offset + 2 <= total && offset + 2 <= full_len {
        let length = CONTROLLER.data[offset] as usize;
        if length < 2 || offset + length > total || offset + length > full_len {
            break;
        }
        let kind = CONTROLLER.data[offset + 1];
        match kind {
            0x04 if length >= 9 => {
                interface = CONTROLLER.data[offset + 2];
                let class = CONTROLLER.data[offset + 5];
                let subclass = CONTROLLER.data[offset + 6];
                let protocol = CONTROLLER.data[offset + 7];
                hid_mouse = class == 0x03 && (subclass == 0x01 || protocol == 0x02);
                diagnostics::puts("[usb] interface class=");
                diagnostics::put_dec(class as u64);
                diagnostics::puts("/");
                diagnostics::put_dec(subclass as u64);
                diagnostics::puts("/");
                diagnostics::put_dec(protocol as u64);
                diagnostics::puts(if hid_mouse { " (HID mouse)\n" } else { "\n" });
            }
            0x05 if length >= 7 && hid_mouse => {
                let address = CONTROLLER.data[offset + 2];
                let attributes = CONTROLLER.data[offset + 3];
                let packet = (CONTROLLER.data[offset + 4] as usize)
                    | ((CONTROLLER.data[offset + 5] as usize) << 8);
                if address & 0x80 != 0 && attributes & 0x03 == 0x03 {
                    endpoint = address & 0x0F;
                    max_packet = (packet & 0x7FF).clamp(1, 16);
                    interval = CONTROLLER.data[offset + 6];
                    found_endpoint = true;
                }
            }
            _ => {}
        }
        offset += length;
    }

    if !hid_mouse || !found_endpoint || endpoint == 0 || max_packet == 0 {
        diagnostics::println("[usb] no HID mouse interface found");
        return false;
    }

    if !set_configuration(base, address, low_speed, config_value, ep0_max) {
        diagnostics::println("[usb] SET_CONFIGURATION failed");
        return false;
    }

    // Boot protocol makes the report layout predictable (3-4 bytes).
    let _ = hid_request(base, address, low_speed, interface, 0x0A, 0, ep0_max);
    if !hid_request(base, address, low_speed, interface, 0x0B, 0, ep0_max) {
        diagnostics::println("[usb] SET_PROTOCOL(boot) failed");
    }

    let report_phys = physical(core::ptr::addr_of_mut!(CONTROLLER.report.0) as usize);
    let mouse_td = mouse_td_ptr();
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*mouse_td).link),
        TD_TERMINATE,
    );
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*mouse_td).status),
        TD_ERROR_COUNTER | TD_ACTIVE | if low_speed { TD_LOW_SPEED } else { 0 },
    );
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*mouse_td).token),
        PID_IN
            | ((address as u32) << 8)
            | ((endpoint as u32) << 15)
            | (((max_packet - 1) as u32) << 21),
    );
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*mouse_td).buffer),
        report_phys as u32,
    );

    let mouse = core::ptr::addr_of_mut!(USB_MOUSE);
    (*mouse).ready = true;
    (*mouse).base = base;
    (*mouse).address = address;
    (*mouse).endpoint = endpoint;
    (*mouse).max_packet = max_packet;
    (*mouse).toggle = false;
    (*mouse).low_speed = low_speed;
    (*mouse).pending_error = false;
    (*mouse).reports = 0;

    let interrupt_qh = core::ptr::addr_of_mut!(CONTROLLER.interrupt_qh);
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*interrupt_qh).element),
        physical(mouse_td as usize) as u32,
    );
    fence(Ordering::SeqCst);

    crate::mouse::usb_ready();
    diagnostics::puts("[usb] HID mouse ready ep=");
    diagnostics::put_hex((0x80 | endpoint) as u64);
    diagnostics::puts(" packet=");
    diagnostics::put_dec(max_packet as u64);
    diagnostics::puts(" interval=");
    diagnostics::put_dec(interval as u64);
    diagnostics::puts(" addr=");
    diagnostics::put_dec(address as u64);
    diagnostics::puts("\n");
    true
}

/// Poll the interrupt IN endpoint. Called from the shell loop.
pub fn poll() {
    unsafe {
        let mouse = core::ptr::addr_of_mut!(USB_MOUSE);
        if !(*mouse).ready {
            return;
        }
        let td = mouse_td_ptr();
        let status = core::ptr::read_volatile(core::ptr::addr_of!((*td).status));
        if status & TD_ACTIVE != 0 {
            return;
        }

        if status & TD_ERROR != 0 {
            if !(*mouse).pending_error {
                (*mouse).pending_error = true;
                diagnostics::puts("[usb] mouse transfer error status=");
                diagnostics::put_hex(status as u64);
                diagnostics::puts("\n");
            }
            rearm();
            return;
        }

        let length = (status & TD_ACTUAL_LEN) as usize;
        if length >= 3 {
            let report = &*core::ptr::addr_of!(CONTROLLER.report.0);
            crate::mouse::usb_report(report, length.min(16));
            (*mouse).reports += 1;
        }
        rearm();
    }
}

unsafe fn rearm() {
    let mouse = core::ptr::addr_of_mut!(USB_MOUSE);
    if !(*mouse).ready {
        return;
    }
    (*mouse).toggle = !(*mouse).toggle;
    (*mouse).pending_error = false;
    let td = mouse_td_ptr();
    let report_phys = physical(core::ptr::addr_of_mut!(CONTROLLER.report.0) as usize);
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).status),
        TD_ERROR_COUNTER
            | TD_ACTIVE
            | if (*mouse).low_speed { TD_LOW_SPEED } else { 0 },
    );
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).token),
        PID_IN
            | (((*mouse).address as u32) << 8)
            | (((*mouse).endpoint as u32) << 15)
            | (((*mouse).toggle as u32) << 19)
            | ((((*mouse).max_packet - 1) as u32) << 21),
    );
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).buffer),
        report_phys as u32,
    );
    let interrupt_qh = core::ptr::addr_of_mut!(CONTROLLER.interrupt_qh);
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*interrupt_qh).element),
        physical(td as usize) as u32,
    );
    fence(Ordering::SeqCst);
}

/// Whether a USB HID mouse has been enumerated and is being polled.
pub fn present() -> bool {
    unsafe { (*core::ptr::addr_of!(USB_MOUSE)).ready }
}
