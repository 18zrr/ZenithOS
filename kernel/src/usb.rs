//! USB host controller (UHCI) support for HID keyboards and mice.
//!
//! The driver resets and enumerates both root ports, keeps one interrupt queue
//! head per device, and polls the shared interrupt chain from the shell loop.
//! Mice are upgraded to the report protocol when their report descriptor parses,
//! which unlocks wheels and side buttons; keyboards use the HID boot protocol.
//! Port connect-change bits are serviced on every poll, so plugging a device in
//! or out after boot works without restarting the machine.

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
const PORTS: [u16; 2] = [PORTSC1, PORTSC2];

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
const TD_NAK: u32 = 1 << 19;
const TD_ERROR: u32 = (1 << 17) | (1 << 18) | (1 << 20) | (1 << 22);
const TD_ACTUAL_LEN: u32 = 0x7FF;
const TD_ERROR_COUNTER: u32 = 3 << 27;

const PID_IN: u32 = 0x69;
const PID_OUT: u32 = 0xE1;
const PID_SETUP: u32 = 0x2D;

const MAX_TDS: usize = 40;
const MAX_DATA: usize = 256;
const MAX_DEVICES: usize = 4;
const MAX_INTERFACES: usize = 4;

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

const EMPTY_QH: QueueHead = QueueHead {
    horizontal: TD_TERMINATE,
    element: TD_TERMINATE,
    reserved: [0; 2],
};

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
    interrupt_qh: [QueueHead; MAX_DEVICES],
    control_tds: [TransferDescriptor; MAX_TDS],
    interrupt_tds: [TransferDescriptor; MAX_DEVICES],
    reports: [Aligned16; MAX_DEVICES],
    setup: Aligned8,
    status: Aligned8,
    data: [u8; MAX_DATA],
}

static mut CONTROLLER: ControllerData = ControllerData {
    frame_list: [TD_TERMINATE; 1024],
    control_qh: EMPTY_QH,
    interrupt_qh: [EMPTY_QH; MAX_DEVICES],
    control_tds: [EMPTY_TD; MAX_TDS],
    interrupt_tds: [EMPTY_TD; MAX_DEVICES],
    reports: [Aligned16::new(), Aligned16::new(), Aligned16::new(), Aligned16::new()],
    setup: Aligned8::new(),
    status: Aligned8::new(),
    data: [0; MAX_DATA],
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeviceKind {
    Keyboard,
    Mouse,
}

struct UsbDevice {
    used: bool,
    kind: DeviceKind,
    base: u16,
    port: u8,
    address: u8,
    endpoint: u8,
    interface: u8,
    max_packet: usize,
    interval: u8,
    low_speed: bool,
    toggle: bool,
    pending_error: bool,
    reports: u32,
    report_protocol: bool,
    layout: zenith_hid::MouseLayout,
    last_leds: u8,
}

impl UsbDevice {
    const fn new() -> Self {
        UsbDevice {
            used: false,
            kind: DeviceKind::Mouse,
            base: 0,
            port: 0,
            address: 0,
            endpoint: 0,
            interface: 0,
            max_packet: 0,
            interval: 0,
            low_speed: false,
            toggle: false,
            pending_error: false,
            reports: 0,
            report_protocol: false,
            layout: zenith_hid::MouseLayout {
                report_id: 0,
                has_report_id: false,
                buttons: 0,
                button_offset: 0,
                x: zenith_hid::Field { offset: 0, size: 0, signed: false },
                y: zenith_hid::Field { offset: 0, size: 0, signed: false },
                wheel: zenith_hid::Field { offset: 0, size: 0, signed: false },
                pan: zenith_hid::Field { offset: 0, size: 0, signed: false },
            },
            last_leds: 0,
        }
    }
}

static mut DEVICES: [UsbDevice; MAX_DEVICES] = [
    UsbDevice::new(),
    UsbDevice::new(),
    UsbDevice::new(),
    UsbDevice::new(),
];

static mut CONTROLLER_COUNT: u32 = 0;
static mut CONTROLLER_BASE: u16 = 0;
static mut NEXT_ADDRESS: u8 = 1;

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

fn device(index: usize) -> *mut UsbDevice {
    unsafe { core::ptr::addr_of_mut!(DEVICES[index]) }
}

fn controller() -> *mut ControllerData {
    core::ptr::addr_of_mut!(CONTROLLER)
}

/// Scan all PCI functions for USB controllers and initialise the first UHCI
/// controller. Devices present at boot are enumerated immediately.
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

    let base = unsafe { CONTROLLER_BASE };
    if base != 0 {
        for (index, port) in PORTS.iter().enumerate() {
            unsafe { service_port(base, index, *port, true) };
        }
    }
}

