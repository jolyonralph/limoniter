# 2. Working with Limoniter

[‹ Introduction](01-introduction.md) · [Contents](README.md) · [The machine ›](03-the-machine.md)

In the 1980s a Spectrum program was written on the Spectrum: an assembler such as Hisoft's GENS in the same 48K as the program, the source saved to tape, and a crash costing an afternoon. Today most Spectrum code is written on a PC or a Mac, assembled there, and run in an emulator a second later. This chapter is about doing that with Limoniter, which has a Z80 assembler and a Spectrum of its own, so there's nothing else to install.

## Starting

Copy the examples into a folder of your own with **Tools › ZX Spectrum and Next › Copy How To Code's Spectrum examples to a folder…**. The folder opens as a project, with `startup_demo.asm` open: the smallest whole program on the guide's startup ([chapter 4](04-startup-and-interrupts.md)). Or choose **File › New as › ZX Spectrum demo (Z80, How To Code)**, which starts a new file with the same program and the startup put inside it, so it needs nothing beside it.

**Help › How To Code: the ZX Spectrum** opens this guide.

## Build and run: F9

Press **F9** (**File › Run on machine**). Limoniter assembles the source, and if there's a problem, marks the line and stops there. Otherwise it makes the program into something a Spectrum can load, and runs it:

