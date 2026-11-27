## What's new in 0.17.1

Limoniter 0.17.1 lets you install an emulator for Limoniter's window straight from F9, lets you change what F9 remembers, and offers a new Amiga C project from File, New as.

**Emulators in Limoniter's window**
- **Install from F9**: when a machine's "in Limoniter's window" choice is greyed out because limoniter-retro or its core isn't downloaded, the **Run on machine** box has a button that downloads what's missing in one go ("Install limoniter-retro and the PUAE core").

**F9**
- **Where F9 runs programs**, in Devices and emulators: each machine F9 remembers, with **Change** and **Forget**. Before, undoing "Always run there" meant editing the settings file.

**And**
- **File, New as, Amiga** offers **New Amiga C project…** beside the single files.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **Windows, portable:** `Limoniter-…-Windows-portable.zip`, with no installer and no administrator needed: unzip it anywhere (a USB stick, say) and run `limoniter.exe`. It keeps its settings and everything else in the `Limoniter data` folder beside it, leaving nothing on the PC, so several versions can sit side by side. It's unsigned too, so Windows warns about it as it does about the installer.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

`SHA256SUMS.txt` lists each download's SHA-256, and `SHA256SUMS.txt.minisig` is its signature, which Limoniter checks before it updates itself.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
