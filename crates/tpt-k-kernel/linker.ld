/* SPDX-License-Identifier: MIT OR Apache-2.0 */
/*
 * Kernel image layout. Loaded at 1 MiB; p_paddr equals p_vaddr, so the boot
 * loader can place segments at their physical addresses.
 *
 * The user code and the user stack each start and end on a page boundary, so
 * the kernel can mark exactly those pages accessible from ring 3.
 */
ENTRY(_start)

SECTIONS
{
    . = 0x100000;

    .text : { KEEP(*(.text._start)) *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }

    . = ALIGN(4096);
    __user_code_start = .;
    .user : { KEEP(*(.user .user.*)) }
    . = ALIGN(4096);
    __user_code_end = .;

    __user_stack_start = .;
    .user_stack : { *(.user_stack .user_stack.*) }
    . = ALIGN(4096);
    __user_stack_end = .;

    .data : { *(.data .data.*) }
    .bss : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    /DISCARD/ : { *(.eh_frame) *(.eh_frame_hdr) *(.note .note.*) }
}
