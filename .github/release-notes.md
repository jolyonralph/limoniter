## What's new in 0.16.0

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

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **Windows, portable:** `Limoniter-…-Windows-portable.zip`, with no installer and no administrator needed: unzip it anywhere (a USB stick, say) and run `limoniter.exe`. It keeps its settings and everything else in the `Limoniter data` folder beside it, leaving nothing on the PC, so several versions can sit side by side. It's unsigned too, so Windows warns about it as it does about the installer.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

`SHA256SUMS.txt` lists each download's SHA-256, and `SHA256SUMS.txt.minisig` is its signature, which Limoniter checks before it updates itself.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
