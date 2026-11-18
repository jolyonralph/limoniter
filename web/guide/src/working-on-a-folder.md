description: Use Limoniter as an IDE: open a folder, find files and symbols, search and replace across it, use language servers, debug Python, C, C++ and Rust, and work with files side by side and terminals in tabs.

# Working on a folder of code

Open a folder and Limoniter works across all of it, the way VS Code does. You get an Explorer, Go to file and Go to symbol, a Search panel, language servers, a debugger, files side by side and terminals in tabs. This page shows you how to use them.

## Opening a folder

1. Choose **File › Open folder…** and pick the folder your code is in.
2. The Project panel shows it as an **Explorer**: folders first, the file you're editing picked out, and Git's status beside each changed file (M changed, A added, U not tracked yet). A dot marks a folder with changes inside.
3. Click a file to open it. Right-click for New file, Rename, Find in this folder, Open a terminal here, Copy path and the rest.

Drag a file or folder onto another folder to move it there. Files you have open follow it.

Every file is indexed in the background, leaving out what `.gitignore` leaves out, so even a folder of thousands of files is quick to search. Nothing is written into the folder: Limoniter keeps the project's tabs, tasks and notes beside its own settings.

> If the folder has an `.editorconfig`, Limoniter follows it: its indentation, line endings, trailing spaces and final newline.

## Finding files and symbols

- [[Ctrl+E]] (**File › Go to file…**) finds a file from a few letters of its name. Type `wsh` for `workspace_host.rs`; letters together and at the start of words count most.
- [[Ctrl+T]] finds a function or label anywhere in the folder. It's the same box with `#` in front.
- [[Ctrl+Shift+O]] lists the functions and labels in the file you're in, with `@` in front.
- [[Enter]] opens what's chosen; [[Ctrl+Enter]] opens it beside the file you're in.

## Searching and replacing

![The Search panel](ide-search "The Search panel shows each replacement before it's made.")

1. Press [[Ctrl+Shift+F]] to open the **Search panel**. Whatever's selected becomes what to find.
2. Type. Results come as you type, as a tree: each file with how many matches, and under it each line with the match marked. Click a line to go there.
3. **Aa**, **ab** and `.*` turn on match case, whole words and regular expressions. **…** shows boxes for the files to search and to leave out, such as `*.rs;*.s`.
4. To replace, click **▸** to open the Replace box and type what goes in its place. Each match now shows struck through, with the new text after it, so you can see the change before you make it.
5. Click **Replace all**, or **⇄** on one file. Limoniter asks first. Files you have open change in their tabs, where Undo takes it back; the rest change on disk.

Click **×** on a file to leave it out of the results, and out of Replace all.

## Language servers

A language server knows a language well: it marks mistakes as you type, finishes words, says what things are, and finds where they're defined. Limoniter uses rust-analyzer, clangd, pylsp, typescript-language-server, gopls and others.

When you open a file whose language has one, Limoniter offers it: **Use it** if it's installed, or **Install it in a terminal** if it isn't. It runs the install in a terminal tab and starts the server when it's done.

With a server running:

- [[Ctrl+Space]] offers words to finish the one you're typing, and hovering over a name says what it is.
- [[F12]] goes to where it's defined, [[Alt+F12]] shows the definition in a small window without leaving your place, and [[Shift+F12]] lists every use.
- [[Ctrl+.]] offers the server's **quick fixes** for the line you're on.
- [[Shift+Alt+F]] formats the whole file. Tick **Format files as they're saved** in **Settings › Editing › Language servers** to do it every time you save.
- As you type a call, its parameters show above the line, with the one you're typing marked. Types and parameter names show faintly at the ends of lines.

## Debugging

![Debugging a Python program](ide-debug "A Python program paused at a breakpoint.")

Limoniter debugs **Python** with debugpy, and **C, C++ and Rust** with gdb (version 14 or later) or lldb-dap.

1. Click beside a line's number to set a breakpoint. A red dot shows.
2. Press [[Ctrl+F5]] (**Tools › Run › Debug**). A Python file runs straight away. For C, C++ or Rust, say which program to run the first time: Limoniter guesses the debug build Cargo makes.
3. When it stops, the line is marked and the **Debug panel** shows the call stack and the variables. Click **▸** beside a variable to see inside it.
4. [[F10]] steps over a line, [[F11]] steps into a call, [[Shift+F11]] steps out, [[F5]] carries on and [[Shift+F5]] stops.
5. Type an expression in the console box to work it out where the program has stopped.

> If debugpy isn't installed, Limoniter says how to install it: `python3 -m pip install --user debugpy`.

## Laying out your work

![Two files side by side](ide-beside "Two files side by side, with breadcrumbs, sticky scroll and a terminal.")

- [[Ctrl+\]] shows the file you used before beside the one you're in. Click in either to bring it to the front; it stays on its side. Press [[Ctrl+\]] again to go back to one.
- **Breadcrumbs** above the text show the file's folders and the function you're in. Click any of them for a menu: a folder's files to open, or the file's functions to go to.
- **Sticky scroll** keeps the function you've scrolled into at the top of the text. Click it to go there.
- [[Ctrl+`]] opens a terminal in the folder. Click **+** for another; each has its own tab. [[Ctrl+`]] hides the panel and brings it back, and the terminals keep running.

Both breadcrumbs and sticky scroll can be turned off under **View › Show**.

## Building and testing

- **Tools › Tasks** finds build, run and test commands from Cargo, a Makefile, package.json, Go and CMake. [[Ctrl+Shift+B]] builds and [[Ctrl+Shift+T]] tests.
- Where the compiler says there's a problem is a click away in the Output panel, and marked beside its line.
- A test task's results are listed test by test, with **Run again** and **Run only this** for one that fails.

For Git, see [Git](git.html).
