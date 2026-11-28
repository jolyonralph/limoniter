# What's new in Limoniter

Every release since 0.5.0, newest first. The latest five are described in full; older ones in short, each with a link to its full notes.

## 0.18.0

Released 28 November 2026.

Limoniter 0.18.0 makes Limoniter the place where Claude Code and Codex work. Both run in a panel beside your files, and both work with `claude` and `codex` in the terminal. Each edit is a diff to accept or change. Both can work on one project at once, with Limoniter as referee. 0.18.0 also adds tools for writers: rhymes, a thesaurus, metre, and song lyrics for Suno. Colour tags can now be named, a colour picker writes colours the way an Amiga, a Spectrum or a web page expects them, and a Greaseweazle reads and writes real floppy disks.

**AI agents**
- **The AI agents panel** (View, Panels; Tools, New AI agent session): Claude Code or Codex working on the project from a side panel, as in VS Code. Choose the permission mode, the model and the effort, and change them as it works. The transcript shows its answers as Markdown, its thinking and each tool it uses, folded. `@` mentions a file, a message typed while it works waits its turn, and Stop interrupts. Sessions run side by side as tabs, and past ones can be carried on.
- **Each edit opens as a diff** in the editor: Accept, Reject, or change the text first and then Accept. Commands ask **Allow once**, **Allow for this session** or **Refuse**.
- **Codex** with its own permission levels (Read only, Ask before changes, Workspace write, Full access), its own models and reasoning effort, and a tool server from Limoniter that gives it the problems, the open file, the selection and the open tabs.
- **Claude Code in Limoniter's terminal** connects by itself, as it does with VS Code: its proposed edits open as a review tab (Accept with Alt+A, Reject with Alt+R), it sees what you select and the problems the language servers have found, and **Edit, Mention in Claude Code** puts the file and the selected lines in its prompt.
- **In WSL**: on Windows the panel finds Claude Code and Codex in your WSL distributions too, and runs them there on your project, with every path translated both ways.
- **A copy of its own**: a session can work in its own Git worktree, so it can't disturb your work or another agent's. **Review and merge** shows what it changed and merges it back with Limoniter's three-way merge. **Ask the other to review** hands the changes to the other tool to look over.
- **Claude Code and Codex together** on one copy: files and folders can be **claimed** for a session (by you, from the Explorer, or by the agent), and another session's edits there are refused with a message saying who has them. **Builds and tests wait their turn**, one at a time across sessions. A **task board** hands out tasks for the sessions to take.
- **Usage** (Tools, Usage of AI tools): what Claude Code and Codex have used today or this week, by tool and model, counted from the tools' own logs so that sessions in a terminal count too. It also shows the plan's five-hour and weekly limits with their reset times, and the sessions running now with their tokens and how full their context is.
- **Files changing** (View, Panels): the project's files as they change, newest first, each marked **You**, an agent's name, or **Elsewhere**.

**Opening files from a terminal**
- `limoniter notes.txt:42` opens the file in the Limoniter window already open, at that line; `--new-window` gives it a window of its own, and `--wait` waits until it's closed, so Limoniter can be Git's editor (`git config core.editor "limoniter --wait"`).
- In Limoniter's own terminal the command reaches that terminal's window. The Windows installer can add it to PATH for every terminal.
- In WSL, a small `limoniter` script (offered when a WSL terminal opens) runs the Windows Limoniter with the paths turned into Windows' form.

**Writing**
- **Words** (Tools, Words): rhymes for the word at the cursor (Ctrl+F7) and a thesaurus (Shift+F7), both kept in the program, so nothing is downloaded. Syllable counts beside each line, the stress and metre of a passage ("iambic pentameter"), haiku and limerick checks, and repeated words, filler words and adverbs marked.
- **Song lyrics** (File, New as, Song lyrics): section tags such as `[Verse]` and `[Chorus]`, coloured, folded and offered as you type, a style box kept in the file, Suno's length limits shown as you write, and the syllables and rhyme scheme of each section in the margin.

**Colours**
- **Colour tags of your own**: tags with a name, a colour and a key of your own ("To check", "Slow"), kept in your settings or with the project. The six can be renamed, and the tag menu is now in View.
- **A colour picker** (Tools, Ctrl+Alt+K, or More… on the swatch beside a colour) that writes the colour in the file's own form: hex, rgb(), hsl() or oklch() for the web, `$0RGB` and AGA's halves on the Amiga, INK and BRIGHT on the Spectrum, and the palettes of the C64, CPC, Next, Atari ST and others, each machine's nearest colour shown. It has a contrast checker and an eyedropper for anywhere on the screen.

**Floppy disks**
- **Greaseweazle**: read and write real floppy disks through a Greaseweazle USB controller. **File, Open from a floppy disk** reads an Amiga, PC, Atari ST, Commodore 1541, Apple II or BBC disk (or any of the 159 kinds its `gw` program knows, or raw flux) and opens the image in the disk browser, with a progress bar by track and the sectors that couldn't be read. **File, Write to a floppy disk** writes an image back, read back to check. **Retro, Greaseweazle** also cleans the heads, measures the drive's speed and erases a disk, and **Set up Greaseweazle** installs the tools, tests the connection and updates the firmware.

