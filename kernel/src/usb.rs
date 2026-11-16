//! USB host controller detection and UHCI controller initialisation.

use crate::diagnostics;
use crate::io::{inw, outw};
use crate::pci;
use crate::timer;

const CLASS_SERIAL: u8 = 0x0C;
const SUBCLASS_USB: u8 = 0x03;

const USBCMD: u16 = 0x00;
const USBSTS: u16 = 0x02;
const USBINTR: u16 = 0x04;
const FRNUM: u16 = 0x06;
const FRBASEADD: u16 = 0x08;
const PORTSC1: u16 = 0x10;
const PORTSC2: u16 = 0x12;

const TD_ACTIVE: u32 = 1 << 23;
const TD_IOC: u32 = 1 << 24;
const TD_LOW_SPEED: u32 = 1 << 26;
const TD_ERROR: u32 = (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20) | (1 << 22);
const PID_IN: u32 = 0x69;
const PID_OUT: u32 = 0xE1;
const PID_SETUP: u32 = 0x2D;

#[repr(C, align(16))]
struct TransferDescriptor {
    next: u32,
    status: u32,
    token: u32,
    buffer: u32,
}

const EMPTY_TD: TransferDescriptor = TransferDescriptor {
    next: 1,
    status: 0,
    token: 0,
    buffer: 0,
};

/// GET_DESCRIPTOR(Device) setup packet for address 0.
static mut SETUP_PACKET: [u8; 8] = [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00];
static mut DEVICE_DESCRIPTOR: [u8; 18] = [0; 18];
static mut TRANSFER: [TransferDescriptor; 3] = [EMPTY_TD, EMPTY_TD, EMPTY_TD];

#[repr(C, align(16))]
struct QueueHead {
    horizontal_link: u32,
    element_link: u32,
}

#[repr(C, align(4096))]
struct UhciData {
    frame_list: [u32; 1024],
    queue_head: QueueHead,
}

static mut UHCI_DATA: UhciData = UhciData {
    frame_list: [1; 1024],
    queue_head: QueueHead { horizontal_link: 1, element_link: 1 },
};

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

