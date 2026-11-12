//! The files a large file's whole-file changes are written to. Converting, reindenting or sorting a file too
//! large to hold writes the result to a new file, which the document then reads from, so these can be as large as
//! the files themselves.
//!
//! Each run keeps its own folder, locked for as long as it runs, as the recovery folders are. A run that ends
//! normally deletes its folder; one that crashed leaves it, and the next run deletes it, except for any file a
//! recovery copy still reads from, which is kept until that copy has been dealt with.

use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const LOCK_NAME: &str = "run.lock";

/// This run's folder.
pub struct Folder {
    dir: PathBuf,
    lock: Option<File>,
    made: usize,
}

impl Folder {
    /// Makes this run's folder under the root, and locks it.
    pub fn start(root: &Path) -> std::io::Result<Self> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = root.join(format!("run-{stamp}-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let lock = crate::recovery::take_lock(&dir.join(LOCK_NAME))?;
        Ok(Self { dir, lock: Some(lock), made: 0 })
    }

    /// A new file to write to, named after the document's file.
    pub fn new_path(&mut self, stem: &str, ending: &str) -> PathBuf {
        self.made += 1;
        self.dir.join(format!("{stem}-{}{ending}", self.made))
    }
}

impl Drop for Folder {
    /// Ending normally: nothing is read from these files once the window has closed.
    fn drop(&mut self) {
        drop(self.lock.take());
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Deletes what runs that have stopped left behind, keeping any file in `keep` (those recovery copies read from).
/// Returns how many files were deleted.
pub fn sweep(root: &Path, keep: &HashSet<PathBuf>) -> usize {
    let mut deleted = 0;
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    let kept = |path: &Path| keep.iter().any(|k| crate::app::same_path(k, path));
    for entry in entries.flatten().map(|entry| entry.path()) {
        if entry.is_file() {
            // Loose files, as earlier versions wrote them.
            if !kept(&entry) && fs::remove_file(&entry).is_ok() {
                deleted += 1;
            }
            continue;
        }
        // A folder still locked belongs to a run that's still going.
        let Ok(lock) = crate::recovery::take_lock(&entry.join(LOCK_NAME)) else {
            continue;
        };
        let mut left = false;
        for file in fs::read_dir(&entry).into_iter().flatten().flatten().map(|e| e.path()) {
            if file.file_name().is_some_and(|name| name == LOCK_NAME) {
                continue;
            }
            if kept(&file) {
                left = true;
            } else if fs::remove_file(&file).is_ok() {
                deleted += 1;
            }
        }
        drop(lock);
        if !left {
            let _ = fs::remove_dir_all(&entry);
        }
    }
    deleted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_runs_files_go_except_those_recovery_needs_and_a_running_ones_stay() {
        let root = std::env::temp_dir().join(format!("limoniter-large-edits-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        // A run that crashed: its folder is there, but nothing holds the lock.
        let crashed = root.join("run-1-1");
        fs::create_dir_all(&crashed).unwrap();
        fs::write(crashed.join(LOCK_NAME), "").unwrap();
        let (needed, spare) = (crashed.join("big-1.txt"), crashed.join("big-2.txt"));
        fs::write(&needed, "a").unwrap();
        fs::write(&spare, "b").unwrap();
        // A loose file from an earlier version.
        fs::write(root.join("old-1234-1.txt"), "c").unwrap();
        // This run, still going.
        let mut running = Folder::start(&root).unwrap();
        let mine = running.new_path("big", ".txt");
        fs::write(&mine, "d").unwrap();

        let keep: HashSet<PathBuf> = [needed.clone()].into();
        assert_eq!(sweep(&root, &keep), 2);
        assert!(needed.exists() && !spare.exists() && !root.join("old-1234-1.txt").exists());
        assert!(mine.exists(), "a running copy's files are left alone");
        // Once recovery no longer needs it, it goes too, and its folder with it.
        assert_eq!(sweep(&root, &HashSet::new()), 1);
        assert!(!crashed.exists());
        // Ending normally takes this run's folder away.
        drop(running);
        assert!(!mine.exists());
        let _ = fs::remove_dir_all(&root);
    }
}
