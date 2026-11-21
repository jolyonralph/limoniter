# How To Code v8

*How to write Amiga demos that work: the Amiga demo coder's reference, rewritten for today.*

Based on How To Code v7 (July 1993), edited by Comrade J / Share and Enjoy, with Carl-Henrik Skårstedt (Asterix / Movement), and the comments and contributions of the many people credited in the introduction. Version 8 is released under **CC0**: use any of it, for anything.

> All nineteen chapters are written and checked: every example assembles with Limoniter and runs on an emulated Amiga 500 with Kickstart 1.3 and 3.1, and on faster Amigas, before it goes in. Click a chapter to read it.

## Targets

Every example must work on an **Amiga 500**: a 68000, the OCS or ECS chipset, 512K of chip RAM (and often 512K of slow RAM), Kickstart 1.3 or later. Chapters about AGA are marked as such, and the guidelines cover what breaks on faster processors.

## Chapters

| | Chapter | From v7 |
|---|---|---|
| 1 | [**Introduction**](01-introduction.md): what v8 is, its history, every v7 contributor credited, CC0 | Introduction |
| 2 | [**Working with Limoniter**](02-working-with-limoniter.md): a project, F9 to build and run, debugging, the hardware register help, crunching, trackmo disks | *new* (replaces Assembler) |
| 3 | [**General guidelines**](03-general-guidelines.md): the v7 rules that still hold, corrected (slow RAM, caches, sections, version strings) | General Guidelines |
| 4 | [**Startup and exit**](04-startup-and-exit.md): one startup with an `OS_FRIENDLY` flag, taking the machine completely or leaving the system running to load from disk; the VBR; giving everything back | Startup and Exit, startup.asm |
| 5 | [**Interrupts**](05-interrupts.md): vertical blank, copper and CIA interrupts, both ways; INTREQ on fast CPUs | Interrupts |
| 6 | [**The copper**](06-copper.md): waits, line 255, colour changes, copper-driven effects | Copper |
| 7 | [**The blitter**](07-blitter.md): owning it, waiting for it, minterms, fills, lines, clearing | Blitter |
| 8 | [**Display and video**](08-display.md): PAL and NTSC, display windows and fetch, scrolling, sprites | Video |
| 9 | [**AGA**](09-aga.md) *(AGA only)*: FMODE, alignment, 24-bit colour, sprites, HAM8 | AGA |
| 10 | [**Chunky to planar**](10-chunky-to-planar.md) *(AGA, CD32 and A500)*: the merge routines for the processor, the blitter's share on an A500, and the CD32's Akiko, measured | *new* |
| 11 | [**Input**](11-input.md): mouse and joystick, and the keyboard handshake done properly | Input |
| 12 | [**Sound and music**](12-sound-and-music.md): LightSpeedPlayer, The Player 6.1, ptplayer, audio DMA timing | Tracker |
| 13 | [**3D vectors**](13-vectors.md): rewritten with tested code | Vectors |
| 14 | [**The 68000 and later processors**](14-processors.md): optimising, cycle costs, 68020 to 68060 issues | Optimising, 680x0 issues |
| 15 | [**Loading and memory**](15-loading-and-memory.md): allocation; chip, slow and fast RAM; loading with the OS or a trackloader | Misc, Kickstart |
| 16 | [**Debugging and testing**](16-debugging-and-testing.md): the emulators, the target machines, real hardware | Debugging |
| 17 | [**Writing games**](17-writing-games.md): the game loop at 50 and 60 Hz, states, eight-way scrolling tile maps, bobs and sprite multiplexing, collisions, the CD32 pad, sound effects over music, fixed point and random numbers, saving high scores, WHDLoad; with a whole small game | *new* |
| 18 | [**Releasing**](18-releasing.md): crunching, ADF and HDF, WHDLoad, scene sites | *new* |
| 19 | [**Further reading**](19-further-reading.md) | Books |

Left out of v8: Action Replay, CDTV, Reading C and Beginners (AMOS), and the advice on 1993's tools (K-Seka, ArgAsm, CygnusEd, Tetrapack).

## Corrections to v7 already found

- **Slow RAM.** v7 says memory at $C00000 "does not exist on any production machines". The Amiga 500's trapdoor expansion puts 512K right there, and it's very common. The rule that stands: never assume any memory is there; allocate it.
- **The VBR.** v7 never mentions it. On a 68010 or later the exception vectors can be moved, so writing the level 3 interrupt's address to $6C directly can do nothing at all. v8's startup finds the VBR.
- **protracker_replay.s** in v7's archive is someone else's replayer, not part of How To Code; v8 uses ones whose terms are clear.

## Examples

In `examples/`. Each is a whole program in Devpac / vasm syntax, needing no include files, that assembles with Limoniter's own assembler (and with vasm) and runs with F9.
