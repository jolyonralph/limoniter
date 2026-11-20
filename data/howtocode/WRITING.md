# Writing How To Code v8

How the guide is written and its examples tested, for anyone adding to it. Like the rest of v8, this is CC0.

## What v8 is

A practical guide to writing Amiga demos that work: on every Amiga they claim to run on, and leaving the machine as they found it. It grew from How To Code v7 (1993), and keeps v7's principles where they still hold, but every fact in it is checked again, against the manuals and on (emulated) machines, and it uses today's tools: Limoniter, vasm, FS-UAE and WinUAE, Shrinkler, LightSpeedPlayer.

## How it's written

- **Plain, friendly English** (British spelling), short sentences, as if explaining to a capable coder who hasn't met this before. Explain a term the first time it's used. No marketing words.
- **Say why**, not only what: "acknowledge the interrupt twice, because on a fast processor the first write may not have reached the chips by the RTE" beats "write INTREQ twice".
- **Show the mistake people make**, then the fix: that was v7's strength. Where v7 said something still right, keep its point (in new words) and say it came from v7 if that's interesting; where v7 was wrong or is out of date, say so plainly.
- **GitHub-flavoured Markdown.** One `#` heading per chapter, `##` for its sections. Code in fenced blocks marked `asm`. Tables where they help. Link to other chapters by file name (`[the blitter](07-blitter.md)`).
- **Credit**: when a section rests on someone's work (a technique, a tool, a document), name them.

## Accuracy

Every hardware and system fact is checked against a primary source before it goes in:

- The **Amiga Hardware Reference Manual** (3rd edition), online at <http://amigadev.elowar.com/read/ADCD_2.1/Hardware_Manual_guide/node0000.html>.
- The **NDK 3.2**: its includes and Autodocs (`Include_I/` and `Autodocs/` in the NDK, which Limoniter's Tools, Amiga, NDK can fetch).
- The **ROM Kernel Manuals**, online on the same site as the HRM.

What can be shown on an emulated machine is shown there, in the tests below. What can't be checked (a claim about real hardware the emulator doesn't model, say) is said to be unchecked, or left out. Every web link is fetched before it goes in, to be sure it's there.

## Targets

Every example must work on an **Amiga 500**: 68000, OCS/ECS, Kickstart 1.3 and Kickstart 3.1, both with the common 512K of slow (trapdoor) RAM and without it, on a plain 512K machine. FS-UAE's A500 has the slow RAM unless told otherwise (`SLOW=0` for the test script), so the plain machine has to be asked for. It must also be run on a 68020 and a 68040 (with fast RAM), where demos written on an A500 most often fail. AGA examples need an AGA machine, and say so at their top.

## Examples

In `examples/`, named for their chapter: `ch06_copper_bars.s`, `ch07_bob.s`.

- Each is a whole demo on the v8 startup: it includes `hardware.i`, `system.i` and `startup.i`, and defines `Init`, `Frame`, `VBlank` and `Copper` (see `startup.i`, and `startup_demo.s` for the shape). Don't change those three files: if an example needs something from them, say so.
- It starts with a comment saying what it shows, its chapter, `CC0`, and a `$VER:` line, then `OS_FRIENDLY     equ     0` on a line of its own (the tests set it). It must work both ways unless it can't (loading files needs the system running); then its first comment says which, and why.
- It needs no files but its own, the three `.i` files, and anything it `incbin`s from `examples/` (made here, CC0: no one else's music or graphics).
- Comments say why, in the guide's plain English. Labels are CamelCase; local labels `.lowercase`.
- Calls from the program to the startup (and back) are `jsr`, as they're in different sections.

## Testing an example

Limoniter assembles it from the command line, as F9 does:

    limoniter --assemble examples/ch06_copper_bars.s out/ch06-takeover OS_FRIENDLY=0 AUTOEXIT=500
    limoniter --assemble examples/ch06_copper_bars.s out/ch06-os OS_FRIENDLY=1 AUTOEXIT=500

`AUTOEXIT=500` makes it end by itself after 500 frames (10 seconds on PAL), to show it gives the machine back. Then it's run in FS-UAE: a small script (v8's own was written for Linux, with Xvfb so the emulator needs no screen) boots an emulated A500 from a folder with the program in its startup-sequence, followed by a small program that prints "How To Code v8: back in AmigaDOS, return code OK", and captures the screen at the times given. Look at every picture: the effect must be right while it runs, and the CLI with that line must be there after.

Run each example on Kickstart 1.3 and 3.1, both ways, with and without slow RAM, and on a 68020 and a 68040.

The test suite (`cargo test howtocode`) assembles every example both ways, so an example that stops assembling is caught.

## Chapters

`NN-name.md`, numbered as in `README.md`. A chapter can have pictures from the emulator, in `images/`, as PNGs no bigger than they need to be (a 320-wide screen at 2×, under 100 KB).
