# Running programs

How Limoniter runs what you're editing on a machine: the Spectrum built in, an emulator of your own, or the real
thing. **Run on machine** (F9) runs on the machine chosen for that kind of program; **Run on** in the File menu
lists every machine that could run it.

## What's run

When you run, the document becomes a *runnable*: a machine and a file it loads.

| You're editing | It runs |
|---|---|
| A BASIC listing opened from a program | The program, tokenised as it was: NextBASIC as a `.bas`, a 48K tape listing as a `.tap`, Commodore as a `.prg` |
| A program, tape, snapshot or disk image (`.tap`, `.tzx`, `.z80`, `.sna`, `.bas`, `.nex`, `.prg`, `.t64`, `.d64`, `.crt`, `.ssd`, `.dsk`, `.trd`, `.rom`, `.cas` and others) | That file, as it is |
| A file selected in a disk image or archive | That file, taken out of it |
| Anything else | Nothing yet: Run asks which machine it's BASIC for, and runs it as a listing |

The machine is worked out from the file: a Commodore program by where it loads, a Z80 snapshot by its header, a
+3DOS file by which BASIC it holds.

## Where it runs

Every place a program can run is in one list, **File, Send to device, Devices**:

- **Built in:** a ZX Spectrum 48K and 128K, running in the preview pane, with nothing to install.
- **Emulators:** Fuse, ZEsarUX, VICE, CSpect, b2, openMSX and others, started as programs of their own. Limoniter
  finds the ones installed, and any other can be added by its path.
- **Real machines:** the devices programs are sent to. An Ultimate 64 runs a program as it arrives; a Spectrum
  Next, a card or a folder only receives it, and the program is started there.

The first time a kind of program is run, Limoniter asks where, and remembers.

## While it runs

A run is a session, shown in the status bar with a Stop button. Running again reloads into the session if the
machine can take it, resets and loads if not, and starts it afresh otherwise.

Machines that can be reached while they run (the built-in Spectrum, ZEsarUX through its remote protocol, VICE
through its binary monitor) can also:

- show their memory in hex view, as it changes, and take edits to it;
- show their screen as text in the machine's own character set;
- give back the BASIC program in memory as a listing, and take an edited one back without a reset.

## Assembler source

