description: Change how Limoniter looks and behaves: the Settings window, themes for the window and for documents, autosave and backups, new files, hex mode, and file types on Windows.

# Settings

Limoniter works without any setting up, but most of what it does can be changed to suit you. This page shows you where.

## The Settings window

Choose **Tools › Settings…**. Settings are in six sections down the left:

- **Appearance**: the colour theme, the editor's font and text size, and what's shown in the editor and the window (line numbers, ruler, document map, the cursor's line and column, rainbow brackets, file tabs and the status bar).
- **Editing**: tab size, whether the Tab key puts in spaces, word wrap, keeping the indent on new lines, what Home does, ignoring the Insert key, copying the whole line when nothing is selected, **Copy before paste**, how Spectrum BASIC listings write graphics, the assembler for each processor, language servers, and whether the undo history is kept after a file is closed.
- **Hex mode**: whether files with zero bytes in them open in hex, and how many bytes go on a row.
- **Searching**: whether Find starts with the selected text, and whether Find in Files starts in this file's folder.
- **Files**: checking for changes by other programs, autosave, backups, opening CSV files as tables, a window for each file, the format and kinds of new files, recently opened files, how Markdown opens, and printing.
- **File types**: offering Limoniter to Windows as the editor for the types you choose.

To find a setting, type a word in the **Find a setting** box above the sections.

Many settings are also in the menus, such as **View › Show** for what's shown in the editor, and change there and in Settings alike.

## Themes

Limoniter has two kinds of theme: a **window theme** for the menus, panels and boxes, and a **document theme** for the text and everything drawn with it.

![Four documents, each in a different document theme](themes "Each document can have its own theme, chosen by its ending, language or character set.")

- **View › Theme › Window** chooses the window theme.
- **View › Theme › Documents** chooses the usual document theme.
- **View › Theme › This document only** gives the file in front a theme of its own.

### Choosing themes by rules

Each document's theme can be chosen by rules: by its file ending, its language, its character set, or whether it's in hex or table view. The first rule that fits wins. Out of the box, Commodore, Atari, Spectrum, BBC, teletext and Amiga text open in their machine's colours.

To change the rules, or make your own theme:

1. Choose **View › Theme › Themes and rules…**.
2. Copy a theme and change any colour, or make a whole theme from three colours. You see it on a sample as you go.
3. Add or change the rules for which documents get which theme.

Themes can be saved to a file and opened elsewhere, to share them.

![The Themes box with the Commodore 64 theme and its sample](themes-editor "Copy any theme and change it colour by colour, or make one from three colours.")

## Saving and backups

Two settings under **Files** are off until you turn them on:

- **Save files by themselves** (autosave) saves a changed file once it has had unsaved changes for the time you choose, from 15 seconds to 10 minutes. It leaves a file for you to save when saving would need a question answered: one never saved before, read-only, on a server, changed by another program, too large, or holding characters its character set can't. The status bar says when it's leaving one, and why.
- **Keep a backup when saving** keeps the file as it was beside it, with `.bak` added to its name, the first time it's saved in a session.

Whatever you choose, unsaved edits are copied to disk as you type, and offered back after a crash.

## New files

- **Format for new files** sets the character set, line endings and display new files start with, from the kinds-of-computer presets. It's also under **Format › Encoding › Kind of computer for new files…**.
- **Kinds of new file (File, New as)** changes the list under **File › New as**: add your own kinds, change them, remove them and put them in order. Each is a file ending, a character set, line endings, colouring, and text to start with.

## Language servers

For C, C++, Rust, Python, JavaScript, TypeScript, Go, Java, Lua, shell scripts and others, Limoniter can use a language server you've installed (rust-analyzer, clangd, pylsp and the rest) for completion, hover, Go to definition, Find all uses, Rename and problems marked beside their lines. They're off until you tick **Use them** beside **Language servers**, under Editing. **Which…** shows which servers are installed, and lets you change the command for each language.

## File types on Windows

To make Limoniter the editor for some kinds of file on Windows:

1. Open **Tools › Settings…** and choose **File types**.
2. Tick the types you'd like Limoniter to open.
3. Windows only lets you change a default yourself, so a Windows page opens: choose Limoniter for each type there.

This is only on Windows so far.

## Other things you can change

- **View › Font…** chooses the editor's font, and **View › Larger text** and **Smaller text** its size.
- **View › Always on top** keeps the window above others.
- **Tools › Add-ons › Manage add-ons…** turns add-ons on and off. A new add-on is off until you turn it on.
- **Tools › Run › Run a command…** ([[F5]]) runs a command in the file's folder, and **Tools › Tasks** runs a project's build and test commands.
