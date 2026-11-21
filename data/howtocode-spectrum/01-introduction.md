# 1. Introduction

[Contents](README.md) · [Working with Limoniter ›](02-working-with-limoniter.md)

How To Code: the ZX Spectrum is a guide to writing programs for the Spectrum that work: demos and games for the 48K, the 128K and its family, and the Spectrum Next, that run on every machine they claim to, and that hand the machine back as they found it.

It's by **Comrade J and Mnemonic of Share and Enjoy**, and it's made as How To Code v8 was for the Amiga: a practical reference, every fact checked against the machines' own documentation, and every example a whole program that's been run. It ships inside **Limoniter**, a text and hex editor with its own Z80 assembler and a Spectrum built in, so F9 runs any example on a 48K or 128K at once. You don't need Limoniter to use it, though: the examples are plain sjasmplus syntax.

## Who it's for

Someone who can already program, perhaps in Z80, perhaps in something else, and wants to write for the Spectrum's own hardware: the ULA and its screen, the beeper, the 128K's banks and AY chip, and the Next's Layer 2, sprites and copper. It's also for anyone coming back after thirty years, who remembers `RANDOMIZE USR 32768` but not why their old border effects only worked on a 48K.

It isn't a Z80 tutorial. Rodnay Zaks's *Programming the Z80* and the Z80's own manual (see [further reading](14-further-reading.md)) explain the instructions; this guide explains the machine around them.

## What it covers

The 48K first, because everything else is built on it, and every 48K example here runs on the later models too. Then what the 128K adds (paging, the shadow screen, the AY), what changed on Amstrad's +2A and +3, and the Pentagon, a Russian clone with different timing that many demos are written for. Then the Next: its Z80N processor's new instructions, nextreg, Layer 2, sprites, the copper, the tilemap and DMA.

## How it was checked

Every hardware fact is checked against a primary source, and the source is named where the fact is used: World of Spectrum's technical FAQ, Logan and O'Hara's ROM disassembly (as SkoolKit annotates it), Sinclair's and Amstrad's manuals, Fuse's documented timings, and the Next's own register and port lists from its FPGA core. Where sources disagree, both are given. [Writing](WRITING.md) has the list, and the rules.

Every example was assembled by Limoniter's command line, and run:

- on Limoniter's **built-in 48K and 128K**, by the test suite, which loads each from its tape, checks it reaches its main loop, and that it goes back to BASIC with `0 OK` when its time is up;
- in **Fuse** 1.6, from a snapshot, on a 48K and a 128K, with a screenshot looked at (most of the pictures in this guide are those screenshots; the rest are ZEsarUX's);
- in **ZEsarUX** 13.0, loaded from tape as a person would, on a 48K, a 128K and a +2A, and for the Next examples on its emulated Next. The sound examples' sound was recorded there and its notes measured.

What hasn't been checked, said plainly:

- **Nothing here has been run on a real Spectrum.** The emulators are good (Fuse's and ZEsarUX's timings are well tested against real machines), but the exact T-state of a border effect can differ by one between real 48Ks ([chapter 3](03-the-machine.md) says why).
- **The Next examples have been run only in ZEsarUX**, not on a real Next, nor in CSpect. One thing differs there already: ZEsarUX restarts the copper at the top of the frame, not at the screen's first line ([chapter 11](11-the-next.md)).
- **The Pentagon** is described from Fuse's timings, and the examples were run on Fuse's Pentagon only by accident ([chapter 8](08-the-128k-and-later-models.md) says what happened); they're written for the 48K and 128K.

If you find something wrong, please report it. As How To Code v7 promised in 1993: "I won't be angry if 100 people point out the same mistake. If no one does and everyone tells me later that they knew - then I will!"

## Free to use: CC0

This guide and its examples are released under **CC0 1.0** (<https://creativecommons.org/publicdomain/zero/1.0/>): given to the public domain, as far as the law allows. Copy it, change it, use the startup and the examples in your own programs, with or without a credit. (A greeting in your scroller is always welcome.)

CC0 covers this guide and its examples. The Spectrum's ROMs are Amstrad's: Amstrad allows them to be given away with emulators, keeping the copyright, which is why Fuse, ZEsarUX and Limoniter can include them. The tools the guide points to belong to their authors.

## How to read it

Read [chapter 2](02-working-with-limoniter.md) if you're new to Limoniter, then [chapter 3](03-the-machine.md), the machine everything else depends on, and [chapter 4](04-startup-and-interrupts.md), the startup every example is built on. After that, read in any order.

Every example is a whole program in `examples/`, named for its chapter (`ch05_plot.asm`). Copy them to a folder with **Tools › ZX Spectrum and Next › Copy How To Code's Spectrum examples to a folder…**, open one, and press F9. Each starts with a line that the tests change:

```asm
AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)
```

A few words used throughout:

- **T-state**: one tick of the Z80's clock, 1/3,500,000 of a second on a 48K. Every instruction takes a known number of them (`NOP` 4, `LD A,(HL)` 7, `PUSH HL` 11), and all the timing in this guide is counted in them.
- **The ULA**: the Spectrum's one custom chip, which makes the picture, reads the keyboard, drives the beeper and the border, and shares the lower 16K of RAM with the Z80.
- **Contended memory**: RAM the ULA reads the screen from. While it's drawing, the Z80 has to wait its turn to get at it ([chapter 3](03-the-machine.md)).
- **Bank**: a 16K piece of a 128K's RAM, paged in at `$C000` ([chapter 8](08-the-128k-and-later-models.md)).
- Hexadecimal is written `$4000`, as sjasmplus and most Spectrum assemblers write it.

[Contents](README.md) · [Working with Limoniter ›](02-working-with-limoniter.md)
