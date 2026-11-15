description: Keep your work's history with Git in Limoniter: the Git wizard for beginners, committing, pushing and pulling, branches, conflicts, history and blame.

# Git

Git keeps the history of your work: every version you've committed, which you can compare with, go back to, and share online. Limoniter does the everyday things in Git without a command line, and the Git wizard sets it up for you if you've never used it. This page shows you how.

## Starting with Git

If you've never used Git, the Git wizard takes you from nothing to your first commit, and to your work online if you'd like:

1. Choose **Tools › Git › Git wizard…**.
2. Follow the steps. The wizard gets Git if you haven't got it, asks for your name and email (which go with each commit), and then either starts keeping the history of a folder, with a first commit, or copies a project from GitHub.
3. If you'd like your work online, it connects your folder to a new GitHub repository.

Each step says what it does and why.

When you open a folder that looks like a project but isn't keeping its history, Limoniter offers Git once: **Show me how…**, **Not now**, or **Never for this folder**.

## Committing your changes

1. Open **Source control** with [[Ctrl+Shift+G]], or **Tools › Git › Source control**. It shows the branch, what's changed, what's ready to commit, and anything in conflict.
2. Click **+** beside each file to put it in the next commit, or **−** to leave it out. **↶** throws away a file's changes.
3. Write what the commit does in the box, in a line, with more below it if needed.
4. Click the commit button. It says how many files go in, such as **Commit 2 file(s)**.

With nothing put in, the button says **Commit all changes**, and every change to files Git already has goes in. **Add to the last commit** folds your changes into the commit before, if you forgot something.

**Write it for me** asks the coding assistant to write the commit message from your changes, for you to read over first.

## Seeing what's changed

- Marks beside the line numbers show what's changed since the last commit: green for new lines, blue for changed ones, and a red notch where lines were taken out. They follow your edits as you type.
- **Tools › Git › Put the change at the cursor in the next commit** commits just part of a file; **Undo the change at the cursor** puts that part back as it was (and Undo takes that back in turn).
- **Tools › Git › Compare with the last commit** shows the file beside its last committed version, in a tab of its own, where you can still edit the file. In the Project panel, a folder's menu compares the whole folder with the last commit.
- **Tools › Git › History of this file…** lists every commit that changed it. Compare any one with now, or open the file as it was.
- **Tools › Git › Show who changed each line** shows the author, date and commit of the cursor's line in the status bar.

## Sharing your work

- **Push** sends your commits online.
- **Pull** brings in others' commits, when yours simply follow on from them, so nothing is merged by surprise.
- **Fetch** finds out what's online without changing anything. Source control shows how far ahead or behind your branch is.

When your commits and the ones online have both moved on, **Pull, putting mine after theirs** puts your commits after theirs (a rebase). If it stops at a conflict, sort it out and press **Carry on**, or **Abort, back to how it was** to undo the whole pull.

Git runs in the background, and its own sign-in window, SSH keys and signing work as they do at a command line. Limoniter never holds a Git password. The messages people meet most often come with a line on what to do, such as "Pull first, then Push".

## Branches, putting aside and tags

- Switch branches, or make a new one with **New branch**, from Source control.
- **Put aside** keeps every change to one side (a stash), so you can work on something else. **Bring back** brings them back, and **Throw away** drops them.
- **Tags** names a commit, as a release is named, with **Tag this commit**, and can push the tag online.

## Settling a conflict

When a merge stops at a conflict, a bar over the file helps you settle each one:

1. Put the cursor in a conflict.
2. Choose **Keep mine**, **Keep theirs** or **Keep both**.
3. Go to the **Next conflict**, and so on.

When they're all done, the file is marked settled. **Ask the assistant** suggests lines that do what both sides meant, shown first for you to accept or not. To give up on the merge, choose **Abort the merge**.

## Merging in the merge tab

The merge tab shows the three versions side by side, and the merged result below for you to edit:

- At the top, left to right: the **original** both sides started from, **mine** and **theirs**. They scroll together and line up, with blank space where one side has fewer lines. Lines only one side changed are green on that side, lines both changed alike are yellow, and conflicts are red on all three.
- Below is the **result**, an ordinary editor. Everything that merged by itself is already in it. Each conflict is still there between Git's markers (`<<<<<<<`, `|||||||`, `=======` and `>>>>>>>`).

To settle the conflicts:

1. Press **Next conflict** or **Previous conflict**, or click a red line at the top. The cursor goes to that conflict in the result.
2. Choose **Take mine**, **Take theirs**, **Take both (mine first)**, **Take both (theirs first)** or **Take the original**. Or edit the lines by hand and take the markers out.
3. The bar counts the conflicts still left. Press **Save the result** (or [[Ctrl+S]]) when there are none. If you save with conflicts still left, Limoniter asks first.

There are three ways to open it:

- When Git stops a merge, **Resolve in merge…** beside the file in Source control (or in the bar over the file) opens it from Git's own copies of each side. When you save the result with nothing left to settle, Limoniter offers to mark the file resolved in Git (`git add`).
- A file with conflict markers in it, from anywhere, shows a bar offering **Resolve in merge…**, which reads the markers as the conflicts.
- **Tools › Git › Merge three files…** asks for the original, mine and theirs, and where to save the result.

## Using Limoniter for git difftool and git mergetool

**Tools › Git › Use Limoniter for Git diff and merge** (also under **Diff and merge tool** in Source control) sets Git to open Limoniter for `git difftool` and `git mergetool`, in every repository on this computer. It changes only your own Git settings (`git config --global`), never a repository's. If Git already uses another tool, Limoniter shows you what's set and asks first.

**Stop using Limoniter for Git** puts back exactly what was set before, and takes away anything that wasn't.

`git mergetool` waits for the merge tab to close. If you saved the result with no conflicts left, Git marks the file resolved; if not, Git leaves it in conflict.

From a command line:

```
limoniter --diff LEFT RIGHT
limoniter --merge BASE MINE THEIRS RESULT
```

`--diff` compares two files side by side. `--merge` opens the merge tab in a window of its own, saving to RESULT, and when the tab is closed the program ends with exit code 0 if the result was saved with no conflicts left, or 1 if not. `limoniter --help` lists these.