Source for the Z80, 6502, 6809 or 68000 (told from its highlighting language) is assembled when it's run, with
Limoniter's own assembler (`src/asm/`: one framework, one module per processor) or the one chosen for the processor
(`src/assemblers.rs`: sjasmplus, Pasmo, ACME, lwasm, vasm; `src/c64_assemblers.rs`: KickAssembler, whose debug
file gives each line's address, and 64tass, whose listing does). KickAssembler's and 64tass's source is read by
Limoniter's own assembler through `src/asm/c64_syntax.rs`, which rewrites each line into the assembler's own forms,
one line for one. Either way the result is an `asm::Assembly`: bytes at
addresses, each source line's address and bytes, and problems by file and line. `src/asm_host.rs` turns it into
something to run (a tape with a BASIC loader, a `.nex`, a C64 `.prg` with a `SYS` line, a Color Computer LOADM
`.bin`, an Atari ST `.tos` whose relocations are found by assembling the source a second time 64K further on) or a
file to save (68000 with an ORG), shows the problems, and keeps it for the debugger. 68000 source for the Amiga
(`asm::hunk::is_amiga_source`) is assembled with each SECTION 32 MB from the next, twice, the second time all moved
16 MB and 256 bytes on: a long that moved by exactly that is an address, its value says which section, and it becomes
a HUNK_RELOC32 entry; anything else that moved can't be moved by AmigaDOS and is a problem on its line. It runs on
FS-UAE or WinUAE (`src/amiga_run.rs`): the program goes in a folder made DH0: with an S/Startup-Sequence that runs
it, and the emulator is started with a configuration file made for the run (model, Kickstart or the emulator's own
AROS). Each processor's every instruction form is checked byte
for byte against its reference assembler (`src/asm/testdata`).

Debugging maps breakpoints (clicked beside line numbers, `TextBuffer::code`) to addresses through the assembly, and
the address stopped at back to a line:

- The built-in Spectrum: rustzx's debug interface (`zx_machine::Breaks`), registers read from a snapshot.
- ZEsarUX (`src/remote_debug.rs`): ZRCP in step mode, started empty so the breakpoints are set before the program
  loads; a Spectrum tape is loaded in the built-in Spectrum and handed over as a snapshot at its first instruction.
- VICE: binary monitor checkpoints, set before the program is autostarted through the monitor.
- XRoar (`src/remote_debug_more.rs`): its GDB stub, which stops the machine as it's connected to, before `-run` has
  loaded the program; `Z0` breakpoints, `Z2` write watchpoints, `s`, and step over by a breakpoint after a call.
- Hatari: its own debugger on its console (stdin and both outputs piped). A `--parse` file stops at the program's
  first instruction (`pc = TEXT && TEXT < $e00000`, as the desktop's TEXT is in ROM); breakpoints are then set at
  TEXT plus each line's address, and the stop is reported relative to TEXT.
- FS-UAE and WinUAE (`src/amiga_debug.rs`, `src/amiga_debug_host.rs`): the GDB remote serial protocol, to the GDB
  server in uae-dap's FS-UAE 4.x (`remote_debugger`, `remote_debugger_port`, `remote_debugger_trigger`) or in
  Bartman's WinUAE (`debugging_features=gdbserver`, `debugging_trigger`, always port 2345), told to the emulator in
  the configuration made for the run; an emulator has one if those words are in its program. The trigger is the
  program's name as the Startup-Sequence runs it: as it starts, the emulator stops at its first instruction and
  answers. `qOffsets` gives every hunk's address (`a;b;c`, or vAmiga's `Text=;Data=;Bss=`); a line's address in the
  assembly (section start plus offset) maps to the hunk loaded there, and back. `Z0` breakpoints, `Z2` write
  watchpoints, `vCont;s` and `vCont;c`, Ctrl+C to pause, `P` and `M` to change registers and memory; `O` packets
  (kprintf) are kept as output. Stepping a line steps instructions until one starts a line; a call out of the program
  (or any call, stepping over) runs on to a breakpoint after it, at the same stack depth. Breakpoints changed while it
  runs are set with it stopped for a moment. The custom chips are read at $DFF000, which both servers give from the
  chips' saved state. Tested against a stand-in server running a little 68000 (`amiga_debug_tests.rs`), and on both
  real emulators with AROS (`LIMONITER_AMIGA_GDB`).

- A real Next (`src/dzrp.rs`): DeZog's protocol to dezogif over a serial port (921600 baud) or a socket. dezogif
  works in DZRP's simple mode, so breakpoints are RST 0 written with CMD_SET_BREAKPOINTS before each CONTINUE and put
  back with CMD_RESTORE_MEM at each stop, and a step is a CONTINUE with breakpoints after the instruction and where
  it can jump (`z80_next`, with instruction lengths checked against every instruction the assembler knows). The
  program is written into its 8K banks and paged into slots 2 to 7, then PC and SP set. Tested against a stand-in
  (`dzrp::tests::FakeNext`), not yet on a Next.

The debugger's panel (`src/debug_panel.rs`) adds the call stack, watches, a memory window, conditions on breakpoints
and write watchpoints. The built-in Spectrum's conditions are checked as breakpoints are reached (passing straight on
when false), and its write watchpoints come from a hook added to a copy of rustzx-core (`vendor/rustzx-core`, see
its LIMONITER.md). Spectrum BASIC is debugged by a breakpoint, inside Limoniter, where the ROM starts each statement
(`src/zx_basic_debug.rs`), reading PPC and SUBPPC there, and the variables from VARS.

## Ground rules

- Only programs listed as emulators, by you or by Limoniter's search, are ever started.
- The file run is written to a temporary folder of its own; disk images are never written.
- Add-ons can act on a file before it runs, with `on_run`, as `on_send` does before a send.
