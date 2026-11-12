## What's new in 0.3.2

- **Send a BASIC listing straight to the machine.** Open a program's listing, edit it, and Send to device: it goes as the program, tokenised. A NextBASIC listing goes to your Spectrum Next as a `.bas`, a C64 listing as a `.prg`, a Spectrum tape listing as a `.tap`.
- **Autosave** (Settings, Files; off to begin with): a changed file saves itself after a time you choose, from 15 seconds to 10 minutes. Files that would need a question answered, such as one another program has changed, are left for you, and the status bar says why.
- **Keep a backup when saving** (also off to begin with): the first save of a file in a session keeps the file as it was, as `name.bak`.
- **On a Mac,** the app is now sealed, so macOS offers Open Anyway instead of saying it's damaged.

## Also in 0.3.1

Files of 64 MB and more can now be compared, printed, shown as tables, reindented, filtered, and converted to another encoding or line ending in place, with the long work done in the background.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon Macs (M1 and later). Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** build it from the source with `cargo build --release`; see the README.

See [what Limoniter can do](https://limoniter.com/features.html) and [how it compares](https://limoniter.com/compare.html).