/// Reset and initialise a UHCI controller.
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

    let data = controller();
    let control_qh_phys = physical(core::ptr::addr_of_mut!((*data).control_qh) as usize) as u32;
    let list_phys = physical(core::ptr::addr_of_mut!((*data).frame_list) as usize);

    for entry in core::ptr::addr_of_mut!((*data).frame_list)
        .as_mut()
        .unwrap()
        .iter_mut()
    {
        *entry = control_qh_phys | 0x2; // queue head
    }
    (*data).control_qh.horizontal = TD_TERMINATE;
    (*data).control_qh.element = TD_TERMINATE;
    for index in 0..MAX_DEVICES {
        (*data).interrupt_qh[index] = EMPTY_QH;
    }

    outw(base + FRNUM, 0);
    outl(base + FRBASEADD, list_phys as u32);
    outw(base + SOFMOD, 0x40);
    outw(base + USBCMD, CMD_RUN);
    wait_ticks(2);

    CONTROLLER_COUNT = 1;
    CONTROLLER_BASE = base;

    let status = inw(base + USBSTS);
    if status & STS_HCH != 0 {
        diagnostics::puts("[usb] controller halted, status=");
        diagnostics::put_hex(status as u64);
        diagnostics::puts("\n");
    }
    status & STS_HCH == 0
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

unsafe fn interrupt_td_ptr(index: usize) -> *mut TransferDescriptor {
    core::ptr::addr_of_mut!(CONTROLLER.interrupt_tds[index])
}

unsafe fn report_ptr(index: usize) -> *mut u8 {
    core::ptr::addr_of_mut!(CONTROLLER.reports[index].0) as *mut u8
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
    core::ptr::write_volatile(core::ptr::addr_of_mut!((*td).link), TD_TERMINATE);
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
    core::ptr::write_volatile(core::ptr::addr_of_mut!((*td).buffer), buffer as u32);
}