**Retro**
- **Set up the Amiga** (File, Run on): downloads the emulator, finds your Kickstarts (Amiga Forever's too) or uses the free AROS, and sets F9. Amiga programs then run in Limoniter's window, booted straight into them.
- **Watchpoints**: stop when an address is read or written, or when its value changes, rises or falls. **Conditional breakpoints**, with hit counts. Both work on the built-in machines, the Amiga debugger, ZEsarUX and VICE.
- **The trainer** (Run on, Trainer…) finds where a game keeps its lives or score, and freezes them.
- A **debug console port** on each built-in machine shows what a program writes to it in the Output panel.

**And**
- With no project open, the Project panel lists the **recent projects** to open.

## 0.17.1

Released 27 November 2026.

Limoniter 0.17.1 lets you install an emulator for Limoniter's window straight from F9, lets you change what F9 remembers, and offers a new Amiga C project from File, New as.

**Emulators in Limoniter's window**
- **Install from F9**: when a machine's "in Limoniter's window" choice is greyed out because limoniter-retro or its core isn't downloaded, the **Run on machine** box has a button that downloads what's missing in one go ("Install limoniter-retro and the PUAE core").

**F9**
- **Where F9 runs programs**, in Devices and emulators: each machine F9 remembers, with **Change** and **Forget**. Before, undoing "Always run there" meant editing the settings file.

**And**
- **File, New as, Amiga** offers **New Amiga C project…** beside the single files.

## 0.17.0

Released 27 November 2026.

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

## 0.16.4

Released 23 November 2026.

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

## 0.16.3

Released 22 November 2026.

Limoniter 0.16.3 installs packages into a notebook's kernel from the notebook itself, makes installing a Python kernel work on Windows, and tidies two menus.

**Notebooks**
- **Install packages into this kernel** (the Kernel menu): type the names, such as `matplotlib` or `pandas==2.2`, and they're installed into the kernel's own Python (with `%pip install`) or R, with the output shown as it comes and a Stop button.
- **Install x** under a "No module named 'x'" error installs the package it needs (`cv2` is opencv-python, `PIL` is pillow, `sklearn` is scikit-learn and so on), then offers to run the cell again.
- **Install a kernel** now works on Windows, where the commands failed in PowerShell. The kernel is registered so Jupyter finds it too, and the notebook picks it up by itself when it's ready.
- **Interrupt** stops a running cell on Windows, where ipykernel ignores the interrupt message Limoniter sent.

**Menus**
- **File, New as** starts with the three kinds you chose last.
- **Spectrum GRAPH mode** and **UDG pictures** are in Tools, ZX Spectrum and Next, not Format, Tokenised BASIC.

## 0.16.2

Released 22 November 2026.

Limoniter 0.16.2 makes the Explorer's recency slider quick on big projects, opens links again, puts the licences behind a button in the About box, and adds example notebooks.

**Fixes**
- **The recency slider is quick on big projects**: it no longer reads every file under every folder. Each folder's own files are ranked by their dates, and its subfolders by the folder's own date, as the folder is shown.
- **Links open again**: the About box's GitHub link and the tools' website links did nothing when clicked.
- **Project settings**: each tab fills the window, so making it taller shows more, not empty space.

**Small additions**
- **Example notebooks**: File, New as has a Python notebook and an R notebook to try a kernel with. They show printed text, a result, a table, a plot, an error with its traceback and, in Python, `input()` answered in a box.
- **Licence information** in the About box opens the licences of Limoniter and of the work it's built on, in a tab to read.

## 0.16.1

Released 22 November 2026.

Limoniter 0.16.1 gives projects a settings window of their own, reads VS Code's project settings, colours CSV columns and queries their rows, checks the spelling in code, and can put the tabs down the side, with a toolbar of your own buttons.

**Projects**
- **Project settings**: one window, from the gear on the Project panel or File, Project, Project settings, for everything a project keeps:
  - files and folders to hide, or to leave out of search and Go to file;
  - the indent, line endings, encoding and formatter;
  - autosave;
  - the `.env` file, the Python environment and the working folder;
  - which language servers run;
  - file colours and templates;
  - only what you change is written, in the `.limproj` or in `.limoniter/project.json`, so it can go into Git.
- **VS Code's project settings**: a folder's `.vscode` is read as VS Code reads it:
  - `settings.json` gives the project its exclusions, indent, formatters, autosave and the rest;
  - `tasks.json` adds its tasks, and `launch.json` its debug setups;
  - the extensions it recommends are offered;
  - `.code-workspace` files open as projects;
  - Import into Limoniter makes them the project's own.
- **The recency slider** now hides git-ignored files too, and counts a new file as the newest at once.

**Editing**
- **Rainbow CSV**: each column of a CSV or TSV file in its own colour in the text view. **Query the rows** asks for some of them in SQL's words (`SELECT`, `WHERE`, `GROUP BY`, `ORDER BY`, `COUNT`, `SUM`…), and the answer opens in the data viewer, worked out in the background.
- **Spell checking for code**: only comments and strings, and if you like the words in names (`getHTTPResponse` is get, HTTP, Response), with a word list kept per project and the words programmers use already known.
- **Tabs down the side**, grouped by folder, and a **toolbar** under the menus with the buttons you choose.

**Web**
- **ESLint** runs beside the TypeScript server in projects that use it, with its fixes in Quick fix.
- **Debug an npm script** from Tools, Tasks or the Debug link beside each script in `package.json`.
- Autosave's "when another tab or panel is chosen" now notices the Explorer, Search, Output and the terminal too.

## 0.16.0

Released 22 November 2026.

Limoniter 0.16 is a release for modern coding: Jupyter notebooks, Python and R as their users expect them, Node and React, PHP's own tools, a data viewer, a database browser, one Tests panel, `.http` request files and Docker Compose, each built on the language servers, debugger and tasks Limoniter already has.

**Jupyter notebooks**
- **`.ipynb` files open as notebooks**:
  - code cells are edited in place with their language's colours, and Markdown cells are shown as they read;
  - outputs show below each cell: text with terminal colours, PNG, JPEG and SVG pictures, plots, pandas tables as a grid, and errors with their traceback;
  - run a cell, all, above or below, interrupt and restart, all with Jupyter's keys (Shift+Enter, A, B, D D, M, Y);
  - a notebook is saved exactly as Jupyter saves it, and Git compares it cell by cell;
  - File, New as starts a Python or an R notebook.
- **Kernels with nothing else installed**: Limoniter speaks Jupyter's protocol and ZeroMQ itself, finds the kernels Jupyter knows of (Python's ipykernel, R's IRkernel and any other), and offers to install one when there's none.
- **The Variables panel** lists what a notebook's kernel holds, as RStudio's Environment pane does. Double-click a pandas or R data frame to open it in the data viewer.

**Python and R**
- **Environments**: a project's virtual environments (venv, uv, Poetry, conda) are found, and the one in use shows in the status bar, where a click picks another or makes one. F9, the debugger, pytest, the language server and the kernels all use it.
- **A Python and R console** beside the editor. Ctrl+Enter sends the line or the selection, as RStudio does, and plots are kept to step back through.
- **`# %%` cells** in ordinary scripts get Run cell and Run above links, as in VS Code.
- R's languageserver, and R Markdown and Quarto previews with Knit / Render.

**Data and databases**
- **A data viewer** for `.xlsx` workbooks (read by Limoniter itself), JSON lines, query results and data frames:
  - millions of rows, sorted, filtered and summed up column by column;
  - copied as a spreadsheet copies them, or saved as CSV;
  - View, Open in the data viewer shows a CSV file there too.
- **A database browser** for SQLite, PostgreSQL and MySQL/MariaDB, through Limoniter's own clients with TLS:
  - the Database panel shows schemas, tables and columns;
  - double-click a table for its rows;
  - in a `.sql` file, **Ctrl+Enter** runs the statement at the cursor, and an error is marked at its place;
  - passwords are kept only in the system's keychain.

**Node, React and the web**
- npm, pnpm, yarn or bun, as the project's lock file says. **F9** starts a front-end's dev server (Vite, Next and the rest) and opens the page. Node is debugged with js-debug.
- JSX and TSX coloured and laid out properly, with components in the outline.
- **Emmet**: `ul>li.item*3` and Tab, in HTML, JSX, Vue, Svelte, PHP templates and CSS.
- **`.env` files**: values hidden until you look, a warning when one isn't left out of Git, and given to tasks and F9.
- **`.http` request files**, as VS Code's REST Client has them:
  - Send request above each request;
  - the response laid out;
  - variables, environments, values taken from an earlier response, and cookies.
- **Docker Compose**: a Containers panel with each service's state and ports. For each service: up, down, restart, rebuild, logs and a shell.

**PHP, tests and tidying**
- PHP: Intelephense or Phpactor, Composer's commands as tasks, debugging with Xdebug, and php-cs-fixer.
- **One Tests panel** for pytest, Jest, Vitest, PHPUnit, R's testthat, `cargo test` and `go test`: run one, a file's, the failed ones or all, with each failure marked at its line.
- Formatters kept in the project (`node_modules/.bin`, `vendor/bin`, `.venv/bin`) come first, and R is formatted with styler.
- **Outdated packages**: what npm, pip, Composer and R have newer versions of, in one table, updated as tasks.

**Projects and files**
- **A recency slider** at the bottom of a project's Explorer: show only the newest 25% (or any share) of the files, with how many are shown and hidden. The order is unchanged.
- **A project's own file templates**, in File, New as, with blanks filled in, and folders of templates making several files at once.
- **Autosave as VS Code has it**:
  - after a delay (from 1 second), when you change tab, or when the window loses the focus;
  - it can leave alone files with errors;
  - **hot exit**: Quit doesn't ask about unsaved files, and they come back next time.
- File, **Close all** and **Close the others**, also on a tab's right-click menu.

**Windows**
- No more consoles flashing up and taking the focus when Limoniter starts a program.
- Every system dialog now stays in front of Limoniter's window, so a click can't hide one.

## 0.15.0

Released 21 November 2026.

Limoniter 0.15 is a release for retro coders: the whole 680x0 family, How To Code for the ZX Spectrum and a games chapter for the Amiga, the ZX80, the Vectrex and three of the first computers ever built, the Sinclair QL, five more machines for F9, structure templates for the hex view, tape audio, and Limoniter updating itself.

**The 680x0 family, complete**
- Limoniter's own assembler and disassembler now take every 680x0: the **68010, 68020, 68030, 68040, 68060 and CPU32** as well as the 68000 and 68008, the **68881/68882 maths coprocessor** with floating-point constants in every size, and the **68030's and 68851's MMUs**. `MACHINE 68020` (or `MC68030`, `FPU 1`, `OPT P=68020/68881`), or a setting, chooses; an instruction a processor hasn't got is refused, naming the one that has. Everything matches vasm byte for byte, and every opcode the disassembler reads assembles back to the same bytes.
- **Warnings as vasm gives them**: `moveq #143,d7`, an offset that changes sign, an immediate too big for its size, all shown beside their lines without stopping the build.

**How To Code**
- **How To Code: the ZX Spectrum** (Help, and Tools, ZX Spectrum and Next), a second guide by Comrade J and Mnemonic of Share and Enjoy, CC0: 14 chapters on the 48K, the 128K and the Next, from contended memory and the floating bus to border effects timed to the T-state, masked sprites, beeper and AY music, paging, and the Next's copper, Layer 2 and sprites, with 19 examples (and Catch, a small whole game), each run on Limoniter's built-in Spectrum, in Fuse and in ZEsarUX.
- **Chapter 17 of How To Code v8, Writing games**, built round Gem Cave, a small whole Amiga game: eight-way scrolling over a tile map, sprites and bobs, collisions, the CD32 pad, sound effects over music, and staying friendly to WHDLoad, each example run on Kickstart 1.3 and 3.1, a 68020, a 68040, an A1200 and a CD32.
- Both guides read like a book in Limoniter: the contents page links every chapter, a link turns the page in the same tab, a bar at the top has Back, Forward, Contents and Close (Alt+Left and Alt+Right work too), and the examples' names open them.

**New machines**
- **A built-in ZX80**, its picture going blank while a program runs as the real one's did, with `.o` files loaded, saved, listed and tokenised, and File, New as, ZX80 BASIC.
- **A built-in Vectrex**: its 6809, VIA and AY, and the vector screen drawn as glowing lines that fade like phosphor, with breakpoints and stepping; you choose its BIOS once.
- **Early computing** (Tools): the **Manchester Baby** (1948), with Kilburn's first program; **EDSAC** (1949), its real Initial Orders reading your source as paper tape, with Wilkes's table of squares; and the **PDP-8** (1965), with its front panel and Teletype, PAL source and paper tapes.
- **Sinclair QL programming**: QDOS programs with their headers, microdrive cartridges made and browsed, and F9 into sQLux, Q-emuLator or MAME.
- **F9 on more machines**: the **Amstrad CPC**, **Apple II**, **Atari 8-bit computers**, **Atari 7800** and **SNES**, with their emulators found or installed for you.
- **Control hints** on every built-in machine, with keys you can change and a Kempston joystick on the Spectrum.

**Tools**
- **Structure templates**: a Structure panel beside the hex view names a file's fields, colours their bytes and follows the cursor both ways; 16 formats built in (Amiga hunks, IFF, ADF, TAP, TZX, D64, PNG, ZIP, ELF and more), and templates of your own in a simple language.
- **Tape audio**: Spectrum, CPC, ZX80 and ZX81, Commodore, BBC and MSX tapes made into the sound of the tape (WAV), to load on a real machine.
- **PHP tools** (Tools, PHP): short tags made long, the "headers already sent" fixer, `array()` to `[]`, a PHP 7/8 upgrade check, old files made UTF-8, `php -l` on save, and F9 serving a page.
- **TRSE** (Turbo Rascal) and **RGC BASIC**, coloured and run with F9.
- **Open a ZIP, or a web address, as a project.**
- The **68000 disassembler** reads real demos properly (since 0.14.4), and Limoniter's Spectrum now keeps the Z80's H'L' right in snapshots it hands to other emulators.

**Limoniter itself**
- **Limoniter updates itself**: Help, Check for a newer Limoniter offers Install now, checking every download against the release's signed list first, and it never restarts with unsaved work. The previous version is kept, to go back to. (Copies before 0.15 need one last download by hand.)
- **A portable Windows download**: unzip `Limoniter-…-Windows-portable.zip` anywhere, and it keeps everything in a folder beside itself.
- **Menus and boxes follow the window**: a menu that had to scroll in a small window now grows again when the window does.
- **Settings** has the shortcut everyone expects, **Ctrl+,** (Cmd+, on a Mac), and **File, Quit** is there with **Ctrl+Q** (Cmd+Q). The About box says what Limoniter is today, and its scroller bends each letter round the wave, as demos did, apostrophes and all.

## 0.14.4

Released 21 November 2026.

Limoniter 0.14.4 fixes the 68000 disassembler on real Amiga demos, and gives a project's Explorer filters, colours and a sense of what's being worked on.

**The 68000 disassembler, put right on a real demo**
- A demo such as SAE's MULater now comes back from its source exactly: the same program, with every relocation, through Limoniter's own assembler and through plain vasm. Before, `/*` inside one of a demo's tables was taken as the start of a comment in Limoniter's assembler, and the check that followed turned correct instructions into bytes and lost relocations, so the rebuilt program could crash.
- Pointers to bitplanes, buffers and tables are no longer followed as code, so there are no more pages of `ori.b #0,d0`; tables of numbers are no longer shown as text.
- Copper lists come out as `dc.w` pairs with the custom chips' register names, their WAITs and SKIPs explained; interrupt and exception vectors are named.
- A big disassembly is much faster: MULater's takes 0.4 seconds instead of 18.

**Projects**
- **Hide what you don't need:** right-click a folder in the Explorer for Hide this folder, or a file for Hide .o files; hidden things also leave Go to file and searching, and come back from the project's settings or with Show hidden items. When you open a folder with build or dependency folders (`node_modules`, `target`, `build`…), Limoniter offers to hide them.
- **Recently edited files stand out:** in the Explorer's A–Z list, a file edited lately has its name in bold with a dot whose colour says how lately (today, this week, this month), and folders holding them are marked too. It goes by Limoniter's saves and Git's history, so a fresh clone doesn't light up everything; choose the time span from the Explorer's menu.
- **Colour this file:** right-click a tab to give a file a tint (blue, green, amber, red, rose, violet) or a theme of its own, so several files of the same kind are told apart at a glance; rules can colour a folder's files or a pattern (`admin/`, `config/production.php`). Kept with the project.
- **Open recent, with Projects and Files apart:** File › Open recent and the start page show recent projects first, and a Files tab with files grouped by the project they belong to; a project's file opens its project too.

## 0.14.3

Released 21 November 2026.

Limoniter 0.14.3 fixes boxes and menus that didn't fit the window, takes Limoniter's own assembler beyond the 68000, and gives the About box a demo screen.

- **Boxes and menus fit any screen.** A box taller or wider than the window used to be cut off with no way to reach the rest; now it stays on the screen and scrolls, with its OK and Cancel buttons kept in sight below. Menus longer than the window, such as File › New as and its sub-menus, scroll too, and a menu from the menu bar opens below the bar. This applies to every box and menu in Limoniter, and new ones get it automatically.
- **The 68008, 68010, 68020 and 68030 in Limoniter's own assembler and disassembler.** `MACHINE 68020` (or `MC68020`, `OPT P=68020`) in the source, or a setting, chooses the processor. The 68020/030's addressing modes (scaled indexes, memory indirect), bit fields, CAS, CHK2, 64-bit multiply and divide, PACK and the rest assemble to exactly vasm's bytes, and the disassembler reads them back for an A1200's or A4000's programs. An instruction a processor hasn't got is refused, naming the one that has it. The maths chips, the MMU and the 68040/060's own instructions come next.
- **Cycle counts beside 68000 code**, as for the 6502, Z80 and 6809, and the Sinclair QL's 68008's counts.
- **Four 68000 mistakes put right:** `bra.l`, `tst.w a0` and `cmp #n,x(pc)` are 68020 instructions and are now refused for a 68000 (as vasm refuses them) instead of being quietly assembled as something else; `move ccr,d0` now gives the right bytes; and a PC-relative offset out of reach is refused.
- **The About box** opens with an 80s demo screen: the Limoniter logo over bouncing copper bars, and a sine scroller saying what Limoniter is.
- A test that couldn't run on Windows (it needs a Unix shell) no longer stops Windows' checks.

## 0.14.2

Released 21 November 2026.

Limoniter 0.14.2 is for Amiga programmers above all: new C projects with their makefiles kept up to date, locale catalogs, Workbench icons, the AutoDocs in a window, AmigaShell scripts, GNU as, and Shell programs run on your own computer with vamos. It also installs Boriel's ZX Basic for you, puts File, New as into sub-menus with the modern languages, and brings everything from 0.14.1, which was never published.

**Amiga C**
- **New Amiga C project** (Tools, Amiga, or Project) makes a folder from a template: a Shell or Workbench program for AmigaOS 1.3 or 3, a ReAction program for 3.2, an MUI 5 program, or a multilingual program with its catalog. It comes with makefiles for amiga-gcc, vbcc and SAS/C, a Workbench icon and build tasks.
- **Makefiles kept up to date:** as you add, rename and delete C files, Limoniter rewrites only its own part of each makefile, choosing the maths library when you use floating point. F9 builds the way the makefiles do.
- **Import an Amiga C project** sorts a folder of sources, headers, guides, icons and SAS/C's files, and adds makefiles only where there are none.
- **Locale catalogs:** FlexCat's `.cd` and `.ct` files are coloured and checked against each other (missing strings, lengths, `%` commands, Latin 1), and Limoniter writes the `.catalog` files and the strings' header itself, or with FlexCat.

**Workbench icons and vamos**
- **`.info` files are shown as their pictures**, normal and selected, whichever kind they are: the original icons, NewIcons, GlowIcons and OS 4's PNG icons. **Workbench icon…** changes the kind, default tool, stack, position and tool types, and an icon you don't change is saved byte for byte.
- **An icon for every program F9 builds**, if you want one, from Limoniter's own or one you choose.
- **vamos** (from amitools) runs AmigaShell programs and scripts on your own computer, with their output in the Output panel. Limoniter offers to install it, and SAS/C can build through it.

**Reading and writing Amiga code**
- **AutoDoc reader** (Tools, Amiga): the NDK's autodocs, and MUI's if you point to them, in a window beside your source. Right-click a function to jump to its explanation.
- **AmigaShell scripts are coloured** (the Startup-Sequence, `.key` and `.bra` lines, IF, SKIP and LAB), with their labels in the outline.
- **GNU as's 68000 dialect** (m68k-amigaos-as) is coloured and used to build. Source written for one assembler is never handed to another, and a project can choose its own.

**Also new**
- **Boriel's ZX Basic installed for you:** when zxbc isn't found, F9 and the Compilers box offer to install it: the ready-made build from boriel.com on Windows and Linux, or a private Python environment elsewhere.
- **File, New as is in sub-menus** by machine, with **Modern languages**: an HTML page, CSS, JavaScript, TypeScript, JSON, Python, C, C++, Rust, Go, Java, C#, PHP, Ruby, Lua, shell, PowerShell and batch scripts, SQL, YAML, TOML and XML.
- **Plain HTML is recognised as HTML** when a file has no name to go by, rather than as Svelte.

**From 0.14.1**
*Emulators*
- **Get an emulator** now installs MAME on Windows, from its GitHub releases, along with Hatari, XRoar, WinUAE, BlastEm, Fuse, BGB, Gearsystem, Gearcoleco and TIC-80. On Linux it adds Mesen, TIC-80 and SameBoy, and on a Mac BlastEm, Gearsystem and FCEUX. The Mac commands for Stella, mGBA and ZEsarUX, which no longer worked, are put right.
- **Emulators you unpack yourself are found:** in a folder at the top of a drive (`C:\MAME`), and as a program straight inside Downloads or the Desktop, as MAME's own unpacker leaves it.
- MAME's link now goes to its GitHub releases, as its website isn't answering.

*For 6809 and Dragon programmers*
- **Cycle counts beside 6809 and 6309 code**, as for the 6502 and Z80: every opcode and indexed mode, from Motorola's and Hitachi's figures. The 6309's counts are given for its native mode or its 6809 mode, as the source sets with `PRAGMA 6309` or `LDMD`. They show in the status bar and on hover, and selected lines are added up.
- **F9 to a Dragon DOS disk:** with `; run from: disk`, or the choice in XRoar's settings, the program goes on a disk made for the run, and XRoar starts with it in drive 1 and `RUN"NAME.BIN"` typed. A `.vdk` disk you open runs with F9 too, and files on a Dragon DOS disk can be renamed.

*Also*
- **Debug settings kept:** the program, arguments, folder and debugger chosen for each file are kept for next time. A project's files keep them with the project, so they move with it; other files keep them in Limoniter's settings.
- `limoniter --version` (or `-V`) prints the version with no window. An option Limoniter doesn't know is now refused with the usage, rather than opened as a file.
- F9 says "A program was made" for an Amiga program or another assembler's program, instead of calling it a tape. Its problem count is translated too.

## 0.14.0

Released 20 November 2026.

Limoniter 0.14 is a workshop for the older machines as much as an editor. How To Code v8, the Amiga demo coders' guide, is rewritten, tested and built in. An Atari 2600, a ZX81 and a Jupiter Ace are built in beside the Spectrum. You can now build for the Game Boy, the NES, Sega's consoles and the ColecoVision, compile Boriel's ZX Basic and C for the retro machines, and debug Amiga code in Copperline, stepping backwards as well as forwards. Crunched Amiga files are unpacked. For everyday work there's a start page, an optional ribbon, Mermaid diagrams and PDFs from Markdown, SFTP and FTP projects, and VS Code extensions' themes and grammars.

**How To Code v8**
- **The Amiga demo coders' guide of 1993, rewritten** for today's tools and released as CC0. It has 18 chapters: the startup, interrupts, the copper, the blitter, the display, AGA, chunky to planar (processor, blitter and Akiko), input, sound, 3D vectors, the processors from the 68000 to the 68060, loading and memory, debugging, releasing and further reading. Open it from **Help** or **Tools, Amiga**.
- **Every fact checked** against the Hardware Reference Manual and the NDK 3.2. Its 31 example programs were run on emulated A500s (Kickstart 1.3 and 3.1), faster processors, A1200s and a CD32, both ways, each giving the machine back. Where v7 was wrong, it says so.
- **One startup for every demo**: it takes the machine, or keeps the system running so files can load, chosen by one setting. It puts back everything it changed, the audio filter and AGA's BPLCON4 included, and can check for AGA first.
- **File, New as, Amiga demo** starts from the guide's startup, all in one file. **Copy How To Code's examples to a folder** puts them in a project of your own.
- `limoniter --assemble SOURCE OUTPUT [NAME=VALUE…]` builds an Amiga program from the command line, as F9 does.
- The guide is on limoniter.com too, and the Markdown preview now follows links to files beside a page and shows their PNG and SVG pictures.

**Built-in machines**
- **An Atari 2600**, beside Stella and MAME: a cycle-exact 6507 with every undocumented opcode, the TIA's picture and sound, the RIOT, and bank switching from 2K to F4, FA, E0, E7 and 3F. Debugging works at the source line, and its frames match MAME's pixel for pixel.
- **A ZX81**, its picture made by the Z80 as on the real one, in SLOW and FAST modes. It runs `.p` files, BASIC tokenised from text, and machine code in a REM.
- **A Jupiter Ace**, with redefinable characters, tapes and Forth typed in.
- The ZX81's and Ace's ROMs aren't Limoniter's to give away: choose your own once, and it's checked by its CRC.

**New machines to build for**
- **Game Boy**: the SM83 in RGBDS's syntax, cartridges made as rgbfix makes them, and F9 in SameBoy, mGBA, Gambatte, BGB, Mesen or MAME.
- **NES**: iNES and NES 2.0 files, run in Mesen, FCEUX, Nestopia or MAME. The 65C02, the HuC6280 (PC Engine) and the SNES's SPC700 are assembled and disassembled too.
- **Sega Mega Drive / Genesis**: its ROM header and checksum fixed, run in BlastEm or MAME. The starter has the Z80 play a note.
- **Sega Master System, Game Gear and SG-1000, and the ColecoVision**: their cartridge headers and checksums written, run in Emulicious, Gearsystem, Gearcoleco or MAME.
- **PICO-8 and TIC-80 carts**: their sprite sheet, map and sounds shown beside the code, and F9 runs them.
- Each has a starter under **File, New as**.

**Compilers**
- **Boriel's ZX Basic**: coloured, its SUBs and FUNCTIONs in the outline, completion and hover, and F9 compiling to a tape and running it.
- **C for the older machines**: cc65, llvm-mos, z88dk, SDCC, SGDK, vbcc and amiga-gcc found, with starters. Their errors are marked on their lines, and F9 compiles and runs the result. SAS/C sources are built with amiga-gcc or vbcc through a compatibility header.

**Amiga**
- **Copperline**, the cycle-driven Amiga emulator, joins FS-UAE and WinUAE. F9 boots the program straight away. With copperline-ctl, it's debugged at your source lines, with **Step back** (Shift+F10), memory, a CPU profile by line, and its warnings about chip misuse and self-modifying code.
- **Crunched files unpacked**: PowerPacker, Imploder, StoneCracker, CrunchMania and ByteKiller, as data or as programs. Programs are rebuilt with their hunks and relocations. Disassemble shows the real code, and **Save the decrunched program** writes it out.
- The 68000 assembler now refuses the 68020's forms (`MULU.L`, `TST` of a PC-relative address and the like) instead of quietly making different instructions.

**Tools for retro work**
- **Tile map and level editor**: characters, tiles and maps for the C64, the Spectrum and 1 to 8 bits a pixel. It reads and writes CharPad projects (versions 5 to 9), and writes maps out as source.
- **Raster and copper designer**: bars and splits dragged on a picture of the screen, made into an Amiga copper list, C64 raster interrupts or Atari ST Timer B code.
- **Unit tests for assembly**: Z80 and 6502 routines run with registers and memory set, checked, and their cycles counted.
- **Instruction reference** and **call graph** panels.
- **Serial terminal**, for serial ports and the Ultimate 64, with XMODEM and YMODEM. **EPROM programming** with TL866 and T48 programmers, through minipro.
- **Patches, checksums and entropy**: IPS, BPS and UPS patches made and applied; CRCs and sums, with ROM header checksums put right; a file's entropy shown along it.
- Z80 lines show their T-states, and microcontrollers can be flashed with PlatformIO, avrdude or ipecmd.

**Everyday work**
- **A start page** when every tab is closed: Start, Recent, Learn and a tip.
- **A ribbon**, switched on beside Find a command, made from the very same commands as the menus.
- **VS Code extensions**: their themes, grammars, snippets, file icons and language settings, installed from a .vsix or from Open VSX.
- **Markdown**: Mermaid diagrams drawn in the preview, the preview saved as a PDF, a table of contents kept up to date, lists carried on, tables lined up, and markdownlint's problems marked.
- **Remote work**: SFTP and FTP folders opened as projects, kept in step with the server. FTP and FTPS files are opened and saved directly. `sftp://` addresses open from anywhere, and hosts come from `~/.ssh/config`.
- **Writing code**:
  - indentation guides, problems shown at the end of their line, colours shown under where they're written, and TODO and FIXME coloured with a panel listing them;
  - comments hidden on request, and Find that skips comments and strings;
  - HTML tags closed and renamed in pairs, file paths finished as you type them, and doc comments written from a function's signature;
  - external formatters run on save, and a regular expression tester.
- **Code as a picture**, a **live server** for web pages, **file icons**, the **time spent** on each project (only if switched on), and SVG drawn in the preview.
- **Your own layout**: the window's own font, the status bar's items, panels that hide themselves, and a note of the keys for commands you choose from menus.
- **Colour as** is now a box you can search, by kind, by machine or A to Z. Amiga E is coloured.
- Every new piece of text is in German, Spanish, French and Italian.

## 0.13.0

Released 18 November 2026.

Limoniter is now a full IDE for a folder of code: open the folder and work across all of it, with the Explorer, Go to file and symbol, a Search panel, split editors, terminals in tabs, more from language servers, and debugging for Python, C, C++ and Rust. For retro work there are x86 and ARM assemblers, the Hitachi 6309, Vectrex cartridges, Dragon DOS disks and Amiga boot disks.

**Working on a folder of code**
- **Open folder** (File, Open folder): any folder becomes a project, with an **Explorer** like VS Code's. Files are created, renamed, moved by dragging and deleted in place, and Git's status shows beside each one. Every file is indexed in the background, keeping to `.gitignore`.
- **Go to file** (Ctrl+E) finds any file from a few letters of its name. **Go to symbol** finds a function or label in the folder (Ctrl+T), or in this file (Ctrl+Shift+O).
- **Search panel** (Ctrl+Shift+F): results as you type, as a tree of files and lines. With text to put in its place, each match shows the change before it's made. Replace in one file or all, after a question.
- **Split editor:** another file beside the one in front (Ctrl+\, or Ctrl+Enter in Go to file). Clicking either pane brings it to the front.
- **Breadcrumbs** above the text show the folders and the function you're in, each a menu. **Sticky scroll** keeps the function you've scrolled into at the top.
- **Peek definition** (Alt+F12) shows a definition without leaving your place. F12 now finds definitions by name across the folder too.
- **Terminals in tabs:** as many as you like, kept running when the panel's hidden.
- **.editorconfig** files are followed: indentation, line endings, trailing spaces and the final newline.

**Language servers**
- **Quick fixes** (Ctrl+.), **Format document** (Shift+Alt+F) and format on save, **parameter hints** as you type a call, and **inline type hints**.
- **Offered when you need them:** a language server that's installed is offered for its files. One that isn't installed is installed in a terminal, then started.

**Debugging** (Tools, Run, Debug, Ctrl+F5)
- **Python** with debugpy, and **C, C++ and Rust** with gdb or lldb-dap, through the Debug Adapter Protocol.
- **Breakpoints** beside the line numbers, and the stopped line marked. The Debug panel has the call stack, variables to open level by level, and a console that works out expressions. F5, F10, F11 and Shift+F11 step as in VS Code.

**Retro machines**
- **x86 assembler** in NASM's syntax, for the 8086 to the Pentium and later. `.COM` programs and boot sectors run in DOSBox or QEMU.
- **ARM:** an assembler and disassembler for the Archimedes's ARM2 and ARM3, with RISC OS's SWI names. Later ARM code (ARMv7, Thumb and AArch64, Raspberry Pi kernels included) is disassembled too.
- **Hitachi 6309:** its registers and instructions in the 6809 assembler, as lwasm takes them (PRAGMA 6809 turns them off), and in the disassembler.
- **Vectrex cartridges:** a starter with the cartridge header and the BIOS's routine names. F9 runs it in MAME or ParaJVE. Disassembled cartridges name the BIOS calls.
- **Dragon 32 and 64:** `; machine: dragon32` makes F9 build a Dragon DOS binary and run it in XRoar. The Dragon DOS disk window (Tools, Dragon and Vectrex) makes, reads and changes disks.
- **Amiga boot disks with no file system** (Tools, Amiga): code assembled to chosen sectors, a map of the disk, and a boot block that loads them, checked for size.

**Also**
- **Several clipboards** (Ctrl+0 to Ctrl+9 choose one).
- **Music saved as WAV**, and a module's samples as 8SVX.
- The **DEC Technical** character set.
- The ripper finds **15-sample Soundtracker modules** by their instruments' st-NN: names.
- **Translations completed:** about 1,800 more pieces of text in each language.
- A **What's new** page on limoniter.com.

## 0.12.0

Released 17 November 2026.

Limoniter now speaks French, German, Italian and Spanish. It also makes sound effects for four sound chips, finds the music and graphics inside old games and disks, and shows where a program sits in memory after every build. There are new graphics editors for the Spectrum, the Next and the Atari 2600.

**Languages**
- **The whole window in French, German, Italian or Spanish** (Settings, Appearance, Language): the menus, settings, buttons, messages, and the comments in the source Limoniter writes for you.
- **Each language is listed under its own name** (Deutsch, Español, Français, Italiano), with a globe beside the setting, so you can always find your way back.

**Sound effects** (Tools, Sound effects)
- **Effects for four sound chips:** the **AY** (ZX Spectrum 128, Amstrad CPC, MSX), the C64's **SID**, the Atari 2600's **TIA** and the Spectrum's **beeper**.
- **Draw the notes and the volume** on a graph, type each frame's values, or start from a preset (pickup, laser, explosion, power-up, hit, jump, blip, siren) and change its shape. Random and Vary make new ones.
- **Each change plays straight away.** The AY uses the built-in Spectrum's own emulation, and the beeper runs its player on a Z80, so what you hear is what the player does.
- **AYFX banks** from AYFX Editor open, and are saved the way it saves them.
- **Out come the effects with a player to call** once a frame. **Program to run** opens a program that plays each one in turn, for F9 on the built-in Spectrum, VICE, Stella or MAME.

**Ripper** (Tools, Ripper)
- **Music found in any file:** a game, a demo, a snapshot, or a disk, tape or archive image, including every file inside it and a DMS disk's raw tracks.
  - Tracker modules (ProTracker and its kin, S3M, XM, IT), SID, SNDH (Pack-Ice packed too), YM, PT3 and AY files.
  - Each is measured from its own header. Open it in a tab to hear it, or save one or all of them.
- **Graphics found in any bytes:** see them as characters, C64 multicolour and sprites, NES and Game Boy tiles, Amiga and Atari ST bitplanes, a Spectrum screen, or the Next's 8-bit colours, at any offset and width. Save as a PNG, copy as source, or open in the right editor.

**Building programs**
- **A memory map after every build** for the Spectrum, C64, BBC Micro, Color Computer and Atari 2600. A bar over the Output panel shows the program among the machine's ROM, screen, system areas and I/O, and how much memory is still free. It warns when code lands somewhere it can't go or shouldn't, such as ROM, the C64's I/O, the screen or contended memory.

**Graphics editors**
- **Spectrum screen designer:** lines, boxes, ellipses, fill, text in the ROM's font, selections, scrolling, an attribute lock, redo and zoom.
- **Picture converter** for the Spectrum, Timex hi-res, Next, CPC, MSX and BBC Micro.
- **Spectrum Next Layer 2 painter:** every Layer 2 mode and LoRes, with palette editing. The Next's tilemap can be made from a picture, and sprites cut from a sheet, each with a `.nex` that shows them.
- **Atari 2600 sprites and playfield:** sprite frames with a colour on each line, playfields made from pictures, out as DASM tables and programs that show them.

**Also**
- **AMOS and STOS programs** are listed, and tokenised back from a listing.
- **Column mode with Word wrap on** now offers to turn Word wrap off for you.

## 0.11.0

Released 16 November 2026.

Music and sound play in the preview, there's a 68000 disassembler, and the Atari 2600 is supported from end to end: disassemble a cartridge, write your own, and run it in Stella or MAME.

**Music and sound**
- **Tracker modules play**, with Limoniter's own player:
  - ProTracker `.mod` files from the Amiga
  - the PC's Scream Tracker 3 (`.s3m`), FastTracker 2 (`.xm`) and Impulse Tracker (`.it`) modules
  - each with its own tracker's effects
- **The player** has Play, Pause, Stop, Loop and a slider to move through the song. The pattern scrolls past as it plays, and clicking a channel's number switches it off or on. The music keeps playing while you work on other files.
- **Sound files play** too: MP3, WAV, FLAC, Ogg Vorbis and AIFF, and the Amiga's 8SVX samples, Creative VOC files and Sun and NeXT `.au` files. Their tags (title, artist, album) are shown beside them.

**The 68000**
- **A 68000 disassembler** (Tools, Disassemble) works out what a file is:
  - Amiga programs come back as sections, with their relocations as labels, so they can be changed and rebuilt
  - Atari ST programs, with their symbols
  - Amiga boot blocks and Atari ST boot sectors
  - Mega Drive cartridges, followed from their vectors
  - Kickstart ROMs
- **What the code does is named beside it:** the Amiga's library calls (`jsr -552(a6) ; exec OpenLibrary`) and custom chip registers, and the ST's hardware and GEMDOS, BIOS and XBIOS calls.
- **The source always assembles back** to exactly the same bytes, with Limoniter's own assembler or vasm.

**The Atari 2600**
- **Cartridges** (`.a26`) are recognised with their bank switching worked out: F8, F6, F4 and the Superchip's, FA, E0, E7, FE, 3F and DPC.
- **Disassembled into DASM source** that rebuilds the cartridge:
  - code followed from bank to bank
  - the TIA's and RIOT's registers named
  - each instruction's cycles beside it
  - code that runs past a scanline's 76 cycles pointed out
  - graphics drawn as pixels
- **DASM source** is made into a cartridge, and **F9** runs it in **Stella** or **MAME**.
- **File, New as, Atari 2600** starts a whole frame that runs straight away.
- **Get an emulator** now installs Stella and MAME.

**Also**
- **The assembler** reads DASM's `processor`, `seg.u`, `rorg` and `.w`, and the 68000's MOVEP.
- **Amiga programs** with debug hunks or Kickstart 2.0's short relocation tables are read.
- **The website's home page** shows what Limoniter does for writing new software for older machines, and its figures are up to date.

## 0.10.0

Released 16 November 2026.

Lots of everyday editing improvements, previews of web pages, pictures and old documents, a value inspector for hex, two new disassemblers, and support for the Cambridge Z88 and the Amstrad NC100, NC150 and NC200.

**Editing**
- **Surround with** (Edit): puts the selection in brackets, quotes, a tag or a snippet, and leaves it selected.
- **Bookmarks** (Search): go to any bookmark from a list; copy, cut or delete the bookmarked lines, or delete every other line; swap bookmarked and not; clear them all.
- **Tabs and spaces** (Format): whether Tab types spaces, the tab size, and converting tabs and spaces, including in the indent only.
- **Convert** (Format): Base64 encode and decode, and JSON to XML and back.
- **Column markers** at chosen columns (such as 72 or 80), and **line spacing**.
- **Your own highlighting grammars:** put Sublime Text `.sublime-syntax` files in the grammars folder (Settings, Editing).

**Files and windows**
- **Window, Layouts** saves which panels are open under a name, so you can switch between them. **Window, Gather windows** brings every box back onto the screen.
- **Move to the Recycle Bin** (File, This file), or to the Trash on Mac and Linux.
- **Changes made by other programs** can be checked for every two seconds. Files with no unsaved changes can be reloaded without asking, staying at the end to follow a growing log.
- **File properties** shows the file's owner, the program it opens with, and its lines of code.
- **Save and restore settings** (Tools) puts all of Limoniter's settings in one file, to keep or to move to another computer.
- **Check for a newer Limoniter** (Help) asks GitHub. It also runs once a day by itself, which can be turned off.

**Previews and conversions**
- **Web pages** (`.html`) are laid out in the preview, without their scripts or styles.
- **PNG and JPEG** open as pictures.
- **Old word processors' documents** opened by themselves are shown converted: WordStar, WordPerfect, Protext, 1st Word, Signum!, Final Writer, Wordworth, ProWrite and others. **Format, Save the document as** saves them as text, Markdown, RTF or CSV.

**Hex and character sets**
- **The value inspector** (View, Hex) reads the bytes at the cursor as numbers:
  - integers and IEEE floats, in either byte order
  - IBM hexadecimal floats
  - VAX F, D, G and H floats
  - packed decimal
  - RADIX-50
- **New character sets:**
  - the national versions of ASCII (ISO 646)
  - DEC Special Graphics
  - ASCII with the top bit set, as Prime computers kept text
  - the Amstrad PCW's text and screen sets, the Cambridge Z88's, and the Amstrad NC's

**Machine code**
- **An x86 disassembler** covers everything from the 8086 to AVX-512. It reads DOS `.COM` and `.EXE` files, Windows programs and DLLs, and Linux programs, and follows their code from where it starts.
- **An ICL 2900 disassembler** lists the code in SFL, from ICL's *2900 Order Code*.
- **6502 source** runs on a BBC Micro when it calls the MOS, and on a C64 when it uses the KERNAL. When the code doesn't say, F9 asks, or a `; machine:` line in the source can say.

**Cambridge Z88 and Amstrad NC100, NC150 and NC200**
- **BBC BASIC (Z80)** programs are listed, and tokenised from a listing.
- **Z88 card images** (`.epr`) open in the browser, and **PipeDream** documents and spreadsheets are converted.
- **The Z80 disassembler** names the Z88's operating system calls when asked.
- **NC memory card images** (`.card`) in ZCN's format open in the browser, and program cards are recognised.

**Also**
- **Get an emulator** (File, Send to device, Manage devices) installs any emulator Limoniter runs programs in:
  - on Windows, it downloads and installs ZEsarUX, b2, openMSX, FS-UAE and VICE itself
  - on Linux and Mac, it runs apt or Homebrew in the Terminal panel
  - otherwise, it opens the emulator's download page
- **The table generator and the character and sprite editor** write their data in the form you choose:
  - assemblers: 68000 `dc` lines, Z80 `db`, ACME or 64tass for the 6502, 6809 `fcb`, NASM
  - BASIC DATA lines
  - arrays for C, PHP, Python, JavaScript or Rust
  - Spectrum BASIC that sets the UDGs, for characters
- **Apple II Integer BASIC** programs are listed.
- **A file formats page** on the website lists every format Limoniter reads or writes.

**Fixed**
- The Amiga, Atari ST and C64 hardware guides' lists of registers ran across the window on top of each other.
- Assembling for a machine that runs from a disk said a tape was made.

## 0.9.1

Released 15 November 2026.

The Mac app now runs on Intel Macs as well as Apple Silicon ones: one app for both, in the same download.

## 0.9.0

Released 15 November 2026.

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

## 0.8.0

Released 14 November 2026.

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

## 0.7.0

Released 14 November 2026.

Limoniter now writes to the disks and tapes it reads, builds sjasmplus projects as sjasmplus does, runs OpticOS's BASIC, and reads many more machines' media, floppy flux images among them.

**Changing disk and tape images**
- **Add, delete and rename files** (or drop them on the tab) on Spectrum TAP/TZX tapes and +3 disks, Commodore D64/D71/D81, PC and Atari ST FAT disks, Amiga ADF, Atari 8-bit DOS 2 and Color Computer RS-DOS images; make folders, reorder a tape. Each change is saved at once and can be undone.
- **File › New disk or tape image**: blank ones, formatted as their machine formats them.
- Every writer is checked against its machine's own tools (c1541, mtools and fsck, amitools, atrcopy, ToolShed, cpmtools, fuse-utils).

**For the Spectrum Next**
- **sjasmplus projects build as sjasmplus builds them**: devices and paged memory (MMU, SLOT, PAGE), SAVENEX with all its commands, SAVESNA, SAVEBIN, SAVETAP and the rest, written beside the source; MODULE, STRUCT, DEFINE, IFUSED, RELOCATE, fake instructions and more. 52 real projects from GitHub give the same bytes as sjasmplus.
- **.nex files open** as their loading screen, palette and banks; Next Layer 2 and LoRes screens are pictures.
- **Dot commands** build, and go on a NextZXOS SD card image from the browser.

**OpticOS**
- **OpticBASIC**, the BASIC of the OpticOS replacement ROM, lists and tokenises exactly as the ROM does, and **F9 runs it** on the built-in Spectrum with the OpticOS ROM (included, MIT licence).

**More media read**
- **Floppy flux images** from a Greaseweazle, SuperCard Pro or KryoFlux (`.scp`, `.hfe`, KryoFlux streams): PC, Atari ST, Amiga, C64, Apple II and more, decoded to their files.
- **Mac**: HFS+ disks and `.dmg` images; MacBinary, BinHex and StuffIt archives.
- **Apple II WOZ** images; **Atari 8-bit** XEX programs and pictures.
- **TRS-80** BASIC and tapes, **ZX81** programs, **MSX** tapes, **SAM Coupé** disks and BASIC, **Oric** tapes and BASIC.

## 0.6.0

Released 13 November 2026.

Limoniter becomes an IDE, for the old machines and the new: completion, hover, go to definition, renaming and problems as you type in Z80, 6502, 6809 and 68000 source, language servers for everything else, a fuller debugger with watches, a memory window and a profiler, a disassembler, a character and sprite editor, a Spectrum screen designer, a terminal and tasks.

**Knowing your code** (Z80, 6502, 6809 and 68000 source)
- **Completion** as you type: instructions, directives, your labels and macros; BASIC keywords in Spectrum and Commodore listings.
- **Hover** over a label for its value, or an instruction for its address, bytes and timing.
- **Go to definition** (F12), **Find all uses** (Shift+F12) and **Rename** (Ctrl+F12), across INCLUDEd files.
- **Problems as you type**, and an **outline** of labels, macros and ORGs in the Functions panel.

**Language servers** for everything else (Settings, Editing; off until switched on)
- Completion, hover, definitions, uses, renaming and problems from rust-analyzer, clangd, pylsp, gopls and others.

**Debugging**
- **Call stack, watches and a memory window** that picks out what changed.
- **Conditional breakpoints** (`A=0 && [count]>3`) and **stopping when an address is written**.
- **Spectrum BASIC**: breakpoints on lines, a statement at a time, and its variables, on the built-in Spectrum.
- **6809 programs in XRoar** and **68000 programs in Hatari**.
- **A real Next**, through DeZog's dezogif on a serial cable (not yet tried on a real Next: tell us how it goes).
- **A profiler** on the built-in Spectrum: T-states by routine and line.

**Tools for the old machines**
- **Disassembler** for the Z80 (and Next), 6502 and 6809: traced, labelled, and always assembling back to the same bytes.
- **Character and sprite editor**: UDGs, fonts and the Next's 256-colour sprites, to and from DB lines and files.
- **Spectrum screen designer**: draw with INK, PAPER, BRIGHT and FLASH; bring in a PNG; save a .scr.

**For every project**
- **Terminal** (Ctrl+`): your shell in a panel, on a real pseudo-terminal.
- **Tasks**: build, run and test commands found from Cargo, Make, npm, Go, pytest and CMake, or your project's own; **Ctrl+Shift+B** builds, **Ctrl+Shift+T** tests.
- **Problems read back** from compilers and marked by their lines; **test results** listed test by test, with Run only this.

## 0.5.1

Released 13 November 2026.

New files of any kind from File › New as, assembling to a plain binary, and one box to choose the assembler for each processor.

- **File › New as** starts a new file of a kind: Spectrum, Next, C64 or BBC BASIC (ready for F9, and GRAPH mode for the Spectrum's), Z80, 6502, 6809 or 68000 assembler with a few lines to begin with, Markdown, or text in a retro encoding. The list is yours to change: each kind is a file ending, an encoding, line endings, colouring and starting text (File › New as › Change this list, or Settings).
- **Assemble and save as binary** (File › Run on) saves just the assembled bytes, with no loader or header, and says the address to load them at.
- **One Assemblers box** (File › Run on › Assemblers, or Settings) chooses Limoniter's own assembler or an installed one for each processor, all in one place. A program you point to is matched to its processor.
- **Fixed:** assembler source ending `.z80` opened as a Spectrum snapshot. It now opens as source; real snapshots still open as snapshots.

## 0.5.0

Released 13 November 2026.

Limoniter now writes machine code, runs it and debugs it: assemblers for the Z80, 6502, 6809 and 68000 built in, F9 to assemble and run on the built-in Spectrum or in an emulator, and a debugger with breakpoints and stepping. Spectrum BASIC gets GRAPH mode, UDGs drawn as their pictures, and a Spectrum keyboard on the screen.

**Write machine code, run it and debug it**
- **Assemblers built in** for the Z80 (with the Next's extra instructions), the 6502, the 6809 and the 68000. Each gives exactly the same bytes as the tool people already use: sjasmplus, ACME, lwasm and vasm. They understand those tools' ways of writing too, ca65's as well for the 6502.
- **Or use your own**: File › Run on › Assembler… picks sjasmplus or Pasmo for the Z80, ACME for the 6502, lwasm for the 6809 or vasm for the 68000. Problems go in the Output panel; click one to go to its line.
- **F9 assembles and runs**: a Spectrum tape with a BASIC loader (48K or 128K), a Next `.nex`, a C64 program, or an Atari ST program in Hatari. A 6809 program is saved as a CoCo `.bin`. **Assemble and save** saves it without running.
- **A debugger**: click beside a line number for a breakpoint. When the program stops there, the line is marked and the registers are shown, with Step, Step over and Carry on. It works on the built-in Spectrum, in ZEsarUX (so Next programs too) and for 6502 programs in VICE.
- The status bar shows each line's address, bytes and how long it takes to run.

**Spectrum BASIC**
- **GRAPH mode** (Ctrl+Shift+9) types block graphics and UDGs as the Spectrum's GRAPHICS key does.
- **UDGs are drawn as their pictures**, taken from the tape or snapshot the program came from, its DATA lines, or a file you choose.
- **zmakebas and bas2tap listings** convert to programs exactly as those tools do. Settings chooses which way listings write graphics.
- **A Spectrum keyboard** under the built-in Spectrum, with every key's words and symbols, for the key combinations.

**Also**
- Packaging for the Microsoft Store and winget is ready, for when Limoniter is published there.