/// Scan all PCI functions for USB controllers.
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
                let command = pci::read(0, slot, function, 4);
                pci::write(0, slot, function, 4, command | 0x06);
                let io_base = (pci::read(0, slot, function, 0x20) & 0xFFF0) as u16;
                diagnostics::puts(" io=");
                diagnostics::put_hex(io_base as u64);
                diagnostics::puts("\n");
                if io_base != 0 {
                    unsafe { init_uhci(io_base) };
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

unsafe fn wait_ticks(count: u64) {
    let target = timer::ticks() + count;
    while timer::ticks() < target {
        core::hint::spin_loop();
    }
}

/// Perform a control GET_DESCRIPTOR transfer to the device at address 0.
unsafe fn get_device_descriptor(base: u16, low_speed: bool) {
    let setup_phys = physical(core::ptr::addr_of!(SETUP_PACKET) as usize);
    let descriptor_phys = physical(core::ptr::addr_of!(DEVICE_DESCRIPTOR) as usize);
    let transfer = core::ptr::addr_of_mut!(TRANSFER);

    for index in 0..3 {
        let td = core::ptr::addr_of_mut!((*transfer)[index]);
        (*td).next = 1;
        (*td).status = 0;
        (*td).token = 0;
        (*td).buffer = 0;
    }

    let status_flags = TD_ACTIVE | TD_IOC | if low_speed { TD_LOW_SPEED } else { 0 };
    let td0 = core::ptr::addr_of_mut!((*transfer)[0]);
    let td1 = core::ptr::addr_of_mut!((*transfer)[1]);
    let td2 = core::ptr::addr_of_mut!((*transfer)[2]);

    // SETUP -> DATA IN -> STATUS OUT.
    (*td0).next = physical(td1 as usize) as u32;
    (*td0).status = status_flags;
    (*td0).token = ((8u32 - 1) << 21) | PID_SETUP;
    (*td0).buffer = setup_phys as u32;

    (*td1).next = physical(td2 as usize) as u32;
    (*td1).status = status_flags;
    (*td1).token = ((18u32 - 1) << 21) | (1 << 19) | PID_IN;
    (*td1).buffer = descriptor_phys as u32;

    (*td2).next = 1;
    (*td2).status = status_flags;
    (*td2).token = ((8u32 - 1) << 21) | (1 << 19) | PID_OUT;
    (*td2).buffer = setup_phys as u32;

    // Attach the transfer while the controller is stopped, then restart it.
    outw(base + USBCMD, 0);
    let data = core::ptr::addr_of_mut!(UHCI_DATA);
    (*core::ptr::addr_of_mut!((*data).queue_head)).element_link = physical(td0 as usize) as u32;
    outw(base + USBCMD, 1);

    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    let deadline = timer::ticks() + 200;
    while (*core::ptr::addr_of!((*transfer)[1])).status & TD_ACTIVE != 0 {
        if timer::ticks() >= deadline {
            break;
        }
        core::hint::spin_loop();
    }

    let status = (*core::ptr::addr_of!((*transfer)[1])).status;
    if status & TD_ACTIVE == 0 && status & TD_ERROR == 0 {
        let descriptor = core::ptr::addr_of!(DEVICE_DESCRIPTOR) as *const u8;
        let vendor = *descriptor.add(8) as u16 | ((*descriptor.add(9) as u16) << 8);
        let product = *descriptor.add(10) as u16 | ((*descriptor.add(11) as u16) << 8);
        diagnostics::puts("[usb] device vid=");
        diagnostics::put_hex(vendor as u64);
        diagnostics::puts(" pid=");
        diagnostics::put_hex(product as u64);
        diagnostics::puts("\n");
    } else {
        diagnostics::puts("[usb] descriptor transfer incomplete (status=");
        diagnostics::put_hex(status as u64);
        diagnostics::puts(")\n");
    }

    (*core::ptr::addr_of_mut!((*data).queue_head)).element_link = 1;
}

/// Initialise a UHCI controller: reset, frame list, queue head, ports.
unsafe fn init_uhci(base: u16) {
    let command = inw(base + USBCMD);
    outw(base + USBCMD, command | (1 << 2)); // host controller reset
    wait_ticks(20);
    outw(base + USBCMD, 0);
    outw(base + USBINTR, 0);
    outw(base + USBSTS, 0xFFFF);

    let data = core::ptr::addr_of_mut!(UHCI_DATA);
    let list_phys = physical(core::ptr::addr_of!((*data).frame_list) as usize);
    let qh_phys = physical(core::ptr::addr_of!((*data).queue_head) as usize);

    for entry in core::ptr::addr_of_mut!((*data).frame_list).as_mut().unwrap().iter_mut() {
        *entry = qh_phys as u32 | 0x2; // pointer type = Queue Head
    }
    (*core::ptr::addr_of_mut!((*data).queue_head)).horizontal_link = 1;
    (*core::ptr::addr_of_mut!((*data).queue_head)).element_link = 1;

    outw(base + FRNUM, 0);
    // 32-bit I/O write to FRBASEADD is two 16-bit writes.
    let low = (list_phys & 0xFFFF) as u16;
    let high = ((list_phys >> 16) & 0xFFFF) as u16;
    crate::io::outl(base + FRBASEADD, ((high as u32) << 16) | low as u32);
    outw(base + USBCMD, 1); // run

    for (index, port) in [PORTSC1, PORTSC2].iter().enumerate() {
        let status = inw(base + *port);
        if status & 1 == 0 {
            continue; // no device connected
        }
        // Clear status-change bits and assert reset.
        outw(base + *port, (status & 0xFFF8) | 0x0004);
        wait_ticks(60);
        outw(base + *port, (status & 0xFFF8) | 0x0005);
        wait_ticks(5);
        let final_status = inw(base + *port);
        diagnostics::puts("[usb] UHCI port ");
        diagnostics::put_hex(index as u64);
        diagnostics::puts(" connected, status=");
        diagnostics::put_hex(final_status as u64);
        diagnostics::puts("\n");

        let low_speed = final_status & 0x100 != 0;
        get_device_descriptor(base, low_speed);
    }

    diagnostics::debug("[usb] UHCI controller running");
}