unsafe fn link_td(previous: usize, next: usize) {
    let next_phys = physical(td_ptr(next) as usize) as u32;
    let td = td_ptr(previous);
    core::ptr::write_volatile(core::ptr::addr_of_mut!((*td).link), next_phys | TD_DEPTH);
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

unsafe fn get_report_descriptor(
    base: u16,
    address: u8,
    low_speed: bool,
    interface: u8,
    length: usize,
    ep0_max: usize,
) -> Option<usize> {
    let setup = [
        0x81,
        0x06,
        0x00,
        0x22,
        interface,
        0x00,
        (length & 0xFF) as u8,
        ((length >> 8) & 0xFF) as u8,
    ];
    control_transfer(base, address, low_speed, &setup, true, length, ep0_max)
}

unsafe fn set_address(base: u16, address: u8, low_speed: bool, ep0_max: usize) -> bool {
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

/// SET_REPORT(Output) with a one-byte LED bitmap for a boot keyboard.
unsafe fn set_keyboard_leds(
    base: u16,
    address: u8,
    low_speed: bool,
    interface: u8,
    mask: u8,
    ep0_max: usize,
) -> bool {
    *core::ptr::addr_of_mut!(CONTROLLER.data[0]) = mask;
    let setup = [0x21, 0x09, 0x00, 0x02, interface, 0x00, 0x01, 0x00];
    control_transfer(base, address, low_speed, &setup, false, 1, ep0_max).is_some()
}

#[derive(Clone, Copy)]
struct InterfaceInfo {
    number: u8,
    protocol: u8,
    endpoint: u8,
    max_packet: usize,
    interval: u8,
    hid: bool,
}

impl InterfaceInfo {
    const fn new() -> Self {
        InterfaceInfo {
            number: 0,
            protocol: 0,
            endpoint: 0,
            max_packet: 0,
            interval: 0,
            hid: false,
        }
    }

    fn valid(&self) -> bool {
        self.hid && self.endpoint != 0 && self.max_packet != 0
    }
}

fn has_usage(descriptor: &[u8], page: u8, usage: u8) -> bool {
    let mut index = 0usize;
    while index + 3 < descriptor.len() {
        if descriptor[index] == 0x05
            && descriptor[index + 1] == page
            && descriptor[index + 2] == 0x09
            && descriptor[index + 3] == usage
        {
            return true;
        }
        index += 1;
    }
    false
}

fn allocate_address() -> u8 {
    unsafe {
        for _ in 0..127 {
            let address = NEXT_ADDRESS;
            NEXT_ADDRESS = if NEXT_ADDRESS >= 127 { 1 } else { NEXT_ADDRESS + 1 };
            let mut used = false;
            for index in 0..MAX_DEVICES {
                if (*device(index)).used && (*device(index)).address == address {
                    used = true;
                    break;
                }
            }
            if !used {
                return address;
            }
        }
    }
    1
}

fn has_device_on_port(port: usize) -> bool {
    unsafe {
        for index in 0..MAX_DEVICES {
            let device = &*device(index);
            if device.used && device.port as usize == port {
                return true;
            }
        }
    }
    false
}

fn remove_devices_on_port(base: u16, port: usize) {
    unsafe {
        let mut removed = false;
        for index in 0..MAX_DEVICES {
            let device = &mut *device(index);
            if device.used && device.base == base && device.port as usize == port {
                device.used = false;
                removed = true;
            }
        }
        if removed {
            diagnostics::puts("[usb] device removed from port ");
            diagnostics::put_dec(port as u64);
            diagnostics::puts("\n");
            rebuild_interrupt_chain(base);
        }
    }
}

unsafe fn rebuild_interrupt_chain(base: u16) {
    // Stop the controller while the queue-head chain is edited so a frame
    // cannot walk a half-written link.
    let command = inw(base + USBCMD);
    outw(base + USBCMD, command & !CMD_RUN);
    for _ in 0..100 {
        if inw(base + USBSTS) & STS_HCH != 0 {
            break;
        }
        wait_ticks(1);
    }

    let data = controller();
    for index in 0..MAX_DEVICES {
        (*data).interrupt_qh[index].element = TD_TERMINATE;
        (*data).interrupt_qh[index].horizontal = TD_TERMINATE;
    }

    let mut first = TD_TERMINATE;
    let mut previous: Option<usize> = None;
    for index in 0..MAX_DEVICES {
        if !(*device(index)).used {
            continue;
        }
        let qh_phys =
            physical(core::ptr::addr_of_mut!((*data).interrupt_qh[index]) as usize) as u32 | 0x2;
        if let Some(previous) = previous {
            (*data).interrupt_qh[previous].horizontal = qh_phys;
        } else {
            first = qh_phys;
        }
        previous = Some(index);
    }
    (*data).control_qh.horizontal = first;
    fence(Ordering::SeqCst);

    outw(base + USBCMD, CMD_RUN);
    wait_ticks(1);
}

/// Program the interrupt TD for one device and publish its queue head.
unsafe fn arm_device(index: usize) {
    let device = &mut *device(index);
    let td = interrupt_td_ptr(index);
    let report_phys = physical(report_ptr(index) as usize);
    let packet = device.max_packet.clamp(1, 16);

    core::ptr::write_volatile(core::ptr::addr_of_mut!((*td).link), TD_TERMINATE);
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).status),
        TD_ERROR_COUNTER | TD_ACTIVE | if device.low_speed { TD_LOW_SPEED } else { 0 },
    );
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).token),
        PID_IN
            | ((device.address as u32) << 8)
            | ((device.endpoint as u32) << 15)
            | ((device.toggle as u32) << 19)
            | (((packet - 1) as u32) << 21),
    );
    core::ptr::write_volatile(core::ptr::addr_of_mut!((*td).buffer), report_phys as u32);

    let qh = core::ptr::addr_of_mut!(CONTROLLER.interrupt_qh[index]);
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*qh).element),
        physical(td as usize) as u32,
    );
    fence(Ordering::SeqCst);
}

