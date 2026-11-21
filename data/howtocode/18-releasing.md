# 18. Releasing

[‹ Writing games](17-writing-games.md) · [Contents](README.md) · [Further reading ›](19-further-reading.md)

The program you release is not the program you tested. It has been crunched, so a decruncher you didn't write runs first. It's on a disk, or in an archive, started from a disk or an icon. And it's going to machines you've never seen. This chapter is about the last steps: crunching, packaging, the small things a release needs, where to put it, and a checklist that ties it all back to the machines in [chapter 16](16-debugging-and-testing.md).

## To crunch or not

A **cruncher** packs a program into a smaller program that unpacks (decrunches) itself in memory and then runs. v7 pointed out that if you spread your demo electronically, an archive (LhA in 1993) makes the download as small as most crunchers would, so you may not need one. That's still true. Crunch when the size of the program itself matters:

- **Size-limited competitions**: a 4K or 64K intro is judged on the size of the program file.
- **Floppy disks**: to fit, and to load faster. Reading a floppy is so slow that loading less and unpacking it can be quicker than loading it all.

v7's recommended crunchers (PowerPacker, Imploder, Titanics Cruncher) have been overtaken. Two are used today, and Limoniter runs both.

### Shrinkler

**Shrinkler**, by Aske Simon Christensen (Blueberry), <https://github.com/askeksa/Shrinkler>, is "designed for maximum compression of Amiga 64k and 4k intros, and everything in between", and its README says its crunched programs "are compatible with all Amiga CPUs and kickstarts". It packs harder than anything else, and it pays for that in decrunching time: it's slow to unpack, especially on a 68000. It also has a mode for crunching data rather than a program, with a 68000 routine to decrunch it in your own code.

### Cranker

**Cranker**, by Timm S. Müller (The Electronic Knights), <https://aminet.net/package/util/pack/cranker>, takes the other side. Its programs decrunch **while they load**, from the disk, and need no extra memory to do it.

That last point matters more than it sounds. A big OCS demo usually reserves most of the memory it needs in its own sections (BSS for screens and buffers), as it should: the memory is then there before the demo starts. But most crunchers need the crunched data and all the program's sections in memory at the same time, so a demo that just fitted in 512K of chip RAM and 512K of slow RAM no longer does. Cranker's readme explains this, and gives its author's measurements for one large demo, Rebels' *Retrodentro*, starting from a floppy on Kickstart 1.3:

| | Size | Time to start |
|---|---|---|
| Not crunched | 499,768 bytes | 41 s |
| Cranker | 217,548 bytes | 20 s |
| Shrinkler | 179,472 bytes | 87 s |

(Imploder made it 213,332 bytes, but then it didn't run in 1 MB.) These are Cranker's author's figures, not v8's.

### Which one

- **A size-limited intro**: Shrinkler. Every byte counts, and an intro is small enough to decrunch quickly.
- **A big demo on a floppy for an A500**: Cranker, for the memory and the loading time. Or Shrinkler with its overlap option (`-o`), which lets crunched and decrunched data share memory; time how long it takes to start on an A500.
- **Anything else**: try both, and run both on an emulated A500 with a floppy, timing the start. Or don't crunch at all, and let the archive do the packing.

### Crunching in Limoniter

**File › Run on › Amiga crunchers and music…** (or **Tools › Amiga › Crunchers and music…**) has three pages.

**Crunch a program** crunches the AmigaDOS program open in front (it can be open as hex), or one chosen in a disk image's browser. Choose the cruncher and its options:

- Shrinkler's: data rather than a program (`-d`, with a header saying the sizes, `-w`), merging hunks of the same memory type (`-h`), overlap (`-o`), the smaller, more restricted decrunch header (`-m`), passing the command line on (`-c`), no parity context, for byte-oriented data (`-b`), the effort (`-1` to `-9`, with your own iterations and references), a line of text shown before decrunching (`-t`), and a register flashed while it works (`-f`, `DFF180` flashes the background colour).
- Cranker's: data rather than a program, fast (packs less), merging hunks (which Cranker itself calls dangerous), and what shows while it loads.

The crunched copy is saved beside the original as `name_crunched`, with the sizes before and after; what the cruncher printed is in the Output panel. It runs in the background, with **Stop**.

**Tools** says where each cruncher is, or lets you point at it, and whose it is and under what licence. Shrinkler's release can be downloaded from GitHub with **Download its release** (only when you ask). Cranker's Aminet package has the Amiga program and its C source; Limoniter's Tools page says how to build it for your computer. As Limoniter describes their terms: Shrinkler has its own permissive licence, and programs crunched with it are their authors' own; Cranker is under the GNU GPL version 2, and its decrunchers are free of copyright.

The Tools page also has **After F9 builds an Amiga program, crunch a copy with:** set it to Shrinkler or Cranker, and every F9 also leaves a crunched copy beside the source, so the crunched program is always there to test.

### Test the crunched program

A crunched program is a new program. Its decruncher writes your program into memory and jumps to it, and that's exactly the code-written-at-run-time case of [chapter 3](03-general-guidelines.md): on a 68040 or 68060, the caches must be cleared before the jump. Shrinkler's README promises programs that work on every Amiga processor; whatever you use, prove it by running the crunched program on every machine in [chapter 16](16-debugging-and-testing.md), and from a floppy if that's how it's going out.

## Don't protect it

v7 strongly advised against "protecting" code by encrypting it: it was easy to make code that failed on anything faster than a 68000, and "lamers will still use Action Replay to get at your code". Both points stand, and harder. Decrypting code at run time is code written at run time, with all the cache trouble that brings. And today anyone can stop your demo in an emulator, save its memory and disassemble it with the labels put back. Limoniter's own disassembler does that in a moment.

If you want people to learn from your demo, release the source. If you don't, let the demo speak for itself.

## Packaging

### An archive

Most Amiga demos are released as an archive: the program, its icon, a readme, and any data files it loads. Amigas unpack LhA archives with `lha`; ZIP is what scene.org prefers (below), and is fine for anyone downloading on another computer.

### An ADF disk with files

An `.adf` is an image of an 880K floppy, ready for an emulator, a Gotek, or writing to a real disk. A disk with AmigaDOS on it holds your demo as files, with an `S/Startup-Sequence` that runs it.

Limoniter makes the disk: **File › New disk or tape image…**, then **Amiga floppy, 880K, OFS (.adf)**, and add the files to it in its browser. Use **OFS**, the old file system: Kickstart 1.3 can't read a floppy in FFS, which arrived in the ROM with Kickstart 2.0.

A new disk is blank as the Workbench's Format leaves it: it isn't **bootable**. To make it boot, it needs a boot block with the standard boot code, which Limoniter can't write. Put the disk in an emulated Amiga with Workbench and type `Install DF0:` (or the drive it's in) in the Shell.

