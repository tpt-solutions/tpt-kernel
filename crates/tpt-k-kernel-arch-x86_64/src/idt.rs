// SPDX-License-Identifier: MIT OR Apache-2.0
//! The interrupt descriptor table, with entries for the CPU exceptions (vectors
//! 0 to 31) and the hardware lines remapped to vectors 32 to 47. Each stub
//! pushes a vector number, and a zero error code where the CPU does not push
//! one, then jumps to a common entry. The common entry saves the
//! general-purpose registers and calls [`exception_dispatch`].
//!
//! Breakpoints (vector 3) return to the interrupted code. Other exceptions are
//! fatal: they print a report and stop the VM. A hardware line calls the handler
//! registered with [`set_irq_handler`]. The handler acknowledges the line.

use core::arch::{asm, global_asm};
use core::fmt::Write as _;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::gdt::KERNEL_CODE;
use crate::{pic, qemu, serial};

/// First vector used by the hardware lines.
pub const IRQ_BASE: u8 = 32;
/// Hardware lines the table can route (the master and slave PICs).
pub const IRQ_LINES: usize = 16;
/// Entries the stubs cover: the exceptions, then the hardware lines.
const STUB_COUNT: usize = EXCEPTION_VECTORS + IRQ_LINES;

/// Number of CPU exception vectors.
const EXCEPTION_VECTORS: usize = 32;
/// Number of IDT entries. Entries past the exceptions stay not-present.
const IDT_ENTRIES: usize = 256;

/// Present, DPL 0, 64-bit interrupt gate.
const INTERRUPT_GATE: u8 = 0x8E;

/// A 16-byte interrupt gate descriptor.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Gate {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

const ABSENT: Gate = Gate {
    offset_low: 0,
    selector: 0,
    ist: 0,
    type_attr: 0,
    offset_mid: 0,
    offset_high: 0,
    reserved: 0,
};

impl Gate {
    fn interrupt(handler: u64, selector: u16) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            ist: 0,
            type_attr: INTERRUPT_GATE,
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

/// The table. It is written once by [`load`], before interrupts are enabled.
static mut IDT: [Gate; IDT_ENTRIES] = [ABSENT; IDT_ENTRIES];

/// The operand of `lidt`.
#[repr(C, packed)]
struct Pointer {
    limit: u16,
    base: u64,
}

/// What the common entry saved, from the lowest address up. The CPU pushed
/// `rip`, `cs` and `rflags` on the interrupt. The stub pushed the vector and
/// the error code.
#[repr(C)]
pub struct TrapFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
}

global_asm!(
    ".macro ISR_STUB n, has_error",
    ".global isr_stub_\\n",
    "isr_stub_\\n:",
    ".if \\has_error == 0",
    "push 0",
    ".endif",
    "push \\n",
    "jmp isr_common",
    ".endm",
    "ISR_STUB 0, 0",
    "ISR_STUB 1, 0",
    "ISR_STUB 2, 0",
    "ISR_STUB 3, 0",
    "ISR_STUB 4, 0",
    "ISR_STUB 5, 0",
    "ISR_STUB 6, 0",
    "ISR_STUB 7, 0",
    "ISR_STUB 8, 1",
    "ISR_STUB 9, 0",
    "ISR_STUB 10, 1",
    "ISR_STUB 11, 1",
    "ISR_STUB 12, 1",
    "ISR_STUB 13, 1",
    "ISR_STUB 14, 1",
    "ISR_STUB 15, 0",
    "ISR_STUB 16, 0",
    "ISR_STUB 17, 1",
    "ISR_STUB 18, 0",
    "ISR_STUB 19, 0",
    "ISR_STUB 20, 0",
    "ISR_STUB 21, 1",
    "ISR_STUB 22, 0",
    "ISR_STUB 23, 0",
    "ISR_STUB 24, 0",
    "ISR_STUB 25, 0",
    "ISR_STUB 26, 0",
    "ISR_STUB 27, 0",
    "ISR_STUB 28, 0",
    "ISR_STUB 29, 1",
    "ISR_STUB 30, 1",
    "ISR_STUB 31, 0",
    "ISR_STUB 32, 0",
    "ISR_STUB 33, 0",
    "ISR_STUB 34, 0",
    "ISR_STUB 35, 0",
    "ISR_STUB 36, 0",
    "ISR_STUB 37, 0",
    "ISR_STUB 38, 0",
    "ISR_STUB 39, 0",
    "ISR_STUB 40, 0",
    "ISR_STUB 41, 0",
    "ISR_STUB 42, 0",
    "ISR_STUB 43, 0",
    "ISR_STUB 44, 0",
    "ISR_STUB 45, 0",
    "ISR_STUB 46, 0",
    "ISR_STUB 47, 0",
    "isr_common:",
    "push rax",
    "push rbx",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    // rbx is saved above, so it can hold the frame pointer across the call.
    "mov rbx, rsp",
    "and rsp, -16",
    "mov rdi, rbx",
    "call {dispatch}",
    "mov rsp, rbx",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rbx",
    "pop rax",
    "add rsp, 16",
    "iretq",
    dispatch = sym exception_dispatch,
);

