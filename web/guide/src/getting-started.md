description: Install Limoniter on Windows, macOS or Linux, open your first files, find your way around the window, and find any command with F1.

# Getting started

This page takes you from downloading Limoniter to finding your way around it: opening files, working with tabs, and finding any command when you don't know which menu it's in.

## Installing Limoniter

### Windows

1. Download the installer from the [latest release](https://github.com/jolyonralph/limoniter/releases/latest) on GitHub.
2. Run it. The installer isn't signed yet, so Windows SmartScreen may ask whether to run it the first time: choose **More info**, then **Run anyway**.
3. Start Limoniter from the Start menu.

Windows is Limoniter's main platform, and everything works there, including Windows' own spell checker, printing through the Windows print dialog, and offering Limoniter as the editor for chosen file types (see [Settings](settings.html)).

### macOS

1. Download the Mac app from the [latest release](https://github.com/jolyonralph/limoniter/releases/latest). It's for Apple Silicon Macs (M1 and later).
2. Move it to your Applications folder and open it.
3. The app isn't signed by Apple, so the first time macOS won't open it straight away. Go to **System Settings › Privacy & Security** and choose **Open Anyway**. On macOS 14 and earlier, Control-click the app and choose **Open** instead.

On a Mac, shortcuts use Cmd where this guide says Ctrl, Option and an arrow moves by a word, and Cmd+[ and Cmd+] go back and forward. Files open from Finder by double-clicking or with **Open With**.

### Linux

There's no ready-made Linux download yet, so you build Limoniter yourself. It builds, runs and passes the same tests as on Windows.

1. Install Rust, a C compiler, `perl`, `make`, and ALSA's headers (`libasound2-dev` on Debian and Ubuntu, `alsa-lib-devel` on Fedora), which the built-in Spectrum's sound needs.
2. Build it:

```
git clone https://github.com/jolyonralph/limoniter
cd limoniter
cargo build --release
```

3. Run `target/release/limoniter`. To add it to your desktop's menus, use `packaging/linux/limoniter.desktop` from the source.

On Linux, spell checking uses hunspell or aspell if either is installed, printing goes to the default printer through CUPS, and kept passwords go in the system keyring.

## Opening files

To open a file, do any of these:

- Choose **File › Open…** ([[Ctrl+O]]) and pick it.
- Drag it from your file manager onto the Limoniter window.
- Choose it from **File › Open recent**.
- On Windows, open it from Explorer: it joins the Limoniter window that's already open as a new tab.

To start a new file, press [[Ctrl+N]]. **File › New as** starts a file of a particular kind, such as Spectrum BASIC, Z80 assembler, Markdown or Commodore text, with the right character set, line endings and colouring already set.

Limoniter opens almost anything. Text opens as text, in the character set it's in. A file that holds zero bytes opens in hex. A disk image, tape or archive opens in the disk browser, as a folder you can look through (see [The disk and tape browser](disks-and-tapes.html)). A file of any size opens quickly: files of 64 MB and more are read from disk a part at a time, so even a 10 GB file opens straight away.

> Opening and saving give back the same file, byte for byte. Saving never changes bytes you didn't edit, so it's safe to open an old file just to look at it.

To keep a file from being changed by accident, tick **Edit › Read-only**.

## The window

![The command box, finding a command by what it does](command-box "The command box: type what you want to do, and the commands that do it are listed.")

From top to bottom, the window has:

- **The menu bar**: File, Edit, Search, View, Format, Tools, Window and Help. Every command is in a menu, arranged by what it's for. On the right is the **Find a command…** box.
- **The file tabs**, one for each open file.
- **The text** of the file in front, with line numbers and the document map down the right.
- **The status bar**, with the cursor's line and column, the file's character set and its line endings. Click the character set or line endings to change them.

Panels open around the text under **View › Panels**: files on this computer, the open files, the functions in this file, bookmarks, the edit history, the character table and more. **View › Panels › Close every panel** puts them all away again. **View › Output** shows the Output panel, where commands, scripts and the assembler write what they have to say, and **View › Terminal** ([[Ctrl+`]]) opens your own shell in a panel under the text.

**View › Preview** shows a preview beside the text, or instead of it: Markdown, ANSI art, teletext pages, BASIC programs listed as the machine would list them, and pictures in older machines' formats.

To make the text larger or smaller, use **View › Larger text** and **View › Smaller text**. **View › Full screen** fills the screen with Limoniter.

## Finding a command

You don't need to know which menu a command is in.

1. Press [[F1]], or click **Find a command…** at the right of the menu bar.
2. Type what you want to do, such as "sort", "wrap" or "hex".
3. Choose a command with [[↑]] and [[↓]], and press [[Enter]] to do it. [[Esc]] closes the box.

The box finds commands by what they do as well as by their names, and it lists the commands of any add-ons you've turned on too.

## Working with tabs

Each open file has a tab. Click a tab to bring its file to the front, and press [[Ctrl+W]] to close it.

- [[Ctrl+Tab]] goes to the file you used last. Keep Ctrl held and press Tab again to go further back, and let go to stay there. [[Ctrl+Shift+Tab]] goes the other way.
- **Window › Previous tab** and **Window › Next tab** go along the tabs in order.
- **Window › List of open files** shows them all in a panel.
- **View › Split** shows a file in two panes, one above the other or side by side, or opens another tab on the same file.
- **File › New window** opens a second Limoniter window.

[[Alt+Left]] and [[Alt+Right]] go back and forward through the places the cursor has been, across files, as a web browser goes back and forward through pages.

## If something goes wrong

Limoniter copies your unsaved edits to disk as you type. If it stops unexpectedly, or the power goes, it offers them back the next time it starts. If you chose to decide later, **File › Recover unsaved edits…** offers them again.