F9 with an `.adf` in front boots it in the emulator, so you can try the disk as people will get it.

### A disk with no file system: a trackmo

Many demos (and most games) don't use AmigaDOS on their disks at all. A boot block reads the program from the disk's sectors, and the demo loads its later parts from the disk itself, with no file system in the way: a **trackmo**. [Chapter 15](15-loading-and-memory.md) is about loading that way.

**Tools › Amiga › Boot disk with no file system (trackmo)** builds such a disk. Each **piece** is an assembler source (assembled by Limoniter at its `org`, or at the address you give) or any other file (graphics, music, packed data), put at a **first sector** you choose and loaded to an **address**. One piece is the one to **start**; a piece can be left unloaded, for your program to load itself later. A map of the disk shows which sectors are used.

Kickstart reads the first two sectors, the **boot block**, and runs the code in it. Limoniter's own boot block reads each piece from its sectors with trackdisk.device, turns the drive's motor off, and jumps to the start, with `a6` still holding ExecBase. **Change it…** saves its source beside your plan for you to make your own (a fade, a loader that decrunches); Limoniter writes the table of pieces between its two marked lines each time. However it's written, it's checked to fit in its 1024 bytes, given its "DOS" mark, and its checksum is worked out as Kickstart checks it.

**Write the disk…** saves it as an `.adf` (880K, or 1760K for a high-density disk), from blank or over a disk image of your own, keeping what's already in the other sectors. **Run it** boots it on the Amiga emulator. The plan is kept in a `.bootdisk` file, to build the disk again after any change.

A trackmo can only be started from its disk: it has no files to copy to a hard drive. That's what WHDLoad is for (below).

### HDF: a hard disk

An `.hdf` is an image of a hard drive. Some large demos, AGA ones especially, are offered as an HDF for emulator users, ready to boot. Limoniter can look inside an HDF (partitions and all) but doesn't make them; WinUAE can, from its hard drive settings (**Create**). For people with real hard drives, an archive of files with an icon is still the friendlier release.

### WHDLoad

