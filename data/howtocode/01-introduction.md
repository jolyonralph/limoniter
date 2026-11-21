# 1. Introduction

[Contents](README.md) · [Working with Limoniter ›](02-working-with-limoniter.md)

How To Code is a guide to writing Amiga demos that work: on every Amiga they claim to run on, and leaving the machine as they found it. This is version 8. It keeps what version 7 got right in 1993, corrects what it got wrong, and swaps 1993's tools for today's.

It's written for someone who can already program, perhaps in 68000 assembly, perhaps in something else, and wants to write demos for the Amiga's own chips: the copper, the blitter, bitplanes, sprites and sound. It's also for anyone coming back after thirty years, who remembers `$DFF180` but not which of their old habits broke on an A1200.

## Where it came from

How To Code started as text files, `howtocode1.txt` to `howtocode6.txt`, the first uploaded over Christmas 1992. In the editor's words, it "started as an angry protest after several new demos I downloaded refused to work on my 3000". It grew from there into a general reference, with particular emphasis on the new Amiga 1200, whose AGA chipset Commodore had not released any hardware information about.

**Version 7**, the AmigaGuide release of 7 July 1993, was "How to write demos that work (Version 7), or the Amiga Demo Coders Reference Manual", edited by **Comrade J / Share and Enjoy**, with co-editor **Carl-Henrik Skårstedt (Asterix / Movement)**, who looked after the vector chapter. It went out on bulletin boards and FTP sites, and is still on Aminet as `docs/misc/howtocode7.lha`.

In 2003 **Jolyon Ralph** made an HTML edition of v7 for the web, with a new introduction explaining that Comrade J had been his own scene name all along. He wrote that many of the major crews who made the big Amiga 1200 demos had told him they worked from it, and that engineers from Commodore had contributed to v7 by email, at least one of whom remained anonymous.

v7 said it was "the work of many authors, although the spelling mistakes are all my fault!". It thanked these people, exactly as it listed them:

> Richard Aplin, Matthew Arnold, Dean Ashton, Andreas Axelsson, Michael Bauer, Bilbo the First/Hypenosis, Eddy Carroll, Mark Cassidy, Nils Liaaen Corneliusen, Walter Dao, David Dustin, Chris Green, Joep Grooten, Grue, Jim Hawkins, Arno Hollosi, Lars Holmgren, Philip Jespersen, Irmen de Jong, Marius Kintel, Christopher Klaus, Mark G Knibbs, Jesper R Larsen, Jacco de Leeuw, Tero Lehtonen, Patrik Lundquist, Jonas Matton, Stanley Merkx, Jesse Michael, Jonas Minnbergh, Richard Moeskops, John Derek Muir, Marco Nelissen, Karsten Niemeier, Boerge Noest, David Nouls, Mats Olsson, Dave Parkinson, Andrew Patterson, Raymond Penners, Vidar Petursson, Michael Pollard, Jolyon Ralph, Bjorn Reese, Vic Ricker, Timo Rossi, Michael Ryffel, Conrade Sanderson, Doz/Shining, Toby Simpson, Darrell Tam, Lehtonen Tero, Magnus Timmerby, Yee Tom, and Michel Vissers.
>
> for their comments and contributions, both rude and polite!

v8 owes them all. Where a section of v8 still rests on one person's contribution to v7 (Arno Hollosi's rules on system private structures, say, or Chris Green's case for demos that use the system), it says so.

## Version 8

v8 is a rewrite, not a new coat of paint. Every chapter was written afresh, and every fact in it checked again: against the *Amiga Hardware Reference Manual*, the ROM Kernel Manuals and the NDK, and by running the examples on emulated Amigas. Where v7 was wrong, v8 says so plainly. (It was wrong about slow RAM, for one, and silent about the VBR, which broke a lot of demos. See [the general guidelines](03-general-guidelines.md) and [startup and exit](04-startup-and-exit.md).)

It ships inside **Limoniter**, a text and hex editor with strong Amiga support: its own 68000 assembler that builds Amiga programs as vasm does, F9 to run them on an emulated Amiga, source-level debugging through the emulator, Shrinkler and Cranker to crunch programs, and LightSpeedPlayer's converter for music. You don't need Limoniter to use v8, though. Every example is plain Devpac/vasm syntax, and assembles with vasm too.

Some of v7 is left out: the chapters on Action Replay, CDTV, reading C and AMOS, and the advice on 1993's tools (K-Seka, ArgAsm, CygnusEd, Tetrapack). The [README](README.md) lists every chapter and what it came from.

## Free to use: CC0

