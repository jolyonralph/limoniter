rustzx-core 0.16.0 from crates.io (https://github.com/rustzx/rustzx), MIT licence (LICENSE.md).

Changes, for Limoniter's debugger: `DebugInterface::check_memory_write`, called from
`ZXController::write_internal`, which can stop emulation after an instruction writes to memory.

And `Emulator::z80_registers`, reading the registers without saving a snapshot, for breakpoint conditions.

And `DebugInterface::instruction_done`, called after each instruction with the frame's clock, for the profiler.