- Source for a **48K or 128K** goes on a tape (`.tap`) with a one-line BASIC loader: `CLEAR` to just below the code, `LOAD "" CODE`, and `RANDOMIZE USR` to start it. Which machine it's for comes from the source's `DEVICE` line: `DEVICE ZXSPECTRUM128` is a 128K's, anything else a 48K's.
- Source that says **`DEVICE ZXSPECTRUMNEXT`** becomes a `.nex`, the Next's own program file ([chapter 11](11-the-next.md)).
- Source that saves its own tape, snapshot or `.nex` (with sjasmplus's `SAVETAP`, `SAVESNA` or `SAVENEX`) runs that instead, as it was meant to: [chapter 13](13-releasing.md)'s example makes its own tape, with a loading screen.

The code has to start at 24000 or above for the BASIC loader to fit below it; the guide's examples start at `$8000` (32768), clear of the screen and of the memory the ULA holds up ([chapter 3](03-the-machine.md)).

The first time, F9 asks where to run it; it remembers. **File › Run on** lists everywhere that can, and **File › Run on › Stop running** stops it.

### The built-in Spectrum

Limoniter has a **48K and 128K built in** (it's rustzx, an emulator written in Rust, with Amstrad's ROMs, which Amstrad allows emulators to include). A program runs in a panel beside its source, with sound from the beeper and the 128K's AY. Click its screen to type on it (Ctrl is Symbol Shift, Esc is Break), or click the keyboard drawn under it. It's the quickest way to try something, and the only one with the profiler (below).

It emulates contended memory and the floating bus, and every example in this guide runs on it. For the last T-state of a border or multicolour effect, though, check in Fuse or ZEsarUX too: they're the emulators whose timing has been measured against real machines most.

### Emulators

**File › Run on › Devices and emulators…**, then **Find emulators**, adds the ones installed. For the Spectrum:

| Emulator | For | Where |
|---|---|---|
| **Fuse** | 48K, 128K, +2, +2A, +3, Pentagon and more; very accurate timing | <https://fuse-emulator.sourceforge.net/> |
| **ZEsarUX** | all of those and the Next; Limoniter debugs through it | <https://github.com/chernandezba/zesarux> |
| **CSpect** | the Next | <https://mdf200.itch.io/cspect> |

Fuse comes with the Spectrum ROMs (Amstrad's permission again), so it needs nothing of yours. (Some Linux distributions leave them out of their Fuse package, and put them in a separate one.)

## Limoniter's Z80 assembler

Limoniter's own Z80 assembler takes the syntax of **sjasmplus**, the assembler most Spectrum and Next coders use today: `org`, `db`/`dw`/`ds`, `equ`, `include`, `incbin`, `align`, `rept`/`endr`, `macro`, `if`/`ifdef`/`ifndef`, local labels (`.loop`, belonging to the label before), and sjasmplus's devices: `DEVICE ZXSPECTRUM48` (or `128`, or `NEXT`) gives it the machine's memory, and `SAVETAP`, `SAVESNA` and `SAVENEX` write files from it. It knows the Z80's undocumented instructions (the halves of IX and IY, `SLL`, `OUT (C),0`) and **the Next's Z80N instructions**: `NEXTREG`, `MUL D,E`, `LDIRX`, `PIXELAD`, `OUTINB` and the rest ([chapter 11](11-the-next.md)).

If you have sjasmplus itself, Limoniter can use it instead (in **Devices and emulators**, or the assembler's own settings), and a project made for sjasmplus builds as sjasmplus builds it.

While you type:

- **The status bar** shows the line's address, its bytes, and **how many T-states** its instruction takes. Counting T-states is how everything in [chapter 5](05-the-screen.md)'s timed effects is done, and this is the quickest way.
- **The memory map**: after a build, a bar over the Output panel shows where the program sits in the Spectrum's memory, among the ROM, the screen and BASIC's system variables, and warns if code lands somewhere it can't go.
- **Tools › ZX Spectrum and Next › Screen designer** draws a Spectrum screen and saves it as a `.scr`; **Tools › General › Character and sprite editor** draws UDGs, fonts and the Next's sprites and puts them in your source as `db` lines; **Tools › General › Sound effects** makes AY and beeper effects.

### From the command line

`limoniter --assemble SOURCE OUTPUT` assembles as F9 does, without opening a window. The ending of OUTPUT says what to make: `.tap` (the tape F9 runs), `.sna` (that tape loaded on the built-in Spectrum and stopped at the program's first instruction: a snapshot any emulator loads), or `.nex`. `NAME=VALUE` after it sets a value the source gives with `NAME equ`:

    limoniter --assemble ch05_plot.asm ch05_plot.tap AUTOEXIT=500

That's how this guide's examples are built for testing ([Writing](WRITING.md)).

## Debugging

Click beside a line's number to put a **breakpoint** there, and press F9. When the program reaches it, it stops, the line is marked, and the registers and flags are shown beside the source. **Step** runs one instruction (into a `CALL`), **Step over** runs a whole `CALL`, `DJNZ` or `LDIR`, and **Carry on** runs to the next breakpoint. While it's stopped you can see the **call stack** (from the return addresses on the stack), **watches** (any expression: `HL`, `[Frames]`, `word[$5C78]`), and **memory**, with what's changed since the last stop picked out.

A breakpoint can stop **only when** something's true (`B=2 && [count]>3`), and **Stop when written to** stops after any instruction that writes to an address or a range: the way to catch whatever is overwriting your table.

This works on the built-in Spectrum and in **ZEsarUX**, which Limoniter drives through its remote protocol (so it can debug Next programs too). A Spectrum program for ZEsarUX is loaded from its tape in Limoniter first and handed over as a snapshot, so ZEsarUX can stop even at its first instruction.

## The profiler

On the built-in Spectrum, **Profile** counts the T-states spent at every address while the program runs, contended memory and interrupts included. Click it, let the program run through the part you care about, and click it again: you get the time in each routine (each label and what follows it), its share, and how often it was entered, and the busiest source lines. Click one to go there.

That's the first thing to do when a frame takes too long: measure, then optimise the routine that's actually slow ([chapter 10](10-speed.md)).

## Tapes, snapshots and .nex files

Limoniter opens all the Spectrum's usual files, to look at and to run:

- **`.tap`**: a tape's blocks one after another, each with a two-byte length. The ROM's own format, nothing more ([chapter 13](13-releasing.md)).
- **`.tzx`**: a tape as it really was, with turbo loaders and copy protection: every block's timing kept. The built-in Spectrum loads its standard blocks.
- **`.sna`** and **`.z80`**: snapshots, the whole machine's memory and registers at a moment. A `.sna` is the simplest (27 bytes of registers, then the RAM); `.z80` is compressed, and its later versions hold 128K machines.
- **`.nex`**: the Next's program file ([chapter 11](11-the-next.md)).

A tape's BASIC is listed as text, its pictures shown, and its code disassembled with **Tools › General › Disassemble…**, into source that assembles back to the same bytes.

## Sending to a real machine

**File › Run on › Devices and emulators…** also sets up real machines: a **Spectrum Next** over Wi-Fi (with NextSync's `.sync` on the Next) or a debug cable, and cards for a divMMC or an SD-based interface. A real 48K or 128K needs a way to load: a tape played from the computer's sound output (save the `.tap` as audio with a tool such as `tape2wav` from Fuse's utilities), or a divMMC.

[‹ Introduction](01-introduction.md) · [Contents](README.md) · [The machine ›](03-the-machine.md)