unsafe fn enqueue_device(
    base: u16,
    port: usize,
    address: u8,
    endpoint: u8,
    interface: u8,
    max_packet: usize,
    interval: u8,
    low_speed: bool,
    kind: DeviceKind,
    report_protocol: bool,
    layout: zenith_hid::MouseLayout,
) -> bool {
    let slot = (0..MAX_DEVICES).find(|index| !(*device(*index)).used);
    let Some(slot) = slot else {
        diagnostics::println("[usb] no free device slot");
        return false;
    };

    let leds = crate::keyboard::led_mask();
    let device = &mut *device(slot);
    device.used = true;
    device.kind = kind;
    device.base = base;
    device.port = port as u8;
    device.address = address;
    device.endpoint = endpoint;
    device.interface = interface;
    device.max_packet = max_packet.clamp(1, 16);
    device.interval = interval;
    device.low_speed = low_speed;
    device.toggle = false;
    device.pending_error = false;
    device.reports = 0;
    device.report_protocol = report_protocol;
    device.layout = layout;
    device.last_leds = leds;

    arm_device(slot);
    rebuild_interrupt_chain(base);

    match kind {
        DeviceKind::Mouse => crate::mouse::usb_ready(),
        DeviceKind::Keyboard => {
            crate::keyboard::usb_ready();
            let _ = set_keyboard_leds(
                base,
                address,
                low_speed,
                interface,
                leds,
                8,
            );
        }
    }

    diagnostics::puts("[usb] HID ");
    diagnostics::puts(match kind {
        DeviceKind::Mouse => "mouse",
        DeviceKind::Keyboard => "keyboard",
    });
    diagnostics::puts(" ready ep=");
    diagnostics::put_hex((0x80 | endpoint) as u64);
    diagnostics::puts(" packet=");
    diagnostics::put_dec(device.max_packet as u64);
    diagnostics::puts(" interval=");
    diagnostics::put_dec(interval as u64);
    diagnostics::puts(" addr=");
    diagnostics::put_dec(address as u64);
    diagnostics::puts(" protocol=");
    diagnostics::puts(if report_protocol { "report" } else { "boot" });
    diagnostics::puts("\n");
    true
}