unsafe extern "C" {
    fn isr_stub_0();
    fn isr_stub_1();
    fn isr_stub_2();
    fn isr_stub_3();
    fn isr_stub_4();
    fn isr_stub_5();
    fn isr_stub_6();
    fn isr_stub_7();
    fn isr_stub_8();
    fn isr_stub_9();
    fn isr_stub_10();
    fn isr_stub_11();
    fn isr_stub_12();
    fn isr_stub_13();
    fn isr_stub_14();
    fn isr_stub_15();
    fn isr_stub_16();
    fn isr_stub_17();
    fn isr_stub_18();
    fn isr_stub_19();
    fn isr_stub_20();
    fn isr_stub_21();
    fn isr_stub_22();
    fn isr_stub_23();
    fn isr_stub_24();
    fn isr_stub_25();
    fn isr_stub_26();
    fn isr_stub_27();
    fn isr_stub_28();
    fn isr_stub_29();
    fn isr_stub_30();
    fn isr_stub_31();
    fn isr_stub_32();
    fn isr_stub_33();
    fn isr_stub_34();
    fn isr_stub_35();
    fn isr_stub_36();
    fn isr_stub_37();
    fn isr_stub_38();
    fn isr_stub_39();
    fn isr_stub_40();
    fn isr_stub_41();
    fn isr_stub_42();
    fn isr_stub_43();
    fn isr_stub_44();
    fn isr_stub_45();
    fn isr_stub_46();
    fn isr_stub_47();
}

const STUBS: [unsafe extern "C" fn(); STUB_COUNT] = [
    isr_stub_0,
    isr_stub_1,
    isr_stub_2,
    isr_stub_3,
    isr_stub_4,
    isr_stub_5,
    isr_stub_6,
    isr_stub_7,
    isr_stub_8,
    isr_stub_9,
    isr_stub_10,
    isr_stub_11,
    isr_stub_12,
    isr_stub_13,
    isr_stub_14,
    isr_stub_15,
    isr_stub_16,
    isr_stub_17,
    isr_stub_18,
    isr_stub_19,
    isr_stub_20,
    isr_stub_21,
    isr_stub_22,
    isr_stub_23,
    isr_stub_24,
    isr_stub_25,
    isr_stub_26,
    isr_stub_27,
    isr_stub_28,
    isr_stub_29,
    isr_stub_30,
    isr_stub_31,
    isr_stub_32,
    isr_stub_33,
    isr_stub_34,
    isr_stub_35,
    isr_stub_36,
    isr_stub_37,
    isr_stub_38,
    isr_stub_39,
    isr_stub_40,
    isr_stub_41,
    isr_stub_42,
    isr_stub_43,
    isr_stub_44,
    isr_stub_45,
    isr_stub_46,
    isr_stub_47,
];

/// Handler for each hardware line, as a function address. Zero means none.
static IRQ_HANDLERS: [AtomicUsize; IRQ_LINES] = [const { AtomicUsize::new(0) }; IRQ_LINES];

