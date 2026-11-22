## What's new in 0.16.1

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

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **Windows, portable:** `Limoniter-…-Windows-portable.zip`, with no installer and no administrator needed: unzip it anywhere (a USB stick, say) and run `limoniter.exe`. It keeps its settings and everything else in the `Limoniter data` folder beside it, leaving nothing on the PC, so several versions can sit side by side. It's unsigned too, so Windows warns about it as it does about the installer.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

`SHA256SUMS.txt` lists each download's SHA-256, and `SHA256SUMS.txt.minisig` is its signature, which Limoniter checks before it updates itself.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
