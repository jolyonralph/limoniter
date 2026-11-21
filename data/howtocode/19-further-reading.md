# 19. Further reading

[‹ Releasing](18-releasing.md) · [Contents](README.md)

v7 ended with a list of books, and a note that the essential ones cost money and came from Commodore's developer support. Today almost everything you need is free and online. Every link here was checked when this chapter was written (November 2026). Sites move, so if one has gone, its name and the people behind it should find it again.

## The manuals

These are the primary sources: every fact in v8 is checked against them.

- **Amiga Developer Docs**, <http://amigadev.elowar.com/>: the manuals from the *Amiga Developer CD 2.1*, readable as web pages, with a search (the site says the complete CD can still be bought from Vesalia):
  - the ***Amiga Hardware Reference Manual***, 3rd edition: <http://amigadev.elowar.com/read/ADCD_2.1/Hardware_Manual_guide/node0000.html>. Commodore-Amiga's reference for the custom chips, the CIAs, the memory map and the keyboard. Start with chapter 1's *General Development Guidelines*. It covers OCS and ECS; AGA came after it.
  - the ***ROM Kernel Reference Manual: Libraries***: <http://amigadev.elowar.com/read/ADCD_2.1/Libraries_Manual_guide/node0000.html>. How the system works: exec, graphics, Intuition, and the rest.
  - the ***ROM Kernel Reference Manual: Devices***: <http://amigadev.elowar.com/read/ADCD_2.1/Devices_Manual_guide/node0000.html>. input.device, trackdisk.device, audio.device and their friends.
  - ***Includes and Autodocs*** for Kickstart 2.0 (<http://amigadev.elowar.com/read/ADCD_2.1/Includes_and_Autodocs_2._guide/node0000.html>) and AmigaOS 3.5 (<http://amigadev.elowar.com/read/ADCD_2.1/Includes_and_Autodocs_3._guide/node0000.html>).
- **Scans of the printed books** are on the Internet Archive, including the *Hardware Reference Manual*, 3rd edition (<https://archive.org/details/amiga-hardware-reference-manual-3rd-edition>) and the *ROM Kernel Reference Manual: Libraries*, 3rd edition (<https://archive.org/details/amiga-rom-kernel-reference-manual-libraries-3rd-edition>).
- **NDK 3.2**, the Native Developer Kit for AmigaOS 3.2, from **Hyperion Entertainment**: <https://aminet.net/package/dev/misc/NDK3.2>. The assembler includes (`Include_I/`), the C headers, and the Autodocs for every library, as text and as AmigaGuide, kept up to date (Release 4 when this was written). Each Autodoc says which version added each function. Limoniter downloads it when you ask, and its assembler finds its includes. Hyperion's own site: <https://www.hyperion-entertainment.com/>.
- **Motorola's processor manuals**: the *M68000 Family Programmer's Reference Manual* (1992) and the user's manuals for each processor, from the 68000 to the 68060 (one folder for each), are scanned at **bitsavers**, Al Kossow's archive of computer documentation: <http://bitsavers.org/components/motorola/68000/>. They're where cycle counts and each instruction's effect on the flags come from.

## Tools

What [working with Limoniter](02-working-with-limoniter.md) uses, and the rest of today's usual kit.

- **vasm** (<http://www.compilers.de/vasm.html>), the portable assembler by **Volker Barthelmann** and **Frank Wille**, and **vlink** (<http://www.compilers.de/vlink.html>), Frank Wille's linker. Both are widely used for Amiga cross-development. v8's examples assemble with vasm as well as with Limoniter's own assembler.
- **FS-UAE** (<https://fs-uae.net/>), the Amiga emulator for Windows, macOS and Linux, by **Frode Solheim**. v8's examples are tested in it.
- **WinUAE** (<https://www.winuae.net/>), the Windows emulator, by **Toni Wilen**: known for its accuracy, with a cycle-exact mode for timing effects, and a full debugger.
- **amiga-debug** (<https://github.com/BartmanAbyss/vscode-amiga-debug>), by **Bartman / Abyss**: a Visual Studio Code extension to build, debug and profile Amiga programs (in C and C++), with its own WinUAE and FS-UAE, a frame profiler and a graphics debugger. Also on the Visual Studio Marketplace: <https://marketplace.visualstudio.com/items?itemName=BartmanAbyss.amiga-debug>.
- **uae-dap** (<https://github.com/grahambates/uae-dap>), by **Graham Bates**: a debug adapter that lets an editor debug a program running in FS-UAE or WinUAE. Limoniter's Amiga debugging works with it.
- **Amiga Assembly** (<https://github.com/prb28/vscode-amiga-assembly>), by prb28: a Visual Studio Code extension for 68000 assembly on the Amiga.
- **Shrinkler** (<https://github.com/askeksa/Shrinkler>), by **Aske Simon Christensen (Blueberry)**: widely used for demos and intros today. See [releasing](18-releasing.md).
- **Cranker** (<https://aminet.net/package/util/pack/cranker>), by **Timm S. Müller** (The Electronic Knights): a cruncher whose programs decrunch while they load, needing no extra memory.
- **LightSpeedPlayer** (<https://github.com/arnaud-carre/LSPlayer>), by **Arnaud Carré (Leonard / Oxygene)**: a ProTracker module converter and replayer, and the fastest replayer there is on a 68000 by its own benchmarks. See [sound and music](12-sound-and-music.md).
- **The Player 6.1** (<https://aminet.net/package/mus/misc/P6108>): Jarno Paananen's (Guru / Sahara Surfers) replayer and converter, in the version kept up by Photon and others.
- **ptplayer** (<https://aminet.net/package/mus/play/ptplayer>), by **Frank Wille**: a ProTracker replayer with support for sound effects alongside the music.
- **WHDLoad** (<https://www.whdload.de/>), by **Bert Jahn**: runs games and demos from a hard drive. Worth knowing how it treats a demo, as many people will run yours that way.
- **Amiga Forever** (<https://www.amigaforever.com/>), by **Cloanto**: the licensed Kickstart ROMs (and Workbench) for emulators. Kickstart isn't free, and neither Limoniter nor v8 can include it.

## Writing games

What [writing games](17-writing-games.md) draws on.

- **ScrollingTricks** (<https://aminet.net/package/dev/src/ScrollingTrick>), by **Georg Steger** (1999): every usual way of scrolling a tile map, for each direction and all eight, explained with pictures of the buffer, and demo programs in C. Its archive has its own terms for games made with it.
- ***Xorshift RNGs***, by **George Marsaglia**, *Journal of Statistical Software*, 2003 (<https://www.jstatsoft.org/article/view/v008i14>): the random number generator in Gem Cave and `ch17_random.s`, in six pages.
- **WHDLoad's programming documentation** (<https://www.whdload.de/docs/en/howto.html>): how an install patches a game, which is the best guide to making one that's easy to install.
- **The CD32 pad**: AROS's `lowlevel.library` reads it (<https://github.com/aros-development-team/AROS/blob/master/arch/m68k-amiga/lowlevel/readjoyport.c>), and Gerd Kautzmann's description of its circuit is preserved at <https://www.mrdictionary.net/PSCD32/diary/sites/CD32Gamepad_gerdkautzmann_de.html>.

## People to ask

- **The English Amiga Board** (<https://eab.abime.net/>): the forum where Amiga coders gather, hosted at abime.net. Its coding forums include **Coders. Asm / Hardware** (<https://eab.abime.net/forumdisplay.php?f=112>) and **Coders. Tutorials** (<https://eab.abime.net/forumdisplay.php?f=73>). The site asks your browser to prove it isn't a robot first, so give it a moment.
- **Aminet** (<https://aminet.net/>), the Amiga's software archive since 1992, by **Urban Müller and the Aminet team**: tools, players, sources, and the NDK. v7 itself is still there.

## Tutorials

- **Photon / Scoopex**'s *Amiga Hardware Programming* videos (the *Asmskool* series, 46 parts) build a whole demo in assembly, from nothing to a released program: <https://www.youtube.com/playlist?list=PLc3ltHgmiidpK-s0eP5hTKJnjdTHz0_bW>, on his ScoopexUs channel, <https://www.youtube.com/@ScoopexUs>.
- **Coppershade** (<https://coppershade.org/>), Photon's site: the videos' files, and articles on the copper, the blitter, sprites, display setup, playroutines and more.

## The scene

- **Pouët** (<https://www.pouet.net/>), by **mandarine**, hosted on scene.org: every demo, with comments, and where to download it. The place to see what's been done, and to show what you've done.
- **Demozoo** (<https://demozoo.org/>), by its staff and contributors: the scene's encyclopaedia, with groups, people, parties and results.
- **scene.org** (<https://www.scene.org/>), *the International Demoscene Organization*: the scene's file archive (<https://files.scene.org/>), where parties put their releases.

## How To Code v7

- The original AmigaGuide release, as Comrade J uploaded it on 11 July 1993: <https://aminet.net/package/docs/misc/howtocode7>.
- An HTML copy of Jolyon Ralph's 2003 web edition: <https://jvaltane.kapsi.fi/amiga/howtocode/index.html>.

Read v7 for history and for the flavour of 1993. For facts, read v8: some of v7's are wrong (the chapters say which).

## v7's books: print history

v7 recommended these books in 1993. Most are long out of print. They're kept here as history, with v7's verdicts (in quotes) where they still help someone deciding whether to look for a copy.

- ***Amiga Hardware Reference Manual*** and the ***ROM Kernel Reference Manuals*** (*Libraries*, *Devices*, *Includes and Autodocs*), Addison-Wesley. "Essential." Now free online: see above.
- ***AmigaDOS Manual***, 3rd edition, Bantam. "Truly awful book, unfortunately the ONLY official dos.library reference." The NDK's dos.library Autodocs replace it for programming.
- ***Amiga User Interface Style Guide***, Addison-Wesley. "Probably the most boring book I've ever read." Not for demos.
- ***Mapping the Amiga***, by Rhett Anderson and Randy Thompson, Compute! Books. "One of my favourite books": an easy-to-read reference to Kickstart 1.3's functions and structures. It can be borrowed from the Internet Archive: <https://archive.org/details/mappingamiga0000ande>.
- ***Amiga System Programmer's Guide*** and ***Advanced Amiga System Programmer's Guide***, Abacus. v7 suggested the official books instead.
- ***Amiga Disk Drives Inside and Out***, Abacus. v7: "AVOID THIS BOOK! It has some of the worst code and coding practices I have ever seen in it." Still good advice.
- ***Amiga Real-time 3D Graphics***, by Andrew Tyler, Sigma Press (1992). Vector graphics in 68000 on the Amiga, from the basics to lit, solid worlds. See also [3D vectors](13-vectors.md).
- ***680x0 Programming by Example***, by Stan Kelly-Bootle, Howard W. Sams (1988). "Excellent book on 68000 programming", covering the 68000 to the 68030.
- ***Motorola's user's manuals*** for each processor: "worth gold when trying to estimate cycles". Free now: see bitsavers, above.
- ***The Discworld series***, by Terry Pratchett. "Nothing to do with Amigas, but excellent books. If you need a break from programming, or just inspiration for impossible things to have in an impossible demo, read one of these!" Still true. Oook.

[‹ Releasing](18-releasing.md) · [Contents](README.md)
