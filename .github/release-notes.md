## What's new in 0.16.3

Limoniter 0.16.3 installs packages into a notebook's kernel from the notebook itself, makes installing a Python kernel work on Windows, and tidies two menus.

**Notebooks**
- **Install packages into this kernel** (the Kernel menu): type the names, such as `matplotlib` or `pandas==2.2`, and they're installed into the kernel's own Python (with `%pip install`) or R, with the output shown as it comes and a Stop button.
- **Install x** under a "No module named 'x'" error installs the package it needs (`cv2` is opencv-python, `PIL` is pillow, `sklearn` is scikit-learn and so on), then offers to run the cell again.
- **Install a kernel** now works on Windows, where the commands failed in PowerShell. The kernel is registered so Jupyter finds it too, and the notebook picks it up by itself when it's ready.
- **Interrupt** stops a running cell on Windows, where ipykernel ignores the interrupt message Limoniter sent.

**Menus**
- **File, New as** starts with the three kinds you chose last.
- **Spectrum GRAPH mode** and **UDG pictures** are in Tools, ZX Spectrum and Next, not Format, Tokenised BASIC.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **Windows, portable:** `Limoniter-…-Windows-portable.zip`, with no installer and no administrator needed: unzip it anywhere (a USB stick, say) and run `limoniter.exe`. It keeps its settings and everything else in the `Limoniter data` folder beside it, leaving nothing on the PC, so several versions can sit side by side. It's unsigned too, so Windows warns about it as it does about the installer.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

`SHA256SUMS.txt` lists each download's SHA-256, and `SHA256SUMS.txt.minisig` is its signature, which Limoniter checks before it updates itself.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