/// Routes hardware line `line` (0 to 15) to `handler`. The handler must
/// acknowledge the line with `pic::eoi` before it returns, or the line stays
/// blocked.
pub fn set_irq_handler(line: usize, handler: fn()) {
    if let Some(slot) = IRQ_HANDLERS.get(line) {
        slot.store(handler as usize, Ordering::Release);
    }
}

/// Builds the exception gates and loads the IDT. Call once, with interrupts
/// disabled, after the kernel GDT is loaded.
pub fn load() {
    let table = ptr::addr_of_mut!(IDT).cast::<Gate>();
    for (vector, stub) in STUBS.iter().enumerate() {
        let gate = Gate::interrupt(*stub as usize as u64, KERNEL_CODE);
        // SAFETY: `vector` is below EXCEPTION_VECTORS, which is below
        // IDT_ENTRIES, so the write stays inside `IDT`. Interrupts are disabled
        // (the entry sequence runs `cli`), so no handler reads the table yet.
        unsafe { ptr::write(table.add(vector), gate) };
    }
    let pointer = Pointer {
        limit: (core::mem::size_of::<[Gate; IDT_ENTRIES]>() - 1) as u16,
        base: table as u64,
    };
    // SAFETY: ring 0. `pointer` describes `IDT`, a static table of valid gates
    // that lives forever. `lidt` only reads the pointer.
    unsafe {
        asm!("lidt [{}]", in(reg) &pointer, options(readonly, nostack, preserves_flags));
    }
}

/// Handler for an exception taken in ring 3, as a function address. Zero means
/// none. It receives the vector, the faulting rip and the error code, and it must
/// not return: it ends the faulting thread.
static USER_FAULT: AtomicUsize = AtomicUsize::new(0);

/// Routes exceptions raised in ring 3 to `handler`. Other exceptions, and those
/// raised in ring 0, still print and stop the VM.
pub fn set_user_fault_handler(handler: fn(u64, u64, u64) -> !) {
    USER_FAULT.store(handler as usize, Ordering::Release);
}

/// Called by the common entry with the saved state. Returns to resume a
/// breakpoint. A ring-3 exception goes to the user fault handler, if one is set.
/// Every other exception stops the VM.
extern "C" fn exception_dispatch(frame: &mut TrapFrame) {
    let mut out = serial::Writer;
    match frame.vector {
        3 => {
            let _ = writeln!(out, "trap 3 (breakpoint) at rip {:#x}", frame.rip);
        }
        vector
            if (u64::from(IRQ_BASE)..u64::from(IRQ_BASE) + IRQ_LINES as u64).contains(&vector) =>
        {
            irq_dispatch((vector - u64::from(IRQ_BASE)) as usize);
        }
        vector => {
            // The low two bits of the saved CS are the privilege level it came from.
            if frame.cs & 3 == 3 {
                let handler = USER_FAULT.load(Ordering::Acquire);
                if handler != 0 {
                    // SAFETY: a non-zero entry was stored by `set_user_fault_handler`
                    // from a `fn(u64, u64, u64) -> !`, so it is a valid function
                    // address of that type.
                    let handler: fn(u64, u64, u64) -> ! =
                        unsafe { core::mem::transmute::<usize, fn(u64, u64, u64) -> !>(handler) };
                    handler(vector, frame.rip, frame.error_code);
                }
            }
            let _ = writeln!(
                out,
                "trap {} at rip {:#x}, error {:#x}",
                vector, frame.rip, frame.error_code
            );
            qemu::exit(qemu::EXIT_FAILURE);
        }
    }
}

/// Runs the handler for hardware line `line`. A line without a handler is
/// acknowledged and ignored.
fn irq_dispatch(line: usize) {
    let handler = IRQ_HANDLERS[line].load(Ordering::Acquire);
    if handler == 0 {
        pic::eoi(line as u8);
        return;
    }
    // SAFETY: a non-zero entry was stored by `set_irq_handler` from an
    // `fn()`, so it is a valid function address of that type.
    let handler: fn() = unsafe { core::mem::transmute::<usize, fn()>(handler) };
    handler();
}
