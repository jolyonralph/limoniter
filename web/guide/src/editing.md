description: Edit text and bytes in Limoniter: selecting, column editing, the undo tree, finding and replacing, laying out text, table view and hex mode.

# Editing text and hex

Limoniter edits text as any editor does, and adds a few things of its own: column editing, an undo that never loses anything, and a hex mode that works on files of any size. This page shows you how to use them.

## Everyday editing

Cut, copy, paste, undo and redo are where you'd expect: [[Ctrl+X]], [[Ctrl+C]], [[Ctrl+V]], [[Ctrl+Z]] and [[Ctrl+Y]]. A few more that save time:

- [[Ctrl+D]] duplicates the line.
- **Edit › Lines** moves lines up and down, joins them, sorts them A to Z or Z to A, and trims spaces.
- **Edit › Change case** makes the selection UPPER CASE ([[Alt+U]]), lower case ([[Alt+L]]), Sentence case or Title Case.
- **Edit › Swap with clipboard** ([[Ctrl+Shift+V]]) puts the selection on the clipboard and what was on the clipboard in its place.
- **Edit › Insert** puts in the date, the time, the file's name or path, a GUID, a colour, a byte by its value, or the whole of another file.
- **Edit › Snippets** ([[Ctrl+J]]) puts in a piece of text you use often. In a snippet, `$CURSOR` is where the cursor goes, `$SELECTION` is what was selected, and `$DATE`, `$TIME` and `$FILE` are filled in.

### Selecting what's inside quotes or brackets

To select the text inside the quotes around the cursor, press [[Ctrl+Shift+Q]]. Press it again to take the quotes too. In the same way, [[Ctrl+Shift+B]] selects inside brackets, [[Ctrl+Shift+T]] inside a tag, [[Ctrl+Shift+W]] a word, [[Ctrl+Shift+L]] a line and [[Ctrl+Shift+G]] a paragraph. Then type to replace it, or press Delete to remove it. **Edit › Select**, **Edit › Replace inside** and **Edit › Delete** do the same from the menu.

## Editing in columns

To change the same place on many lines at once:

1. Hold [[Alt]] and drag across the lines, or press [[Shift+Alt]] and the arrow keys, to select a block.
2. Type. What you type goes on every selected line, and Backspace and Delete work on every line too.

[[Alt+C]] turns column mode on and off, so that every selection is a block (word wrap needs to be off). **Edit › Columns** fills, cuts and deletes columns, justifies them left, centre or right, numbers them, and adds up the numbers in them.

## Undo that keeps everything

If you undo some changes and then type something else, the changes you undid aren't lost. Limoniter keeps every state the file has been in, as a tree.

1. Choose **Edit › Edit history** ([[Ctrl+Shift+H]]).
2. The Edit history panel shows every state, with its branches.
3. Click any state to go back to it.

![The Edit history panel beside some code](undo-tree "The Edit history panel: every state is kept, branches and all.")

Saving keeps the history with the file, so it's there next time you open it. You can turn that off in **Tools › Settings…**, under Editing.

To see the changes to just one part of a file, select the lines, or put the cursor in a function, and choose **Edit › History of this part** ([[Ctrl+Alt+H]]). Each change is listed, and any one of them can be taken back without undoing what you did afterwards anywhere else.

## Finding and replacing

- [[Ctrl+F]] finds, and [[Ctrl+R]] replaces. [[F3]] and [[Shift+F3]] find the next and previous match.
- Tick **Match case**, **Whole word** or **Regular expression** as you need, or **Hex bytes** to search for bytes such as `1B 5B`.
- [[Ctrl+Shift+F]] finds, or replaces, in every file in a folder (with **Include subfolders** and the file types you choose), in the open files, or in a project. Tick **Keep a .bak copy of each file changed on disk** if you'd like a copy of each file as it was.
- **Search › Only show lines with the selection** hides every line that doesn't contain the selected text.
- [[Ctrl+G]] goes to a line.

Searching runs in the background, so the window keeps working while a big search goes on.

## Finding your way around a file

- **Bookmarks**: [[Ctrl+F2]] sets or clears one on the cursor's line, and [[F2]] and [[Shift+F2]] go to the next and previous.
- **Colour tags** mark sections in six colours. Select some lines, or put the cursor in a function or paragraph, and press [[Ctrl+Shift+1]] to [[Ctrl+Shift+6]]. Tags show down the left of the text and across the document map, and stay with the file. [[F4]] and [[Shift+F4]] go to the next and previous, and [[Ctrl+Shift+0]] takes one away.
- **View › Narrow to the selection** ([[Ctrl+Shift+N]]) shows only the selected lines, so that every command, Replace All included, works on just those. **View › Show the whole file** brings the rest back.
- **View › Folding** folds and unfolds everything, and hides or shows chosen lines.

## Laying out text and code