v7 was "Copyright (C) 1993 Share and Enjoy, but may be freely distributed in any electronic form", and asked magazine editors to email first. v8 is released under **CC0 1.0** (<https://creativecommons.org/publicdomain/zero/1.0/>): it's given to the public domain, as far as the law allows. Copy it, change it, put it on a disk, use the startup code and the examples in your own demos, with or without a credit. (A greeting in your scroller is always welcome.)

CC0 covers this guide and its examples. The tools and players it points to (Shrinkler, LightSpeedPlayer and the rest) belong to their authors, under their own terms, and each chapter says what they are.

## What has changed since 1993

**The documentation is free.** In 1993 the V39 includes and Autodocs came only with a Commodore developer registration (£85 a year in the UK, v7 said), and v7 wasn't allowed to include them. Today the *Hardware Reference Manual* and the ROM Kernel Manuals can be read online, and Hyperion's NDK 3.2, the includes and Autodocs for AmigaOS 3.2, is a free download from Aminet. Its Autodocs say which version added each function. [Further reading](19-further-reading.md) has the links.

**The tools run on other computers.** Most Amiga demos are now written on a PC or a Mac and run in an emulator: an assembler such as vasm, a linker such as vlink, an editor, and FS-UAE or WinUAE to run the result in seconds. WinUAE's cycle-exact mode is close enough to a real A500 for timing an effect, and both emulators can be debugged through from an editor (Limoniter, or Bartman's extension for Visual Studio Code): see [debugging and testing](16-debugging-and-testing.md).

**The system has moved on.** Commodore's last Kickstart was 3.1 (V40). Since then Hyperion has released AmigaOS 3.1.4 (2018) and 3.2 (2021), with new ROMs for the classic machines, so a demo may now meet exec V46 or V47. The rules in this guide don't change for them. They're why a demo that follows them keeps working.

**The machines have changed under the demos.** Accelerators far beyond 1993's are widely used now: Vampire boards, whose 68080 core is built in an FPGA, and the PiStorm, a Raspberry Pi running a 68000 emulator in place of the processor. v7 predicted that Amigas would one day run "RISC chip processors running 680x0 emulation". It was right, and a demo that only ever ran on a 68000 will meet one.

**The scene is still here.** New Amiga demos come out every year, at parties and online, and Pouët and Demozoo list them all. Some are for AGA machines or accelerated ones, and many still for the plain A500.

## How to read this guide

If you're new to Limoniter, start with [chapter 2](02-working-with-limoniter.md): how to make a project, build and run with F9, and debug. Then read [chapter 3](03-general-guidelines.md), the rules everything else follows, and [chapter 4](04-startup-and-exit.md), the startup that every example is built on. After that, read in any order. Each chapter stands on its own, and links to the others where it needs to.

Every example is a whole demo, in `examples/`, named for its chapter (`ch06_waits.s`). Add one to a project and press F9. Each builds two ways, chosen by one line at its top:

```asm
OS_FRIENDLY     equ     0               ; 0: take the machine; 1: keep the system running
```

A few words used throughout:

- **OCS, ECS, AGA**: the three generations of custom chips. The Original Chip Set (A1000, early A500 and A2000), the Enhanced Chip Set (later A500s and A2000s, the A500+, A600 and A3000), and the Advanced Graphics Architecture (A1200, A4000, CD32).
- **Chip RAM**: memory the custom chips can read and write. Bitplanes, copper lists, sprites, samples and anything the blitter touches must be in it.
- **Fast RAM**: memory only the processor uses, so the chips never hold it up.
- **Slow RAM**: the A500's trapdoor expansion, at `$C00000`. The processor can use it, the chips can't, and it's no faster than chip RAM, though the system counts it as fast RAM. See [chapter 3](03-general-guidelines.md).
- **Kickstart 1.3, V34**: Kickstart releases go by name or by the version of their libraries. The table is in [chapter 3](03-general-guidelines.md).
- Hexadecimal is written `$DFF180`, as the Amiga's assemblers write it.

## Targets

Every example must work on an **Amiga 500**: a 68000 at 7.09 MHz (PAL), OCS or ECS, 512K of chip RAM (often with 512K of slow RAM), and Kickstart 1.3 or later. If it runs well there, it runs on most Amigas ever made.

Each is also run on a **68020, 68040 and 68060 with fast RAM**. That is where demos written on an A500 most often fail: caches, a processor that overtakes the blitter, vectors that aren't at address 0. Chapters about AGA are marked as such, and need an AGA machine (an A1200, A4000 or CD32).

Before an example goes in, it's run both ways on emulated A500s with Kickstart 1.3 and 3.1, and on the faster processors, and must put the machine back as it found it. If you find one that doesn't, or anything else in v8 that's wrong, please report it. v7 promised: "I won't be angry if 100 people point out the same mistake. If no one does and everyone tells me later that they knew - then I will!" That still stands.

As Comrade J put it in 1993: "I didn't write this for fun, I wrote it for you to use!"

[Contents](README.md) · [Working with Limoniter ›](02-working-with-limoniter.md)
