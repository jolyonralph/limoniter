## What's new in 0.3.1

Files of 64 MB and more, which Limoniter keeps on disk, can now use the whole-file commands that were set aside for them:

- **Compare** two huge files: the lines are compared in the background, and only the ones on screen are read.
- **Table view** of a huge CSV or TSV: rows are found, sorted and filtered, and figures added up, in the background. Inserting or deleting a column and sorting the file rewrite it in the background, as one step for Undo.
- **Print** a huge file: pages are made as they're sent.
- **Reindent**, **convert the encoding or line endings in place**, **Filter on selection** and **hide lines**.

Also:

- If Limoniter stops unexpectedly, the copies these commands write are deleted the next time it starts, unless recovery still needs one.
- Printing on macOS and Linux no longer holds up the window.
- Sorting a table is quicker.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, Control-click it and choose **Open** (or, on newer macOS, **System Settings → Privacy & Security → Open Anyway**).
- **Linux:** build it from the source with `cargo build --release`; see the README.

See [what Limoniter can do](https://limoniter.com/features.html) and [how it compares](https://limoniter.com/compare.html).