- **Format › Reindent** reindents by brackets; **Tabs to spaces** and **Spaces to tabs** change the indenting.
- **Format › Paragraph** reformats paragraphs to a width and margins, aligns them, breaks long lines at a column, or joins each paragraph into one line.
- **Format › Lay out code** arranges HTML, XML, JSON, JavaScript, CSS, SQL and C-like code, and **Format › Compress** puts them on as few lines as possible.
- **Format › Comment out** and **Uncomment** work in the file's language.

> The formatters only ever change spacing. If anything else would change, the text is left as it was.

## Table view

To work on comma-, tab-, semicolon- or pipe-separated data as rows and columns, choose **View › Table view (CSV, TSV)** ([[F6]]). A file that looks like such data is offered as a table when it opens.

- Type, or double-click, to change a cell. [[Enter]] keeps it and moves down, [[Tab]] moves across, and [[Esc]] leaves it as it was.
- Select a block of cells with Shift and the arrow keys, Shift+click, or by dragging. Copy and paste work with spreadsheets such as Excel and LibreOffice.
- Click a heading to show the rows in that column's order, or type in the box above to show only the rows containing something. Neither changes the file; **Sort the file by this column** does.

The text is still the file: changing a cell changes only that field, and one Undo takes it back.

## Hex mode

To see and edit a file's bytes, press [[Ctrl+H]]. Press it again to go back to text. **View › Hex › As hex, in EBCDIC** shows the characters as an IBM mainframe would.

![A 10 GB file open in hex view](huge-hex "A 10 GB file, straight into hex, read from disk as needed.")

- [[Tab]] moves between the hex and the character panes.
- [[Insert]] switches between overwriting bytes and inserting them. **View › Hex › Insert or delete bytes…** adds or takes away a number of bytes at the cursor.
- [[Ctrl+G]] goes to an offset. Write `0x` in front of a hex offset, such as `0x1F00`.
- **View › Hex › Copy the selected rows** copies them as they look on screen.
- **View › Hex › String explorer** lists the readable text among the bytes, and says how each string is kept: with its length in front, a zero after it, padded to a width, with the top bit set on its last character, or ending in `$`. Click one to select it.

Hex mode works on files of any size. Typing, inserting, deleting, undo, search and Replace All all work, even on a file of many gigabytes, and saving copies the untouched parts straight across.

**Tools › Settings…**, under Hex mode, sets how many bytes go on a row, and whether files with zero bytes in them open in hex.

## Comparing files

To see what's different between two files, choose **Tools › Compare with…** and pick another open file or one on disk, or choose **Tools › Compare files…** and pick two. They open side by side in a tab of their own.

- Lines that differ are marked in colour, with a sign beside each: ~ changed, - only on the left, + only on the right. The words that differ within a line are marked more strongly; **Within lines** switches to single characters.
- **Previous** and **Next**, or **Search › Next change**, go from one difference to the next.
- Both sides can be edited, and each has its own **Save** and Undo. If a file is already open, its tab's text is used, unsaved edits and all, and what you change in the comparison changes there.
- The arrows between the sides copy a difference across. **Copy to the right** and **Copy to the left** copy the one the cursor is in.
- **Ignore spaces**, **Ignore case** and **Ignore line endings** leave those out. **Scroll together** keeps the sides in step.
- **Save as a patch…** writes the differences as a unified diff, which patch and git apply can use.

Files that aren't text are compared byte by byte, with each byte that differs marked. Pictures Limoniter can show are compared as pictures, with a third picture showing where they differ. Files too large to hold in memory are compared in the background and shown read-only.

## Comparing folders

**Tools › Compare folders…** shows two folders side by side as a tree. Each entry says how it compares, in words as well as colour: the same, different (and which side is newer), only on the left, only on the right, or a file on one side and a folder on the other.

1. Choose what each side is: a folder, a Git revision (HEAD for the last commit, or a branch, a tag or a commit), a disk image or archive, or a folder on a server (`sftp://user@host/folder`, for a server you've connected to before with File › Open a remote file).
2. Choose how to compare: by size and time, which is quick, or by contents. Where a side keeps no times, as a Git revision doesn't, files of the same size are compared by contents anyway.
3. Type patterns in **Only** or **Leave out** to choose files, such as `*.rs; *.toml` or `target; *.log`, and tick **Leave out what .gitignore does** if you like. Then press **Compare**.

Comparing runs in the background, and **Stop** stops it. Click a folder's arrow to open or close it, and tick **Hide what's the same** to see only the differences. Double-click a pair of files to compare them.

To copy files across, choose them (Ctrl and Shift add to the choice) and press **Copy to the right…** or **Copy to the left…**. You'll see a list of what will happen first. Nothing is ever deleted, and a file that's replaced is kept in a backup folder first.

**Save a report…** saves the comparison as an HTML page or plain text, and **Save as a patch…** saves every difference in the text files as one unified diff.

In the Project panel, right-click a file for **Compare with…**, or a folder for **Compare folders…**. Both offer **Compare with the last commit**.

Within one file, **View › Show › Changed lines** marks the lines you've changed, and **Search › Next change** and **Search › Previous change** go from one to the next.
