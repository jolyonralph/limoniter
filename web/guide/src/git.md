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
- **Tools › Git › Compare with the last commit** shows the file beside its last committed version.
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
