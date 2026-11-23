## What's new in 0.16.4

Limoniter 0.16.4 puts Code and Retro on the menu bar, adds File, Export and a choice of shell for the terminal, starts npm's tools on Windows, and fixes a crash, the window size slider and Settings' widths.

**Menus**
- **Code** and **Retro** on the menu bar, as the ribbon has them. **Tools** keeps what's for any file.
- **File, Export**: Code as a picture, a tape's sound (WAV), and the tokenised BASIC programs.
- What works on plain text (tokenising as BASIC, reformatting, Format document) is greyed out while a file shows as a notebook, a table, the data viewer or a picture.
- **ROT13** and **ROT26** in Edit, Change case.

**Windows**
- npm's programs, such as **Intelephense** and **Prettier**, start now: Windows said they weren't found.
- The terminal's **+** offers PowerShell, Command Prompt, Git Bash and each **WSL** distribution, and keeps the one you choose.
- Downloaded tools (the PHP and JavaScript debuggers) are put in place even when a virus scanner holds the new files for a moment.

**Notebooks**
- **Code as a picture** on a notebook crashed Limoniter: it uses the chosen cell now, and a picture too big to draw is refused instead.
- **Convert a number** works on what's selected in a cell.
- The example notebook shows its **Install** button when pandas or matplotlib is missing, and installing a Python kernel adds pandas and matplotlib.

**Fixes**
- **Window font and size**: the window keeps its size while the slider's dragged, taking the new size when it's let go.
- **Settings**: rows that were wider than the box wrap onto a second line.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **Windows, portable:** `Limoniter-…-Windows-portable.zip`, with no installer and no administrator needed: unzip it anywhere (a USB stick, say) and run `limoniter.exe`. It keeps its settings and everything else in the `Limoniter data` folder beside it, leaving nothing on the PC, so several versions can sit side by side. It's unsigned too, so Windows warns about it as it does about the installer.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

`SHA256SUMS.txt` lists each download's SHA-256, and `SHA256SUMS.txt.minisig` is its signature, which Limoniter checks before it updates itself.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
