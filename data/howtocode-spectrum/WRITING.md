# Writing How To Code: the ZX Spectrum

How the guide is written and its examples tested, for anyone adding to it. Like the rest of the guide, this is CC0. It follows How To Code v8's own rules for the Amiga, made for the Spectrum.

## What it is

A practical guide to writing Spectrum programs that work: on every Spectrum they claim to run on, and handing the machine back as they found it. Every fact in it is checked against a primary source, and every example is run before it goes in.

## How it's written

- **Plain, friendly English** (British spelling), short sentences, as if explaining to a capable coder who hasn't met this before. Explain a term the first time it's used. No marketing words.
- **Say why**, not only what: "the loop is 49 T-states, an odd number, so it samples the bus at every point of the ULA's 8 T-state cycle" beats "use IN A,(C)".
- **Show the mistake people make**, then the fix.
- **GitHub-flavoured Markdown.** One `#` heading per chapter, numbered (`# 5. The screen`), with the Back / Contents / Next line under it and at the end; `##` for its sections. Code in fenced blocks marked `asm`. Link to other chapters by file name (`[the screen](05-the-screen.md)`).
- **Credit**: when a section rests on someone's work (a technique, a tool, a document), name them.

## Accuracy

Every hardware and system fact is checked against a primary source before it goes in, and the source is named where the fact is used:

- **World of Spectrum's technical FAQ**, the 48K and 128K reference pages above all: <https://worldofspectrum.org/faq/reference/48kreference.htm> and <https://worldofspectrum.org/faq/reference/128kreference.htm>.
- **The ROM disassembly**: Logan and O'Hara's *Complete Spectrum ROM Disassembly*, annotated online by SkoolKit at <https://skoolkid.github.io/rom/>.
- **Sinclair's and Amstrad's manuals**: the 48K BASIC manual and the +3 manual, transcribed on World of Spectrum.
- **Fuse's timings**, in libspectrum's `timings.c`: <https://sourceforge.net/p/fuse-emulator/libspectrum/ci/master/tree/timings.c>.
- **The Next's own documentation**: its FPGA core's `nextreg.txt` and `ports.txt` (<https://gitlab.com/SpectrumNext/ZX_Spectrum_Next_FPGA>), and the wiki at <https://wiki.specnext.dev>.

Where two sources disagree (by one T-state, say, as they do over when contention starts), both are given. What can be shown on an emulated machine is shown there; what can't be checked is said to be unchecked, or left out. Every web link is fetched before it goes in, to be sure it's there.

## Targets

Every 48K example must work on a **48K**, a **128K** and a **+2A or +3**. An example that needs a 128K says so at its top and uses `DEVICE ZXSPECTRUM128`; a Next example uses `DEVICE ZXSPECTRUMNEXT`.

## Examples

In `examples/`, named for their chapter: `ch05_plot.asm`.

- Each Spectrum example is a whole program on the startup: it sets `DEVICE`, includes `startup.asm`, and defines `Init`, `Frame` and `Interrupt` (see `startup.asm`, and `startup_demo.asm` for the shape). Don't change `startup.asm`: if an example needs something from it, say so.
- It starts with a comment saying what it shows, its chapter and `CC0`, then `AUTOEXIT equ 0` on a line of its own (the tests set it). Its code stays below `$BC00`, where the startup's stack begins.
- It needs no files but its own, the startup, and anything it `incbin`s from `examples/` (made here, CC0, by a script beside it).
- A Next example is whole in itself (it doesn't use the startup, which returns to BASIC), and still has its `AUTOEXIT` line, though it runs until the Next is reset.
- Comments say why, in the guide's plain English. Labels are CamelCase; local labels `.lowercase`.

## Testing an example

Limoniter assembles it from the command line, as F9 does. The output's ending says what to make:

    limoniter --assemble examples/ch05_plot.asm out/ch05_plot.tap AUTOEXIT=500
    limoniter --assemble examples/ch05_plot.asm out/ch05_plot.sna AUTOEXIT=500
    limoniter --assemble examples/ch11_next_layer2.asm out/ch11.nex

A `.tap` is the tape F9 runs (with a BASIC loader, or the one the source saves); a `.sna` is that tape loaded on the built-in Spectrum and stopped where the code starts; a `.nex` is a Next program. `AUTOEXIT=500` makes it go back to BASIC by itself after 500 frames, to show it gives the machine back.

Then it's run:

- **On Limoniter's built-in Spectrum**, by the test suite: `cargo test howtocode` assembles every example, loads each 48K one from its tape on a 48K and a 128K (and each 128K one on a 128K), checks that it reaches its main loop and, when its time is up, that BASIC says `0 OK`; checks that the sound chapter's examples are heard; and checks that the startup's `Interrupt` starts at the same time in every frame.
- **In Fuse** (1.6), from its `.sna` on a 48K, and on a 128K from the 128K snapshot made into a `.z80` that says it's a 128K (Fuse 1.6 took 128K `.sna` snapshots for a Pentagon), with a screenshot looked at: the effect must be right.
- **In ZEsarUX** (13.0), from its `.tap`, loaded as a person would (`LOAD ""`), on a 48K, a 128K and a +2A, with a screenshot; and the sound examples' sound recorded and their notes measured. Next examples run there on its emulated Next (TBBlue).

`cargo test howtocode -- --ignored write_the_examples` writes every example's tape, and snapshots of it on a 48K and a 128K, to `target/howtocode-spectrum`, to run in an emulator.

## Chapters

`NN-name.md`, numbered as in `README.md`. A chapter can have pictures from the emulator, in `images/`, as PNGs no bigger than they need to be (the screen at 2×, under 100 KB).
