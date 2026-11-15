## What's new in 0.9.0

Limoniter now compares files and whole folders, keeps folders in sync, and merges three versions of a file, and Git can use it for diffs and conflicts. There are also new tools for writing demos and intros on the Amiga, Atari ST and Commodore 64.

**Compare and merge**
- **Compare files…** opens two files side by side in a tab, and both sides can be edited. Changed lines and the words within them are marked. An arrow copies a difference across, and Undo takes it back. Spaces, case and line endings can be ignored, and the differences can be saved as a patch. Binary files are compared byte by byte in hex, and pictures as pictures, with where they differ shown.
- **Compare folders…** compares two folders as a tree. Either side can be a folder, a Git revision, a disk image or archive, or a folder on an SFTP server. You can compare by size and time or by contents, and leave out what `.gitignore` leaves out.
- **Update** and **Mirror** keep folders in sync. You see every copy and deletion before anything happens, deletions are confirmed a second time, and each file that's replaced or deleted is backed up first. Files changed on both sides since the last sync are shown as conflicts and left alone.
- **Saved comparisons** keep their sides, filters and rules. They're saved in your project or on their own. **Rules** set how each kind of file is compared (as text that ignores timestamps, as bytes, as pictures, or skipped), and by default build folders such as `.git/`, `target/` and `node_modules/` are skipped.
- **Reports** can be saved as HTML, text, a counts-only summary, or CSV. **Apply a patch** shows what fits before anything is changed, and backs up each file first.
- **Merge** shows the original, mine and theirs above a result you can edit. Changes made on only one side are merged already, and each conflict waits for Take mine, Take theirs or Take both.
- **Git:** files with conflicts get **Resolve in merge…**. **Use Limoniter for Git diff and merge** makes `git difftool` and `git mergetool` open Limoniter. It asks first if you already use another tool, and **Stop using Limoniter for Git** puts back your earlier settings. The command line takes `limoniter --diff LEFT RIGHT` and `limoniter --merge BASE MINE THEIRS RESULT`.

**Amiga**
- **Code hints** from the Amiga NDK 3.2, downloaded from Aminet when you ask: what each function does and its arguments, `_LVO` offsets in the 68000 assembler, and structure fields on hover.
- **A guide to the hardware registers** for OCS, ECS and AGA, shown on hover, plus copper list tools.
- **A bitplane converter** that turns a PNG or JPEG into planes, a palette, ILBM, source, sprites and bobs.
- **Executables** built by the 68000 assembler, which run in FS-UAE or WinUAE, and source-level debugging through their GDB servers.
- **Demo tools:** a blitter minterm calculator, a table generator, and demo and copper starters. Shrinkler, Cranker, LSPConvert and P61 can be run on your files, and ProTracker modules show their details.

**Atari ST**
- **A guide to the hardware registers**, shown on hover, TOS calls, and calculators for the STE blitter and MFP timers, plus ST and STE demo starters.
- **A picture, sprite and font converter**, and Spectrum 512 pictures read and written.
- **Running in Hatari**: choose the machine, memory, monitor and CPU. Limoniter finds your TOS, or downloads EmuTOS. It makes bootable floppies and uses Hatari's profiler by label and line.
- **Music and packers:** SNDH and YM details, Pack-Ice and Atomik files unpacked wherever they're opened, and YM2149 tables.

**Commodore 64**
- **KickAssembler and 64tass**, with KickAssembler downloaded when you ask (its download states no licence, and Limoniter says so first). A guide to the VIC-II, SID, CIA and KERNAL, shown on hover.
- **Raster timing:** 6502 cycle counts, a raster line calculator, stable raster interrupts, and demo and intro starters.
- **A picture, sprite and charset converter**, with C64 modes in the character and sprite editor.
- **SID tunes, packers, CRT cartridges and TAP tapes.**

**Also**
- **OpticOS 1.0.2**, and Limoniter checks OpticOS's releases for newer ROMs and installs them (this can be turned off).
- Hatari keeps the program's output in the log while you're debugging.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon Macs (M1 and later). Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

See [what Limoniter can do](https://limoniter.com/features.html) and [how it compares](https://limoniter.com/compare.html).
