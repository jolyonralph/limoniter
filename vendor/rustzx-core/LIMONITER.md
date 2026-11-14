rustzx-core 0.16.0 from crates.io (https://github.com/rustzx/rustzx), MIT licence (LICENSE.md).

Changes, for Limoniter's debugger: `DebugInterface::check_memory_write`, called from
`ZXController::write_internal`, which can stop emulation after an instruction writes to memory.

And `Emulator::z80_registers`, reading the registers without saving a snapshot, for breakpoint conditions.

And `DebugInterface::instruction_done`, called after each instruction with the frame's clock, for the profiler.

- `emulator/fastload/tap.rs`: the fast tape load always checks a block's first byte as its flag, rather than only
  when the Z flag is clear, as Fuse's trap does: the Sinclair ROM always enters LD-BYTES with Z clear, but OpticOS
  (a replacement ROM Limoniter runs) doesn't, and its LOAD then never matched a block.
