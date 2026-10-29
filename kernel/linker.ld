/* The kernel lives in the top 2 GiB of the address space, where Limine
 * maps it. Limine finds our requests between the start and end markers;
 * each part gets its own segment, aligned to Saiph's 16 KiB pages, so it
 * can get its own permissions (read-only, executable, writable). */
OUTPUT_FORMAT(elf64-littleaarch64)
ENTRY(_start)

PHDRS
{
    limine_requests PT_LOAD;
    text PT_LOAD;
    rodata PT_LOAD;
    data PT_LOAD;
}

SECTIONS
{
    . = 0xffffffff80000000;
    __kernel_start = .;

    .limine_requests : {
        KEEP(*(.requests_start_marker))
        KEEP(*(.requests))
        KEEP(*(.requests_end_marker))
    } :limine_requests

    . = ALIGN(16384);
    .text : {
        __text_start = .;
        *(.text .text.*)
        __text_end = .;
    } :text

    . = ALIGN(16384);
    .rodata : {
        __rodata_start = .;
        *(.rodata .rodata.*)
        __rodata_end = .;
    } :rodata

    . = ALIGN(16384);
    .data : {
        __data_start = .;
        *(.data .data.*)
    } :data
    /* Per-core variables (cpu_local!, see src/percpu.rs). */
    .cpu_local : ALIGN(64) {
        __cpu_local_start = .;
        KEEP(*(.cpu_local))
        __cpu_local_end = .;
    } :data
    .bss : { *(.bss .bss.*) *(COMMON) } :data
    __kernel_end = .;

    /DISCARD/ : { *(.eh_frame*) *(.note .note.*) }
}
