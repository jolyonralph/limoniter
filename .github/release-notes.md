## What's new in 0.16.2

Limoniter 0.16.2 makes the Explorer's recency slider quick on big projects, opens links again, puts the licences behind a button in the About box, and adds example notebooks.

**Fixes**
- **The recency slider is quick on big projects**: it no longer reads every file under every folder. Each folder's own files are ranked by their dates, and its subfolders by the folder's own date, as the folder is shown.
- **Links open again**: the About box's GitHub link and the tools' website links did nothing when clicked.
- **Project settings**: each tab fills the window, so making it taller shows more, not empty space.

**Small additions**
- **Example notebooks**: File, New as has a Python notebook and an R notebook to try a kernel with. They show printed text, a result, a table, a plot, an error with its traceback and, in Python, `input()` answered in a box.
- **Licence information** in the About box opens the licences of Limoniter and of the work it's built on, in a tab to read.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **Windows, portable:** `Limoniter-…-Windows-portable.zip`, with no installer and no administrator needed: unzip it anywhere (a USB stick, say) and run `limoniter.exe`. It keeps its settings and everything else in the `Limoniter data` folder beside it, leaving nothing on the PC, so several versions can sit side by side. It's unsigned too, so Windows warns about it as it does about the installer.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

`SHA256SUMS.txt` lists each download's SHA-256, and `SHA256SUMS.txt.minisig` is its signature, which Limoniter checks before it updates itself.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
