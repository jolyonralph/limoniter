## What's new in 0.14.2

Limoniter 0.14.2 is for Amiga programmers above all: new C projects with their makefiles kept up to date, locale catalogs, Workbench icons, the AutoDocs in a window, AmigaShell scripts, GNU as, and Shell programs run on your own computer with vamos. It also installs Boriel's ZX Basic for you, puts File, New as into sub-menus with the modern languages, and brings everything from 0.14.1, which was never published.

**Amiga C**
- **New Amiga C project** (Tools, Amiga, or Project) makes a folder from a template: a Shell or Workbench program for AmigaOS 1.3 or 3, a ReAction program for 3.2, an MUI 5 program, or a multilingual program with its catalog. It comes with makefiles for amiga-gcc, vbcc and SAS/C, a Workbench icon and build tasks.
- **Makefiles kept up to date:** as you add, rename and delete C files, Limoniter rewrites only its own part of each makefile, choosing the maths library when you use floating point. F9 builds the way the makefiles do.
- **Import an Amiga C project** sorts a folder of sources, headers, guides, icons and SAS/C's files, and adds makefiles only where there are none.
- **Locale catalogs:** FlexCat's `.cd` and `.ct` files are coloured and checked against each other (missing strings, lengths, `%` commands, Latin 1), and Limoniter writes the `.catalog` files and the strings' header itself, or with FlexCat.

**Workbench icons and vamos**
- **`.info` files are shown as their pictures**, normal and selected, whichever kind they are: the original icons, NewIcons, GlowIcons and OS 4's PNG icons. **Workbench icon…** changes the kind, default tool, stack, position and tool types, and an icon you don't change is saved byte for byte.
- **An icon for every program F9 builds**, if you want one, from Limoniter's own or one you choose.
- **vamos** (from amitools) runs AmigaShell programs and scripts on your own computer, with their output in the Output panel. Limoniter offers to install it, and SAS/C can build through it.

**Reading and writing Amiga code**
- **AutoDoc reader** (Tools, Amiga): the NDK's autodocs, and MUI's if you point to them, in a window beside your source. Right-click a function to jump to its explanation.
- **AmigaShell scripts are coloured** (the Startup-Sequence, `.key` and `.bra` lines, IF, SKIP and LAB), with their labels in the outline.
- **GNU as's 68000 dialect** (m68k-amigaos-as) is coloured and used to build. Source written for one assembler is never handed to another, and a project can choose its own.

**Also new**
- **Boriel's ZX Basic installed for you:** when zxbc isn't found, F9 and the Compilers box offer to install it: the ready-made build from boriel.com on Windows and Linux, or a private Python environment elsewhere.
- **File, New as is in sub-menus** by machine, with **Modern languages**: an HTML page, CSS, JavaScript, TypeScript, JSON, Python, C, C++, Rust, Go, Java, C#, PHP, Ruby, Lua, shell, PowerShell and batch scripts, SQL, YAML, TOML and XML.
- **Plain HTML is recognised as HTML** when a file has no name to go by, rather than as Svelte.

**From 0.14.1**
*Emulators*
- **Get an emulator** now installs MAME on Windows, from its GitHub releases, along with Hatari, XRoar, WinUAE, BlastEm, Fuse, BGB, Gearsystem, Gearcoleco and TIC-80. On Linux it adds Mesen, TIC-80 and SameBoy, and on a Mac BlastEm, Gearsystem and FCEUX. The Mac commands for Stella, mGBA and ZEsarUX, which no longer worked, are put right.
- **Emulators you unpack yourself are found:** in a folder at the top of a drive (`C:\MAME`), and as a program straight inside Downloads or the Desktop, as MAME's own unpacker leaves it.
- MAME's link now goes to its GitHub releases, as its website isn't answering.

*For 6809 and Dragon programmers*
- **Cycle counts beside 6809 and 6309 code**, as for the 6502 and Z80: every opcode and indexed mode, from Motorola's and Hitachi's figures. The 6309's counts are given for its native mode or its 6809 mode, as the source sets with `PRAGMA 6309` or `LDMD`. They show in the status bar and on hover, and selected lines are added up.
- **F9 to a Dragon DOS disk:** with `; run from: disk`, or the choice in XRoar's settings, the program goes on a disk made for the run, and XRoar starts with it in drive 1 and `RUN"NAME.BIN"` typed. A `.vdk` disk you open runs with F9 too, and files on a Dragon DOS disk can be renamed.

*Also*
- **Debug settings kept:** the program, arguments, folder and debugger chosen for each file are kept for next time. A project's files keep them with the project, so they move with it; other files keep them in Limoniter's settings.
- `limoniter --version` (or `-V`) prints the version with no window. An option Limoniter doesn't know is now refused with the usage, rather than opened as a file.
- F9 says "A program was made" for an Amiga program or another assembler's program, instead of calling it a tape. Its problem count is translated too.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