unsafe fn enumerate_device(base: u16, port: usize, address: u8, low_speed: bool) -> bool {
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
    let device_class = CONTROLLER.data[4];
    let vendor = CONTROLLER.data[8] as u16 | ((CONTROLLER.data[9] as u16) << 8);
    let product = CONTROLLER.data[10] as u16 | ((CONTROLLER.data[11] as u16) << 8);
    diagnostics::puts("[usb] device ");
    diagnostics::put_hex(vendor as u64);
    diagnostics::puts(":");
    diagnostics::put_hex(product as u64);
    diagnostics::puts(" ep0=");
    diagnostics::put_dec(ep0_max as u64);
    diagnostics::puts("\n");

    if device_class == 0x09 {
        diagnostics::println("[usb] hub ignored (hubs are not supported)");
        return false;
    }

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
    let mut current = InterfaceInfo::new();
    let mut interfaces = [InterfaceInfo::new(); MAX_INTERFACES];
    let mut interface_count = 0usize;

    while offset + 2 <= total && offset + 2 <= full_len {
        let length = CONTROLLER.data[offset] as usize;
        if length < 2 || offset + length > total || offset + length > full_len {
            break;
        }
        let kind = CONTROLLER.data[offset + 1];
        match kind {
            0x04 if length >= 9 => {
                if current.hid && interface_count < MAX_INTERFACES {
                    interfaces[interface_count] = current;
                    interface_count += 1;
                }
                current = InterfaceInfo::new();
                let class = CONTROLLER.data[offset + 5];
                let protocol = CONTROLLER.data[offset + 7];
                current.number = CONTROLLER.data[offset + 2];
                current.protocol = protocol;
                current.hid = class == 0x03;
            }
            0x05 if length >= 7 && current.hid => {
                let address = CONTROLLER.data[offset + 2];
                let attributes = CONTROLLER.data[offset + 3];
                let packet = (CONTROLLER.data[offset + 4] as usize)
                    | ((CONTROLLER.data[offset + 5] as usize) << 8);
                if address & 0x80 != 0 && attributes & 0x03 == 0x03 {
                    current.endpoint = address & 0x0F;
                    current.max_packet = (packet & 0x7FF).clamp(1, 16);
                    current.interval = CONTROLLER.data[offset + 6];
                }
            }
            _ => {}
        }
        offset += length;
    }
    if current.hid && interface_count < MAX_INTERFACES {
        interfaces[interface_count] = current;
        interface_count += 1;
    }

    if interface_count == 0 {
        diagnostics::println("[usb] no HID interface found");
        return false;
    }

    let mut chosen: Option<(InterfaceInfo, DeviceKind, bool, zenith_hid::MouseLayout)> = None;
    for candidate in interfaces.iter().take(interface_count) {
        if !candidate.valid() {
            continue;
        }
        let mut kind = match candidate.protocol {
            1 => Some(DeviceKind::Keyboard),
            2 => Some(DeviceKind::Mouse),
            _ => None,
        };
        let mut report_protocol = candidate.protocol == 2;
        let mut layout = UsbDevice::new().layout;

        if kind.is_none() || kind == Some(DeviceKind::Mouse) {
            if let Some(length) =
                get_report_descriptor(base, address, low_speed, candidate.number, 256, ep0_max)
            {
                let descriptor = &CONTROLLER.data[..length.min(MAX_DATA)];
                if let Some(parsed) = zenith_hid::parse_mouse(descriptor) {
                    kind = Some(DeviceKind::Mouse);
                    report_protocol = true;
                    layout = parsed;
                } else if kind.is_none() && has_usage(descriptor, 0x01, 0x06) {
                    kind = Some(DeviceKind::Keyboard);
                }
            }
        }

        let Some(kind) = kind else {
            continue;
        };
        // Prefer an explicit mouse, then a keyboard, then anything usable.
        let better = match chosen {
            None => true,
            Some((_, existing, _, _)) => {
                (existing != DeviceKind::Mouse && kind == DeviceKind::Mouse)
                    || (existing == DeviceKind::Keyboard && kind == DeviceKind::Keyboard && candidate.protocol == 1)
            }
        };
        if better {
            chosen = Some((*candidate, kind, report_protocol, layout));
        }
    }

    let Some((candidate, kind, report_protocol, layout)) = chosen else {
        diagnostics::println("[usb] no supported HID keyboard or mouse found");
        return false;
    };

    if !set_configuration(base, address, low_speed, config_value, ep0_max) {
        diagnostics::println("[usb] SET_CONFIGURATION failed");
        return false;
    }

    let _ = hid_request(base, address, low_speed, candidate.number, 0x0A, 0, ep0_max); // SET_IDLE(0)
    let protocol = if kind == DeviceKind::Mouse && report_protocol {
        0
    } else {
        1
    };
    if !hid_request(base, address, low_speed, candidate.number, 0x0B, protocol, ep0_max) {
        diagnostics::println("[usb] SET_PROTOCOL failed");
    }

    enqueue_device(
        base,
        port,
        address,
        candidate.endpoint,
        candidate.number,
        candidate.max_packet,
        candidate.interval,
        low_speed,
        kind,
        report_protocol,
        layout,
    )
}

unsafe fn service_port(base: u16, port_index: usize, port: u16, force: bool) {
    let status = inw(base + port);
    let changed = status & (PORT_CSC | PORT_PEDC) != 0;
    if !changed && !force {
        return;
    }
    if changed {
        port_clear_change(base, port);
    }

    let status = inw(base + port);
    if status & PORT_CCS == 0 {
        remove_devices_on_port(base, port_index);
        return;
    }
    if has_device_on_port(port_index) {
        return;
    }

    let Some(status) = reset_port(base, port) else {
        diagnostics::puts("[usb] port ");
        diagnostics::put_dec(port_index as u64);
        diagnostics::puts(" did not enumerate\n");
        return;
    };
    let low_speed = status & PORT_LSDA != 0;
    diagnostics::puts("[usb] port ");
    diagnostics::put_dec(port_index as u64);
    diagnostics::puts(" enabled status=");
    diagnostics::put_hex(status as u64);
    diagnostics::puts(" speed=");
    diagnostics::puts(if low_speed { "low" } else { "full" });
    diagnostics::puts("\n");

    let address = allocate_address();
    let _ = enumerate_device(base, port_index, address, low_speed);
    // The reset above raises connect-change again; clear it so an unsupported
    // device (for example a hub) is not reset on every poll.
    port_clear_change(base, port);
}

