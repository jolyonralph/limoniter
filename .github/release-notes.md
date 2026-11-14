## What's new in 0.8.0

Limoniter now runs on Linux, writes to many more machines' disks, and converts Amiga and Atari ST word processor documents.

**Linux**
- **AppImage and .deb** downloads, for 64-bit Intel and AMD computers. It runs under Wayland and X11, and remote-file passwords are kept in GNOME Keyring or KWallet.

**Changing more disk images**
- **Add, delete and rename files** on Apple II DOS 3.3 and ProDOS disks, BBC Micro DFS and ADFS discs, Spectrum TR-DOS disks and SCL archives, Mac HFS disks (resource forks, types and creators included), and Atari MyDOS and SpartaDOS disks; Applesoft and TR-DOS BASIC listings are tokenised as they go on.
- **New blank images** for each of them: Apple II 140K and 800K, BBC 100K to 640K, TR-DOS 640K, Mac 400K, 800K and 1.4MB, MyDOS and SpartaDOS.
- Each is checked against its machine's own tools: AppleCommander, a2kit, beebtools, MMB_Utils, Apple's fsck_hfs, hfsutils, lsatr, and a Pentagon's TR-DOS running in ZEsarUX.

**Amiga and Atari ST documents**
- **Converted to Markdown, plain text or RTF**: IFF formatted text (FTXT), ProWrite, AmigaGuide (each node a section, with its links), Protext, 1st Word and 1st Word Plus, WordPerfect 4 and 5 (Amiga, ST and DOS), Signum! 1 and 2, Final Writer and Wordworth. Bold, italics, underlining, headings, footnotes, headers and footers are kept.

**More media read**
- **Oric** Sedoric disks, **ZX81** programs on TZX tapes, **Color Computer and Dragon** `.cas` tapes, **Atari 8-bit** Koala and HIP pictures, **Compact Pro** archives and **Disk Copy 6** images.
- **Flux images** of Commodore 1571 disks, Apple II 13-sector disks, and Mac 400K and 800K disks.

**Easier to use**
- **File › Open a file from the web**: opens a file from an http or https address.
- **Open recent** has **Clean up** (removes files that are gone) and **Clear list**.
- **Screen readers** can follow the editor, tabs, the command box and the image browser; **F10** opens the menus, and **Ctrl+F6** moves out of the editor.
- **OpticOS**: the Spectrum keyboard shows OpticOS's own keys.
- **A user guide** on the website: [limoniter.com/guide](https://limoniter.com/guide/).

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon Macs (M1 and later). Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

See [what Limoniter can do](https://limoniter.com/features.html) and [how it compares](https://limoniter.com/compare.html).
