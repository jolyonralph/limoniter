## What's new in 0.17.0

Limoniter 0.17.0 runs emulators inside its own window, adds an SSH terminal, groups the Functions panel by class, and gives F9 three more machines: CP/M, the Agon Light and the Neo6502.

**Emulators in Limoniter's window**
- Machines run in a **panel beside your file**, docked like the built-in ones: the Spectrum (Fuse), C64 and the other Commodores (VICE), the Amiga (PUAE), Mega Drive, Master System and Game Gear (Genesis Plus GX, PicoDrive), Atari 2600 (Stella), and more, through libretro cores.
- **Keys, joypad and mouse** go to the machine; **Reset**, **Save state** and **Load state**; and a **memory view** of the machine as it runs.
- **F9's Run on** offers each machine "in Limoniter's window", and remembers it if you like.
- **Get an emulator** downloads the cores, and the small helper program that runs them, **limoniter-retro**: a separate GPL project, so Limoniter itself stays Apache-2.0.

**F9 on more machines**
- **CP/M**: Z80 source at 100h (or saying `; machine: cpm`) is built as a .COM and run in **RunCPM**, in the Terminal panel.
- **Agon Light**: BBC BASIC runs in **fab-agon-emulator**, from an SD card folder made for the run.
- **Neo6502**: 6502 code runs in its **emulator**, loaded at its address.

**Code**
- **The Functions panel by class**: each function under the class it's in, the rest under Not in a class; **Order** as in the file, A to Z, or private A to Z then public; private ones marked; classes fold; hovering shows the signature.
- **Project and Functions in one strip** (View, Panels, Stack Project and Functions): one above the other, with a divider to drag.
- **An SSH terminal**: SSH: and a server's name in the terminal's **+**, File, **Open a terminal on a server (SSH)**, and **Open a terminal here** in a remote project. It uses SFTP's saved servers, logins, keys and host key checks, and the server's terminal follows the panel's size.

**Projects**
- **Open the last project when Limoniter starts** (Settings, Files, and on the start page).
- **Always shown**: files and folders the recency slider never hides (README.md, src/main.rs, docs/, *.toml), in Project settings, Files, or with **Always show** in a right-click in the Explorer and the Project panel.

**And**
- **Dialogs can be moved**: About and the other boxes in the middle of the window can be dragged.
- **File, New as**: OpticBASIC is with the ZX machines.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **Windows, portable:** `Limoniter-…-Windows-portable.zip`, with no installer and no administrator needed: unzip it anywhere (a USB stick, say) and run `limoniter.exe`. It keeps its settings and everything else in the `Limoniter data` folder beside it, leaving nothing on the PC, so several versions can sit side by side. It's unsigned too, so Windows warns about it as it does about the installer.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

`SHA256SUMS.txt` lists each download's SHA-256, and `SHA256SUMS.txt.minisig` is its signature, which Limoniter checks before it updates itself.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