/// Poll interrupt endpoints and service hotplug. Called from the shell loop.
pub fn poll() {
    let base = unsafe { CONTROLLER_BASE };
    if base == 0 {
        return;
    }

    for index in 0..MAX_DEVICES {
        poll_device(index);
    }

    for (port_index, port) in PORTS.iter().enumerate() {
        let status = unsafe { inw(base + *port) };
        if status & (PORT_CSC | PORT_PEDC) != 0 {
            unsafe { service_port(base, port_index, *port, false) };
        }
    }
}

fn poll_device(index: usize) {
    unsafe {
        let device = &mut *device(index);
        if !device.used {
            return;
        }
        let td = interrupt_td_ptr(index);
        let status = core::ptr::read_volatile(core::ptr::addr_of!((*td).status));
        if status & TD_ACTIVE != 0 {
            return;
        }

        let nak = status & TD_NAK != 0;
        let error = status & TD_ERROR != 0;
        if error || nak {
            if error && !device.pending_error {
                device.pending_error = true;
                diagnostics::puts("[usb] transfer error status=");
                diagnostics::put_hex(status as u64);
                diagnostics::puts("\n");
            }
            // A NAK is normal for an idle interrupt endpoint and must not
            // advance the data toggle.
            rearm(index, !nak);
            return;
        }

        device.pending_error = false;
        let length = (status & TD_ACTUAL_LEN) as usize + 1;
        let report_len = length.min(device.max_packet).min(16);
        if report_len > 0 {
            let report = &*core::ptr::addr_of!(CONTROLLER.reports[index].0);
            match device.kind {
                DeviceKind::Keyboard => {
                    crate::keyboard::usb_report(report, report_len);
                }
                DeviceKind::Mouse if device.report_protocol => {
                    if let Some(decoded) = zenith_hid::decode_mouse(&device.layout, report) {
                        crate::mouse::usb_input(
                            decoded.buttons,
                            decoded.dx,
                            decoded.dy,
                            decoded.wheel,
                        );
                    }
                }
                DeviceKind::Mouse => crate::mouse::usb_report(report, report_len),
            }
            device.reports += 1;
        }

        if device.kind == DeviceKind::Keyboard {
            let leds = crate::keyboard::led_mask();
            if leds != device.last_leds {
                let base = device.base;
                let address = device.address;
                let low_speed = device.low_speed;
                let interface = device.interface;
                if set_keyboard_leds(base, address, low_speed, interface, leds, 8) {
                    device.last_leds = leds;
                }
            }
        }

        rearm(index, true);
    }
}

unsafe fn rearm(index: usize, advance_toggle: bool) {
    let device = &mut *device(index);
    if !device.used {
        return;
    }
    if advance_toggle {
        device.toggle = !device.toggle;
    }
    let td = interrupt_td_ptr(index);
    let report_phys = physical(report_ptr(index) as usize);
    let packet = device.max_packet.clamp(1, 16);
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).status),
        TD_ERROR_COUNTER | TD_ACTIVE | if device.low_speed { TD_LOW_SPEED } else { 0 },
    );
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*td).token),
        PID_IN
            | ((device.address as u32) << 8)
            | ((device.endpoint as u32) << 15)
            | ((device.toggle as u32) << 19)
            | (((packet - 1) as u32) << 21),
    );
    core::ptr::write_volatile(core::ptr::addr_of_mut!((*td).buffer), report_phys as u32);
    let qh = core::ptr::addr_of_mut!(CONTROLLER.interrupt_qh[index]);
    core::ptr::write_volatile(
        core::ptr::addr_of_mut!((*qh).element),
        physical(td as usize) as u32,
    );
    fence(Ordering::SeqCst);
}

/// Whether a USB HID mouse has been enumerated and is being polled.
pub fn present() -> bool {
    unsafe {
        for index in 0..MAX_DEVICES {
            let device = &*device(index);
            if device.used && device.kind == DeviceKind::Mouse {
                return true;
            }
        }
    }
    false
}

/// Whether a USB HID keyboard has been enumerated and is being polled.
pub fn keyboard_present() -> bool {
    unsafe {
        for index in 0..MAX_DEVICES {
            let device = &*device(index);
            if device.used && device.kind == DeviceKind::Keyboard {
                return true;
            }
        }
    }
    false
}
