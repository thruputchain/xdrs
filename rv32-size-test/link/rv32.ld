/* RV32 linker script for the xdrs code-size probe.
 *
 * Derived from the thruput-sdk-rs rv32 runtime's linker script, keeping the
 * layout that script produces so the probe's section sizes match what a
 * contract build would see. The contract-specific tables (dispatch,
 * interface and spec sections) are omitted: nothing here emits them.
 *
 * The target VM is modified-Harvard: PC indexes an immutable CODE memory;
 * loads and stores address a separate DATA memory. The two are INDEPENDENT
 * address spaces in the VM, so we lay them out as two MEMORY regions whose
 * virtual addresses deliberately overlap numerically — they can never collide
 * because a value used as a jump target is fetched from CODE while a value
 * used by a load/store hits DATA. This is what lets DATA start near address 0
 * instead of being stacked above .text: the stored initial-memory image is
 * then compact (just the real .rodata/.data), with none of the code-sized
 * leading-zero "shadow" a single-region layout would produce.
 *
 * Because CODE and DATA VMAs overlap, the linker must be invoked with
 * `--no-check-sections` (set in .cargo/config.toml); the overlap is
 * intentional, not a layout bug.
 *
 * DATA is laid out stack-first, so a stack overflow runs off the bottom of
 * memory and traps instead of silently corrupting .data:
 *
 *   0x0 .. __stack_top    stack, growing DOWN from __stack_top
 *   __stack_top ..        .rodata / .data / .bss
 *   __heap_base ..        heap, growing UP toward __memory_size
 *
 * With the stack at the bottom, the regions that grow (stack down, heap up)
 * both grow toward a hard boundary instead of toward each other: a stack
 * overflow wraps sp below zero to ~0xFFFFFFxx, far outside the guest
 * memory the VM provisions — an immediate OOB trap — rather than silently
 * corrupting heap or statics. The heap gets a hard bound at the true guest
 * memory size (__heap_end = __memory_size). Note that __memory_size is
 * intentionally smaller than the DATA region: the region lengths are relaxed
 * sanity bounds, not the real limits.
 *
 * Layout invariants the VM's lowerer enforces, which this script preserves:
 *
 *   - .text starts at virtual address 0 (so PC space and VA space match).
 *   - No data section may touch the lowerer's null guard [0, 0x8) — the
 *     stack reserve covers those addresses, so sections start well above.
 *   - No executable sections other than .text.
 *   - No GOT / PLT / dynamic relocations.
 *   - No init_array / eh_frame.
 *
 * The stack reserve starts at address 0. In principle a call chain that
 * fills the stack to the exact byte would place a local at address 0
 * (whose reference would be null); we accept that edge — such a frame is
 * one push away from the wrap trap anyway.
 */

ENTRY(_start)

/* Stack reserve size. Override with a linker --defsym in the rustflags
 * (must be a multiple of 16, enforced by the ASSERT at the bottom), e.g. in
 * .cargo/config.toml under [target.riscv32im-unknown-none-elf]:
 *
 *   "-C", "link-arg=--defsym=__stack_size=0x8000",
 *
 * PROVIDE yields to such an external definition.
 */
PROVIDE(__stack_size = 30K);

/* Guest data-memory size: how much the VM provisions. 64K is the protocol
 * default. Override the same way as __stack_size:
 *
 *   "-C", "link-arg=--defsym=__memory_size=0x20000",
 *
 * PROVIDE yields to such an external definition.
 */
PROVIDE(__memory_size = 64K);

/* Relaxed sanity bounds only — deliberately never-binding. The real
 * code/memory limits are enforced where an artifact is deployed, not here.
 * Keep these <= 1M so all direct `jal` calls stay in range and relocation
 * errors remain impossible. */
MEMORY
{
  CODE (rx) : ORIGIN = 0, LENGTH = 1M
  DATA (rw) : ORIGIN = 0, LENGTH = 1M
}

/* Stack reserve at the bottom of DATA. _start loads sp from __stack_top
 * (16-byte aligned) and the stack grows down toward 0. No output section
 * is emitted for the reserve — data sections are simply placed above it.
 */
__stack_bottom = ORIGIN(DATA);
__stack_top = __stack_bottom + __stack_size;

/* gp-relaxation anchor. With --relax-gp (set in the rustflags), lld
 * rewrites the lui+addi address materialization of any symbol whose final
 * address lands in [gp - 0x800, gp + 0x7FF] into a single gp-relative
 * instruction. Statics start exactly at __stack_top (stack-first layout),
 * so anchoring 0x800 past it covers the first 4 KiB of .rodata/.data/.bss.
 * Purely opportunistic: out-of-window accesses keep the two-instruction
 * form; correctness is never involved. _start loads gp from this symbol.
 * PROVIDE yields to a --defsym override, and the expression tracks any
 * __stack_size override automatically. */
PROVIDE(__global_pointer$ = __stack_top + 0x800);

SECTIONS
{
  .text 0 : ALIGN(4)
  {
    /* _start is the linker entry: the VM always enters here at PC 0. KEEP
     * it so dead-code elimination can't drop it.
     */
    KEEP(*(.text._start))

    *(.text .text.*)
  } > CODE

  .rodata __stack_top : ALIGN(8)
  {
    *(.srodata .srodata.*)
    *(.rodata .rodata.*)
  } > DATA

  . = ALIGN(8);
  .data : ALIGN(8)
  {
    *(.sdata .sdata.*)
    *(.data .data.*)
  } > DATA

  . = ALIGN(8);
  .bss (NOLOAD) : ALIGN(8)
  {
    __bss_start = .;
    *(.sbss .sbss.*)
    *(.bss .bss.*)
    *(COMMON)
    __bss_end = .;
  } > DATA

  /* Heap grows from __heap_base toward __memory_size, the true guest
   * memory size. The allocator checks __heap_end as the bound; with the
   * stack parked at the bottom of DATA nothing lives above the heap, so
   * the bound is the end of guest memory. Deliberately NOT derived from
   * LENGTH(DATA): the DATA region is a relaxed sanity bound, and a heap
   * bound above the real memory size would turn heap exhaustion from a
   * clean alloc failure into a VM OOB trap.
   */
  . = ALIGN(8);
  __heap_base = .;
  __heap_end = __memory_size;

  /DISCARD/ :
  {
    *(.got .got.*)
    *(.got.plt .got.plt.*)
    *(.plt .plt.*)
    *(.data.rel.ro .data.rel.ro.*)
    *(.tdata .tdata.*)
    *(.tbss .tbss.*)
    *(.init_array .init_array.*)
    *(.eh_frame .eh_frame.*)
    *(.debug_* .debug*)
    *(.comment)
  }
}

ASSERT(__heap_base <= __heap_end, "rv32 data sections leave no room for heap");
/* _start loads sp from __stack_top with no rounding; the RISC-V ABI
 * requires sp to be 16-byte aligned. */
ASSERT(__stack_top % 16 == 0, "__stack_top must be 16-byte aligned");
/* Fail a bogus __stack_size override here with a clear message. */
ASSERT(__stack_size >= 16, "__stack_size must be at least one 16-byte slot");
