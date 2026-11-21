# 2. Working with Limoniter

[‹ Introduction](01-introduction.md) · [Contents](README.md) · [General guidelines ›](03-general-guidelines.md)

In 1993, v7's chapter on assemblers told you to avoid K-Seka, explained why Devpac 2's optimiser made broken code, and how to drive ArgAsm from CygnusEd with ARexx macros. Most of those tools are long gone, and so is the way of working: the source in RAM on the Amiga itself, one crash away from being lost.

Today most Amiga demos are written on a PC or a Mac. The source is assembled there, and the program runs in an emulated Amiga a second later. This chapter is about doing that with Limoniter, the editor v8 ships in. Every example in v8 is plain Devpac/vasm syntax, so you can just as well use vasm and your own editor, and the chapter says how.

On the Amiga itself, Devpac and ASM-One (and its descendant ASM-Pro) are still used and still fine. If you write on a real Amiga, v8's examples assemble there too, as long as your assembler takes Devpac syntax and the `include` files sit beside the source.

## What you need

### An emulator

Limoniter runs Amiga programs in **FS-UAE** (Linux, macOS and Windows, <https://fs-uae.net/>) or **WinUAE** (Windows, <https://www.winuae.net/>). Open **File › Run on › Devices and emulators…**: **Find emulators** adds the ones already installed, and **Get an emulator…** fetches one the simplest way your computer has. On Windows, Limoniter downloads FS-UAE's latest release from its GitHub page; on Linux it installs FS-UAE with apt, and on a Mac with Homebrew, in the Terminal panel where you can watch. WinUAE it can't fetch: get it from its own site.

For debugging at source level ([chapter 16](16-debugging-and-testing.md)) you need an emulator with a GDB server built in: the FS-UAE that comes with uae-dap, or WinUAE with Bartman's GDB server. They run programs just like the ordinary ones, so you may as well start with one of those.

### Kickstart

An Amiga needs its Kickstart ROM, and Kickstart is still copyrighted, so neither Limoniter nor v8 can include it. Either buy **Amiga Forever** (<https://www.amigaforever.com/>), which has every Kickstart, or copy the ROM from your own Amiga.

In **Devices and emulators**, under the Amiga emulator, choose the **Amiga** to run on:

| Model | What it is |
|---|---|
| A500 (OCS, 1 MB, Kickstart 1.3) | the machine every v8 example must work on: a 68000, 512K of chip RAM and 512K of slow RAM |
| A500+ (ECS, 1 MB chip, Kickstart 2.04) | the ECS chipset and Kickstart 2 |
| A1200 (AGA, 68020, 2 MB, Kickstart 3.1) | AGA, a 68020, and no fast RAM |

and a **Kickstart** for each: **Choose…** one, or **Find** them where Amiga Forever (its encrypted ROMs too, with its `rom.key`), FS-UAE and WinUAE keep them. Limoniter knows the usual ROMs by their checksums and picks the best for each model; it tells you if the one you chose isn't meant for that model. To try an A500 with Kickstart 3.1, choose a 3.1 ROM for the A500 (Limoniter knows the A500/A600/A2000 3.1 ROM, 40.63).

With no Kickstart at all, the emulator uses AROS, a free replacement for Kickstart. AROS runs well-behaved system programs, but not every demo, and it isn't what anyone's Amiga runs: test on real Kickstarts.

### The NDK (optional)

The **NDK 3.2**, Hyperion's Native Developer Kit for AmigaOS 3.2, has the assembler includes and the Autodocs for every library (see [chapter 3](03-general-guidelines.md)). It's free to download, but it isn't Limoniter's to hand out, so Limoniter fetches it only when you ask: **File › Run on › Amiga NDK (code hints)…** (or **Tools › Amiga › NDK (code hints)…**), then **Download from Aminet**. It's about 8 MB, and it's unpacked beside Limoniter's settings. **Use a folder…** points Limoniter at an NDK you already have.

With the NDK, Limoniter explains system functions as you write (below), and its assembler finds the NDK's headers: `include "exec/types.i"` works, and `jsr _LVOAllocMem(a6)` assembles with no include at all (a definition of your own wins). v8's examples don't need it: everything they use from the system is in `system.i`.

## Starting a project

The quickest start is v8's own: copy [`examples/startup.i`](examples/startup.i), `hardware.i`, `system.i` and `startup_demo.s` into a folder of your own, rename `startup_demo.s`, and open the folder with **File › Open folder…**. [Chapter 4](04-startup-and-exit.md) explains the startup; your demo is four things (`Init`, `Frame`, `VBlank` and `Copper`) and the startup does the rest, either taking the machine or keeping the system running, as `OS_FRIENDLY` says.

Limoniter has starters of its own too: **File › New as › Amiga demo (68000)**, a complete program in one file (it takes the machine, shows a bitplane with a raster bar bouncing over it, and gives everything back when the left mouse button is pressed), and **Amiga copper only (68000)**, a smaller one with a copper gradient. They're good for trying something out. For a demo you mean to release, start from v8's startup: it also handles being started from an icon, finds the VBR, and can keep the system running.

## Build and run: F9

Press **F9** (**File › Run on machine**). Limoniter assembles the source, and if there's a problem, marks the line and stops there. Otherwise it makes an AmigaDOS program of it, puts the program in a new folder that becomes the Amiga's hard drive (`DH0:`), writes an `S/Startup-Sequence` that runs it, and starts the emulator with a configuration made for that run. The Amiga boots straight into your program. Your program can write files back to `DH0:`, which is handy for dumping a table or a picture to look at.

The first time, F9 asks which emulator to run on; it remembers. **File › Run on › Stop running** closes the emulator. **File › Run on › Assemble and save** keeps the program, saved beside the source.

Limoniter decides that 68000 source is for the Amiga when it has a `SECTION`, or uses the custom chips at `$DFF000`, or calls the system's libraries. An `.adf` disk image in front goes into the emulated floppy drive instead, so F9 boots a disk you've made (see [releasing](18-releasing.md)).

## Limoniter's assembler

Limoniter has its own 68000 assembler, so you need nothing else installed. It takes Motorola syntax as Devpac and vasm write it: `dc.b`/`dc.w`/`dc.l`, `ds`, `dcb`, `even`, `cnop`, `include`, `incbin`, `macro`, `rept`, `ifd`/`ifnd`, `ifeq`/`ifne` and the rest of the conditionals, and `section` with `code`, `data` and `bss`, with `_c` or `_f` for chip or fast memory.

Each section becomes a **hunk** of the AmigaDOS program, as vasm makes one with `-Fhunkexe -kick1hunks` and its optimiser off: only the hunk types Kickstart 1.3 knows, so the program loads on every Amiga. Every v8 example comes out with the same code, data and relocations from both (checked against vasm 2.0f); only the order of the labels in the symbol hunk, and sometimes of the relocations, differs, which changes nothing once the program is loaded. An address one section keeps of another becomes a relocation for AmigaDOS's loader to fix up; the labels go in a symbol hunk, for debuggers. A line the loader can't fix up (a word holding an address, or an `org`) is marked.

Two things to know:

- **Nothing is changed behind your back.** As with vasm's `-no-opt`, what you write is what you get: a `move.l #0,d0` stays six bytes, and a branch without a size is a word branch. Write `moveq` and `bra.s` where you mean them. That's how Devpac's own optimiser bit v7's readers, so it's no great loss.
- **It's a 68000 assembler.** It doesn't know the 68020's and later processors' own instructions (`movec`, `extb.l`, bit fields...). Where a program needs one only on a later processor, write it as words, as the startup writes `movec vbr,d0`; for code written for the 68020 and up, use vasm.

It also works while you type: the source is assembled quietly a moment after you stop, so a mistake is underlined before you press F9.

And it goes the other way: **Tools › General › Disassemble…** turns an Amiga program back into source, a section for each hunk, every relocated address as a label, and custom chip registers and library calls named beside the code. The source assembles back to the same bytes.

## Using vasm instead

vasm, by Volker Barthelmann and Frank Wille (<http://www.compilers.de/vasm.html>), is what most Amiga coders cross-assemble with today, with Frank Wille's linker vlink (<http://www.compilers.de/vlink.html>) for linking objects. To have F9 use it, choose it in **File › Run on › Assemblers…**. Limoniter then runs `vasmm68k_mot` with `-Fhunkexe -kick1hunks -no-opt` (and `-I` for the NDK's includes, if you have it), and runs what vasm makes. The same from a shell:

```
vasmm68k_mot -Fhunkexe -kick1hunks -no-opt -o demo startup_demo.s
```

The one thing you give up is source-level debugging in Limoniter, which needs Limoniter's own assembler to know which line made which bytes.

## Building from the command line

Limoniter assembles from a shell or a Makefile too, as F9 does:

```
limoniter --assemble SOURCE OUTPUT [NAME=VALUE...]
```

It writes the AmigaDOS program to OUTPUT and says how big it is:

```
$ limoniter --assemble startup_demo.s demo
demo: 2764 bytes
```

Each `NAME=VALUE` is a setting. A line of the source that starts `NAME equ` (at the start of the line, as a setting is written) gets that value instead; if there's no such line, one is added at the top. So one source builds every way it can be built, without editing it:

```
limoniter --assemble startup_demo.s demo-takeover OS_FRIENDLY=0 AUTOEXIT=500
limoniter --assemble startup_demo.s demo-os OS_FRIENDLY=1 AUTOEXIT=500
```

That's exactly how every v8 example is built for testing ([chapter 16](16-debugging-and-testing.md)). Problems are printed as `file:line: message`, one a line, and the exit code says what happened: 0 for a program written, 1 for problems in the source (or a file that couldn't be read or written), 2 for a command line it didn't understand.

`--assemble` always uses Limoniter's own assembler, whatever F9 is set to use, and the NDK if you've set it up.

## Help while you write

**The custom chips.** Rest the pointer on a register in 68000 source, written any of the usual ways (`$dff180`, `COLOR00`, `bplcon0(a6)`, `$180(a5)` once `a5` holds `$dff000`, `custom+dmacon`, `$bfe001`), and Limoniter shows its name, its address, whether it's read or written or a strobe, which chipsets have it, and what each bit does. Where a value is written to it, the value is read into its bits: `move.w #$5200,BPLCON0` says "5 bitplanes, lowres, colour on". `btst #6,$bfe001` says which bit that is (the left mouse button).

**Copper lists.** Over a copper list, written as `dc.w` pairs or with `CMOVE`, `CWAIT` and `CEND`, each instruction is decoded: "MOVE $0FFF to COLOR00", "WAIT for line $2C, x $06", "END". AGA colours written through BPLCON3's banks are followed, and so is the `$FFDF` wait that gets a copper list past line 255 ([chapter 6](06-copper.md)). An Amiga colour, `$0RGB` on a line with a colour register, is underlined in its own colour; put the cursor in it for a swatch that opens a colour picker.

**System functions**, with the NDK: rest the pointer on `_LVOAllocMem` for its prototype, library, offset, the registers its arguments go in, and its Autodoc. Typing `_LVO` offers them all. Structure offsets and flags too: `LN_NAME` is "offset 10 in LN (APTR, 4 bytes)", and `MEMF_CHIP` shows its value and bit.

**Tools › Amiga › Hardware registers and copper lists** is the same knowledge as a guide: every custom chip register from `$DFF000` to `$DFF1FE` and both CIAs', grouped by what they're for, searchable, narrowed to one chipset, with any value you type read into its bits. Its **Copper list** page shows the copper list the cursor is in, line by line, beside a picture of what it does to a PAL frame, and puts MOVE, WAIT and END lines in for you. Its **Gradient** page makes a copper gradient from colour stops, 12-bit or AGA 24-bit, and writes it into your source as `dc.w` lines.

**Tools › Amiga › Blitter minterms** works out BLTCON0 and BLTCON1: type the function as `A&B | ~A&C` or the classic `AB + aC`, or pick a common one (copy, cookie-cut, clear, invert, XOR, OR), and set the shifts, channels, descending and fill. It also takes existing BLTCON values apart, and warns when the function reads a channel that's switched off. See [the blitter](07-blitter.md).

## Graphics: the bitplane, sprite and bob converter

**Tools › Amiga › Bitplane, sprite and bob converter** turns a picture (PNG, JPEG, IFF, or any older machine's picture Limoniter reads) into what an Amiga program shows. Choose the resolution, 1 to 8 bitplanes, extra half-brite, HAM6 or HAM8, and the palette (found from the picture, 12-bit for OCS and ECS or 24-bit for AGA, or taken from a palette file), and dithering if you want it. The result is shown as the Amiga would show it, pixels their true shape, while you change the settings.

Out come the bitplanes (one after another, or interleaved), the palette (as words, as copper moves, or AGA's 24-bit colours in BPLCON3 banks), an IFF ILBM, and assembler source with the sizes, the modulos, a copper list and `incbin`s of the planes. Hardware sprites (16 pixels wide, or 32 or 64 on AGA, attached or not) come with their control words and their colours as copper moves; bobs come with a mask plane and a spare word a row for shifting, and BLTSIZE worked out.

It works the other way too: raw bitplanes from a file (ripped from a program, say) are shown as a picture, once you've told it the width, height, number of planes and layout.

## Tables: the table generator

**Tools › General › Table generator (sine, ramps…)** makes the tables demos live on: sine and cosine (amplitude, offset, phase, how many waves), straight ramps, easing curves, multiplication and division tables, perspective (scale ÷ z) and random numbers from a seed. Choose signed or unsigned bytes, words or longs, in fixed point if you want, see the table as a graph, and put it in at the cursor as `dc.b`, `dc.w` or `dc.l` lines, in hex or decimal, so many a line. Or save it as a binary file to `incbin`. Values are rounded to the nearest (halves away from zero), and any that don't fit are held at the end of the range and counted, so a table never wraps round by surprise.

## The Ripper

**Tools › General › Ripper: music and graphics in any file** looks through any file (a program, a memory dump, a disk image, and every file inside a disk image or archive) for music and graphics. It finds ProTracker modules and their relatives by their marks, and the older 15-sample Soundtracker modules by their instruments' disk names; each one found can be opened, played and saved. For graphics, it shows the bytes as Amiga bitplanes (or other machines' layouts) at any offset and width, and saves what you see as a PNG, or copies it as source.

Use it to look inside your own work (did the module really end up in the program?), and to study how something was done. What you find belongs to whoever made it: ask before you use it.

## Crunching, disks and real Amigas

- **Crunching**: **File › Run on › Amiga crunchers and music…** runs Shrinkler or Cranker on a program, and can crunch a copy every time F9 builds. When and how, in [releasing](18-releasing.md).
- **Music**: the same window converts a ProTracker module for LightSpeedPlayer, and writes the calls to its replay routine into your source ([chapter 12](12-sound-and-music.md)).
- **Trackmo disks**: **Tools › Amiga › Boot disk with no file system (trackmo)** builds a disk with no AmigaDOS on it, the way many demos and games are made; see [releasing](18-releasing.md).
- **A real Amiga**: **File › Send to device** copies a file to a Gotek's USB stick, a card, a shared folder, or an FTP server (an Amiga running one, say). See [chapter 16](16-debugging-and-testing.md).

## What changed from v7

- v7's tools (K-Seka, ArgAsm, CygnusEd and its ARexx macros) are gone. Devpac and ASM-One are still used on real Amigas; on other computers, vasm and vlink, or Limoniter's own assembler.
- v7's Devpac advice (`opt o3-`, so the optimiser didn't turn relocatable addresses into short ones) isn't needed with Limoniter's assembler or with vasm's `-no-opt`, which don't optimise at all. If you turn an optimiser on, test what it makes.
- v7 assumed you'd write and run on the same Amiga. Now the emulator is a second away, so test early and on every machine: that's [chapter 16](16-debugging-and-testing.md).

[‹ Introduction](01-introduction.md) · [Contents](README.md) · [General guidelines ›](03-general-guidelines.md)