**WHDLoad**, by Bert Jahn (<https://whdload.de/>), is "the AmigaOS friendly Harddisk-Install package". In its own words, it's "designed for easy installation of programs to a harddisk (demos, games, ...) which kill the operating system. To install a program, a so called "Slave" must be written. The Slave is the interface between the program and WHDLoad, and co-ordinates the reading and writing of files." Each installed program has its own **slave**: a small program that patches the original's disk loading so it reads files from the hard drive instead, and lets the user quit back to Workbench with a key.

That's why so many demos have WHDLoad installs: trackmos and demos that took the machine completely could only be run from their disks, and Amiga owners with hard drives (and emulators) wanted them there. WHDLoad itself needs Kickstart 2.0 and 1 MB of memory; it runs on a 68000, but its documentation recommends a 68010 or later, for the VBR that some installs' quit key needs.

You don't have to write a slave for your demo. The installs are written by WHDLoad's community, and the site says where to send new ones. But you can make one unnecessary: a demo that loads its parts as files through the system (`OS_FRIENDLY=1`, [chapter 15](15-loading-and-memory.md)) can simply be copied to a hard drive.

## The finishing touches

### A version string

Put a version string in the program ([chapter 3](03-general-guidelines.md) explains the format):

```asm
        section data,data
        dc.b    "$VER: mydemo 1.0 (20.11.2026)",0
        even
```

It has to be in the program, not only in a comment in the source: v8's examples carry their `$VER:` line in a comment, which documents the source but isn't in the program they make. And once the program is crunched, its version string is packed with the rest, out of the `Version` command's sight. So put the version in the readme and in the archive's name as well.

### An icon

People who start programs from Workbench need an icon: a `.info` file of the same name, beside the program (`mydemo.info` for `mydemo`). Make a **tool** icon, with Workbench's own icon editor, or copy one and change its picture. v8's startup handles being started from an icon ([chapter 4](04-startup-and-exit.md)): a demo that doesn't, crashes when someone double-clicks it. Limoniter doesn't make icons.

### A readme

Write a short text file to go with the demo. Say:

- its name, its version and date, and who made it (and the music and the graphics: credit everyone);
- what it runs on, as you tested it: "A500 with 512K chip and 512K slow RAM, Kickstart 1.3 or later, OCS, ECS or AGA, 68000 to 68060", and anything it needs (1 MB of chip RAM, AGA, fast RAM);
- how to start it, and how to leave it (the left mouse button);
- where it was released (a party, a competition), and how to reach you.

**File › New as › Amiga text** starts a text file in the Amiga's character set with the Amiga's line endings, so it reads properly on the Amiga too.

Uploading to Aminet needs a readme in Aminet's own format: the same name as the archive with `.readme`, lines of 78 characters at most, linefeeds only, and fields at the top including `Short:`, `Uploader:`, `Type:` and `Architecture:`. Its rules are on Aminet's wiki: <https://wiki.aminet.net/Uploading_instructions>.

## Where to release

- **A party.** Most demos are released at a demo party, in a competition. Demozoo lists the parties.
- **scene.org's file archive** (<https://files.scene.org/>) is where demos are kept for good, and where Pouët and Demozoo link to. Upload by FTP to `ftp.scene.org`, logging in as `anonymous`: party releases go in `/incoming/parties/` (by year and party), others in `/incoming/demos/groups/` or `/incoming/demos/artists/`. Its FAQ (<https://files.scene.org/faq/>) asks for file names in lowercase letters, digits and underscores only, one file for each release (packed, not loose files), and ZIP rather than RAR. The staff move uploads into place, usually in a day or two.
- **Pouët** (<https://www.pouet.net/>): add your demo as a production with **Submit**, once you've logged in (with a SceneID). People will download it, comment on it and vote.
- **Demozoo** (<https://demozoo.org/>): add it to the encyclopaedia, with its credits and the party it was released at. You need an account, or a SceneID.
- **Aminet** (<https://aminet.net/>), the Amiga's own archive, has demo directories too (`demo/ecs`, `demo/aga`, `demo/intro` and others). Upload by FTP to `main.aminet.net`, into `/new`, with the readme above.

## The release checklist

Before you upload, with the program exactly as it will be released (crunched, from its disk or archive):

- [ ] Built from clean, from the source you're releasing, the way you're releasing it (`OS_FRIENDLY` as it should be, and `AUTOEXIT` off).
- [ ] Runs on an **A500**: 68000, OCS, 512K chip and 512K slow RAM, **Kickstart 1.3**. If it claims to run in 512K, tried with no slow RAM too.
- [ ] Runs with **Kickstart 3.1** (and 2.04, if you can).
- [ ] Runs on an **A1200**: 68020, AGA, 2 MB chip RAM and no fast RAM.
- [ ] Runs on a **68030, 68040 and 68060 with fast RAM**.
- [ ] Runs on **NTSC**, or, if it's PAL only, says so in the readme and still exits cleanly.
- [ ] **Gives the machine back** on every one of those: the CLI or Workbench works afterwards, the display is right, and the return code is 0. In a Shell, `Avail` before and after shows if memory has gone missing.
- [ ] Starts from the **CLI** and from its **icon**.
- [ ] From a **floppy**, if it's released on one: boots, loads in a reasonable time, and fits in memory.
- [ ] The crunched program run on all of the above, not just the uncrunched one.
- [ ] A **version string** in the program, and the version in the readme and the archive's name.
- [ ] A **readme** that credits everyone and says what it runs on, as tested.
- [ ] Everything in it is yours, or you have permission: music, graphics, fonts, and the players' and tools' own terms.

## What changed from v7

- **Crunchers**: Shrinkler and Cranker instead of PowerPacker, Imploder and Titanics Cruncher. v7's point that an archive is often enough still stands.
- **Protection**: v7's advice against it stands, and the emulator has made it pointless as well as dangerous.
- **New**: disk images (ADF, HDF), trackmo disks built in Limoniter, WHDLoad, and today's places to release: scene.org, Pouët, Demozoo and Aminet. In 1993, demos went out on disks by post and on BBSes.

[‹ Writing games](17-writing-games.md) · [Contents](README.md) · [Further reading ›](19-further-reading.md)
