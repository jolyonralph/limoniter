## What's new in 0.14.1

Limoniter 0.14.1 installs more emulators for you and finds the ones you install yourself. It counts the 6809's and 6309's cycles, runs Dragon programs from a Dragon DOS disk, and remembers how each program is debugged.

**Emulators**
- **Get an emulator** now installs MAME on Windows, from its GitHub releases, along with Hatari, XRoar, WinUAE, BlastEm, Fuse, BGB, Gearsystem, Gearcoleco and TIC-80. On Linux it adds Mesen, TIC-80 and SameBoy, and on a Mac BlastEm, Gearsystem and FCEUX. The Mac commands for Stella, mGBA and ZEsarUX, which no longer worked, are put right.
- **Emulators you unpack yourself are found:** in a folder at the top of a drive (`C:\MAME`), and as a program straight inside Downloads or the Desktop, as MAME's own unpacker leaves it.
- MAME's link now goes to its GitHub releases, as its website isn't answering.

**For 6809 and Dragon programmers**
- **Cycle counts beside 6809 and 6309 code**, as for the 6502 and Z80: every opcode and indexed mode, from Motorola's and Hitachi's figures. The 6309's counts are given for its native mode or its 6809 mode, as the source sets with `PRAGMA 6309` or `LDMD`. They show in the status bar and on hover, and selected lines are added up.
- **F9 to a Dragon DOS disk:** with `; run from: disk`, or the choice in XRoar's settings, the program goes on a disk made for the run, and XRoar starts with it in drive 1 and `RUN"NAME.BIN"` typed. A `.vdk` disk you open runs with F9 too, and files on a Dragon DOS disk can be renamed.

**Also**
- **Debug settings kept:** the program, arguments, folder and debugger chosen for each file are kept for next time. A project's files keep them with the project, so they move with it; other files keep them in Limoniter's settings.
- `limoniter --version` (or `-V`) prints the version with no window. An option Limoniter doesn't know is now refused with the usage, rather than opened as a file.
- F9 says "A program was made" for an Amiga program or another assembler's program, instead of calling it a tape. Its problem count is translated too.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
