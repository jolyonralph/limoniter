//! Syncing two sides of a folder comparison. Update copies what's newer and what's missing, one way or both;
//! Mirror makes one side exactly like the other, taking away what the other hasn't got. Either is planned first
//! as a list of everything it would do, which can be ticked and unticked, and nothing is done until that's
//! agreed to. A file replaced or taken away is kept first in a backup folder named for when it was, and left
//! alone if that can't be done.
//!
//! A saved comparison remembers each file as it was after the last sync (its size, time and fingerprint), so a
//! file changed on both sides since then is told apart as a conflict, shown but left alone unless ticked.

use crate::diff_engine::{FolderResult, Meta, Progress, Row, Status, Tree, check_writable, keep_backup, put_file, safe_path, same_size_and_time, still_as_compared};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The most a file may be to have its fingerprint kept for the next sync.
const MOST_FINGERPRINTED: u64 = 16 * 1024 * 1024;

/// Which kind of sync.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SyncKind {
    /// Newer and missing files are copied; nothing is taken away.
    #[default]
    Update,
    /// One side is made exactly like the other, which takes away what the other hasn't got.
    Mirror,
}

/// Which way a sync goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SyncWay {
    #[default]
    LeftToRight,
    RightToLeft,
    /// Each file goes from the side where it's newer. For Update only.
    Both,
}

impl SyncKind {
    pub fn id(self) -> &'static str {
        match self {
            SyncKind::Update => "update",
            SyncKind::Mirror => "mirror",
        }
    }

    pub fn from_id(id: &str) -> Self {
        if id == "mirror" { SyncKind::Mirror } else { SyncKind::Update }
    }
}

impl SyncWay {
    pub fn id(self) -> &'static str {
        match self {
            SyncWay::LeftToRight => "left-to-right",
            SyncWay::RightToLeft => "right-to-left",
            SyncWay::Both => "both",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "right-to-left" => SyncWay::RightToLeft,
            "both" => SyncWay::Both,
            _ => SyncWay::LeftToRight,
        }
    }
}

/// What a sync does to one file or folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Act {
    /// A file that isn't there is copied over.
    Create,
    /// A file there is kept in the backup folder, then copied over.
    Replace,
    /// A file there is kept in the backup folder, then taken away.
    Delete,
    MakeFolder,
    /// An empty folder is taken away. One with anything left in it is kept.
    RemoveFolder,
}

impl Act {
    pub fn words(self) -> &'static str {
        match self {
            Act::Create => "New",
            Act::Replace => "Replace",
            Act::Delete => "Delete",
            Act::MakeFolder => "New folder",
            Act::RemoveFolder => "Delete folder",
        }
    }

    /// The order things are done in: folders made, files copied, files taken away, then folders.
    fn phase(self) -> u8 {
        match self {
            Act::MakeFolder => 0,
            Act::Create | Act::Replace => 1,
            Act::Delete => 2,
            Act::RemoveFolder => 3,
        }
    }
}

/// One thing a sync would do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncItem {
    pub path: String,
    pub act: Act,
    /// Whether it's done to the right side; otherwise to the left.
    pub to_right: bool,
    /// Whether it will be done. Conflicts start unticked.
    pub ticked: bool,
    /// Changed on both sides since the last sync, or otherwise needing a look before it's done.
    pub conflict: bool,
    /// Why it's unticked or needs a look, in words.
    pub note: Option<String>,
    /// The size of the file copied, or taken away.
    pub size: u64,
    /// The modified time the copy is to have.
    pub modified: Option<u64>,
    /// What was there on the side it's done to when it was compared.
    pub expected: Option<Meta>,
}

/// What a sync will do, to show before it's done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncPlan {
    pub kind: SyncKind,
    pub way: SyncWay,
    pub items: Vec<SyncItem>,
    /// What's left alone, and why.
    pub skipped: Vec<(String, String)>,
}

/// How many of each a plan will do, of those ticked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanCounts {
    pub create: usize,
    pub replace: usize,
    pub delete: usize,
    pub folders_made: usize,
    pub folders_removed: usize,
    pub conflicts: usize,
}

impl PlanCounts {
    pub fn describe(&self) -> String {
        let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let mut parts = vec![plural(self.create, "new file", "new files"), plural(self.replace, "file replaced", "files replaced"), plural(self.delete, "file deleted", "files deleted")];
        if self.folders_made > 0 {
            parts.push(plural(self.folders_made, "new folder", "new folders"));
        }
        if self.folders_removed > 0 {
            parts.push(plural(self.folders_removed, "folder deleted", "folders deleted"));
        }
        parts.join(" \u{B7} ")
    }
}

impl SyncPlan {
    pub fn counts(&self) -> PlanCounts {
        let mut counts = PlanCounts::default();
        for item in &self.items {
            if item.conflict {
                counts.conflicts += 1;
            }
            if !item.ticked {
                continue;
            }
            match item.act {
                Act::Create => counts.create += 1,
                Act::Replace => counts.replace += 1,
                Act::Delete => counts.delete += 1,
                Act::MakeFolder => counts.folders_made += 1,
                Act::RemoveFolder => counts.folders_removed += 1,
            }
        }
        counts
    }

    /// How many files and folders it will take away, which a Mirror asks about by number before it goes ahead.
    pub fn deletions(&self) -> usize {
        self.items.iter().filter(|item| item.ticked && matches!(item.act, Act::Delete | Act::RemoveFolder)).count()
    }

    pub fn ticked(&self) -> usize {
        self.items.iter().filter(|item| item.ticked).count()
    }
}

// ---------- What the last sync left ----------

/// A file as it was on one side after a sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapMeta {
    pub size: u64,
    pub modified: Option<u64>,
}

/// A file as it was on both sides after a sync, when they were the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapEntry {
    pub left: SnapMeta,
    pub right: SnapMeta,
    /// A fingerprint of its contents, for files small enough, to tell a file only touched from one changed.
    pub hash: Option<[u8; 32]>,
}

/// Every file both sides had alike after the last sync.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// When, in seconds since 1970.
    pub taken: u64,
    pub files: BTreeMap<String, SnapEntry>,
}

fn snap(meta: Meta) -> SnapMeta {
    SnapMeta { size: meta.size, modified: meta.modified }
}

/// Whether a file on a side has changed since the snapshot: its size or time differ, and, where its fingerprint
/// was kept, so do its contents.
fn changed_since(tree: &dyn Tree, path: &str, then: SnapMeta, hash: Option<[u8; 32]>, now: Meta, cancel: &AtomicBool) -> bool {
    if same_size_and_time(Meta { is_dir: false, size: then.size, modified: then.modified }, now) {
        return false;
    }
    match hash {
        Some(hash) if then.size == now.size => tree.hash(path, cancel) != Ok(hash),
        _ => true,
    }
}

/// What both sides are like now, after a sync: each file that's the same on both, with its fingerprint where a
/// side is on this computer. What the last snapshot had for a file that's still on a side but no longer the same
/// is kept, so a change on both sides is still noticed the next time.
pub fn take_snapshot(result: &FolderResult, previous: Option<&Snapshot>, progress: &Progress, cancel: &AtomicBool) -> Snapshot {
    let mut files = BTreeMap::new();
    let local = if result.left_tree.local_path("").is_some() { Some(&result.left_tree) } else if result.right_tree.local_path("").is_some() { Some(&result.right_tree) } else { None };
    progress.set_phase("Remembering how both sides are");
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(result.rows.len() as u64, Ordering::Relaxed);
    for row in &result.rows {
        progress.done.fetch_add(1, Ordering::Relaxed);
        if row.is_dir() || cancel.load(Ordering::Relaxed) {
            continue;
        }
        match (row.left, row.right) {
            (Some(l), Some(r)) if row.status == Status::Same => {
                let hash = local.filter(|_| l.size <= MOST_FINGERPRINTED).and_then(|tree| tree.hash(&row.path, cancel).ok());
                files.insert(row.path.clone(), SnapEntry { left: snap(l), right: snap(r), hash });
            }
            _ => {
                if let Some(kept) = previous.and_then(|previous| previous.files.get(&row.path)) {
                    files.insert(row.path.clone(), *kept);
                }
            }
        }
    }
    Snapshot { taken: chrono::Utc::now().timestamp().max(0) as u64, files }
}

// ---------- Planning ----------

fn item(row: &Row, act: Act, to_right: bool) -> SyncItem {
    // `Row::side` takes whether it is the left: the source is the left when copying to the right.
    let source = row.side(to_right);
    let there = row.side(!to_right);
    let (size, modified) = match act {
        Act::Delete => (there.map_or(0, |m| m.size), None),
        _ => (source.map_or(0, |m| m.size), source.and_then(|m| m.modified)),
    };
    SyncItem { path: row.path.clone(), act, to_right, ticked: true, conflict: false, note: None, size, modified, expected: there }
}

fn side_word(right: bool) -> &'static str {
    if right { "right" } else { "left" }
}

/// Works out what a sync would do. `snapshot` is how both sides were after the last one, if it's known.
pub fn plan_sync(result: &FolderResult, kind: SyncKind, way: SyncWay, snapshot: Option<&Snapshot>, cancel: &AtomicBool) -> Result<SyncPlan, String> {
    if kind == SyncKind::Mirror && way == SyncWay::Both {
        return Err("A mirror goes one way: choose which side is to be made like the other.".into());
    }
    match way {
        SyncWay::LeftToRight => check_writable(result, false)?,
        SyncWay::RightToLeft => check_writable(result, true)?,
        SyncWay::Both => {
            check_writable(result, true)?;
            check_writable(result, false)?;
        }
    }
    let mut plan = SyncPlan { kind, way, items: Vec::new(), skipped: Vec::new() };
    let mut removals = Vec::new();
    for row in &result.rows {
        if cancel.load(Ordering::Relaxed) {
            return Err("It was stopped.".into());
        }
        if row.status == Status::Same {
            continue;
        }
        if row.status == Status::TypeDiffers {
            plan.skipped.push((row.path.clone(), "It's a file on one side and a folder on the other, so it's left for you to sort out.".into()));
            continue;
        }
        if row.status == Status::Unreadable {
            plan.skipped.push((row.path.clone(), row.problem.clone().unwrap_or_else(|| "It couldn't be read.".into())));
            continue;
        }
        let known = snapshot.and_then(|snapshot| snapshot.files.get(&row.path));
        let changed = |left: bool| -> bool {
            let (Some(entry), Some(now)) = (known, row.side(left)) else { return false };
            let tree = result.tree(left);
            changed_since(&**tree, &row.path, if left { entry.left } else { entry.right }, entry.hash, now, cancel)
        };
        if row.is_dir() {
            if !matches!(row.status, Status::LeftOnly | Status::RightOnly) {
                continue;
            }
            // A folder on one side only: made on the other, or taken away by a mirror.
            let on_left = row.status == Status::LeftOnly;
            match (kind, way) {
                (SyncKind::Update, SyncWay::Both) => plan.items.push(item(row, Act::MakeFolder, on_left)),
                (_, SyncWay::LeftToRight) if on_left => plan.items.push(item(row, Act::MakeFolder, true)),
                (_, SyncWay::RightToLeft) if !on_left => plan.items.push(item(row, Act::MakeFolder, false)),
                (SyncKind::Mirror, SyncWay::LeftToRight) => removals.push(item(row, Act::RemoveFolder, true)),
                (SyncKind::Mirror, SyncWay::RightToLeft) => removals.push(item(row, Act::RemoveFolder, false)),
                _ => {}
            }
            continue;
        }
        match row.status {
            Status::LeftOnly | Status::RightOnly => {
                let on_left = row.status == Status::LeftOnly;
                // Copied over, unless a mirror the other way takes it away.
                let copy = match (kind, way) {
                    (SyncKind::Update, SyncWay::Both) => Some(on_left),
                    (_, SyncWay::LeftToRight) if on_left => Some(true),
                    (_, SyncWay::RightToLeft) if !on_left => Some(false),
                    _ => None,
                };
                if let Some(to_right) = copy {
                    let mut new = item(row, Act::Create, to_right);
                    if known.is_some() {
                        // Both sides had it at the last sync: it's been deleted from this one since.
                        new.ticked = false;
                        new.note = Some(format!("Deleted from the {} since the last sync", side_word(to_right)));
                        if changed(on_left) {
                            new.conflict = true;
                            new.note = Some(format!("Changed on the {} and deleted from the {} since the last sync", side_word(on_left), side_word(to_right)));
                        }
                    }
                    plan.items.push(new);
                } else if kind == SyncKind::Mirror {
                    // Taken away from the side that has it.
                    let mut gone = item(row, Act::Delete, !on_left);
                    if known.is_some() && changed(on_left) {
                        gone.ticked = false;
                        gone.conflict = true;
                        gone.note = Some(format!("Changed on the {} since the last sync, and deleted from the {}", side_word(on_left), side_word(!on_left)));
                    }
                    plan.items.push(gone);
                }
            }
            Status::Different | Status::NewerLeft | Status::NewerRight => {
                let (left_changed, right_changed) = if known.is_some() { (changed(true), changed(false)) } else { (false, false) };
                let both_changed = known.is_some() && left_changed && right_changed;
                // Which side has the newer file, by the last sync if it says, or by the times.
                let newer_left = match (known.is_some(), left_changed, right_changed) {
                    (true, true, false) => Some(true),
                    (true, false, true) => Some(false),
                    _ => match row.status {
                        Status::NewerLeft => Some(true),
                        Status::NewerRight => Some(false),
                        _ => None,
                    },
                };
                let to_right = match way {
                    SyncWay::LeftToRight => true,
                    SyncWay::RightToLeft => false,
                    SyncWay::Both => newer_left.unwrap_or(true),
                };
                let mut replace = item(row, Act::Replace, to_right);
                if both_changed {
                    replace.ticked = false;
                    replace.conflict = true;
                    replace.note = Some("Changed on both sides since the last sync".into());
                } else if kind == SyncKind::Update {
                    match newer_left {
                        // The one there is newer: an update leaves it.
                        Some(newer) if newer != to_right => continue,
                        Some(_) => {}
                        None => {
                            replace.ticked = false;
                            replace.note = Some("Different, with no telling which is newer".into());
                        }
                    }
                } else if newer_left == Some(!to_right) {
                    replace.note = Some(format!("The {} one is newer", side_word(to_right)));
                }
                plan.items.push(replace);
            }
            _ => {}
        }
    }
    // Folders are taken away from the deepest first, after what's in them.
    removals.reverse();
    plan.items.extend(removals);
    plan.items.sort_by_key(|item| item.act.phase());
    Ok(plan)
}

// ---------- Doing it ----------

/// What a sync did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub created: usize,
    pub replaced: usize,
    pub deleted: usize,
    pub folders_made: usize,
    pub folders_removed: usize,
    pub backed_up: usize,
    pub failed: Vec<(String, String)>,
    /// Where the files replaced and taken away were kept, if any were.
    pub backup_folder: Option<PathBuf>,
}

impl SyncReport {
    pub fn describe(&self) -> String {
        let counts = PlanCounts { create: self.created, replace: self.replaced, delete: self.deleted, folders_made: self.folders_made, folders_removed: self.folders_removed, conflicts: 0 };
        let mut said = format!("Synced: {}.", counts.describe());
        if let Some(folder) = &self.backup_folder {
            said.push_str(&format!("\n\nThe {} file{} replaced or deleted {} kept in {}.", self.backed_up, if self.backed_up == 1 { "" } else { "s" }, if self.backed_up == 1 { "is" } else { "are" }, folder.display()));
        }
        if !self.failed.is_empty() {
            said.push_str(&format!("\n\n{} left as {} were:", self.failed.len(), if self.failed.len() == 1 { "it" } else { "they" }));
            for (path, why) in self.failed.iter().take(12) {
                said.push_str(&format!("\n{path}: {why}"));
            }
        }
        said
    }
}

/// Does what's ticked in a plan. Before a file is replaced or taken away, it's kept in `backup_root`, in a folder
/// for its side ("left" or "right") under its path; a file that can't be kept, or that has changed since it was
/// compared, is left alone. Nothing is written outside the two sides and the backup folder.
pub fn run_sync(plan: &SyncPlan, left: &dyn Tree, right: &dyn Tree, backup_root: &Path, progress: &Progress, cancel: &AtomicBool) -> SyncReport {
    let mut report = SyncReport::default();
    let ticked: Vec<&SyncItem> = plan.items.iter().filter(|item| item.ticked).collect();
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(ticked.len() as u64, Ordering::Relaxed);
    for item in ticked {
        progress.done.fetch_add(1, Ordering::Relaxed);
        if cancel.load(Ordering::Relaxed) {
            report.failed.push((item.path.clone(), "It was stopped before this.".into()));
            continue;
        }
        let (target, source) = if item.to_right { (right, left) } else { (left, right) };
        let backups = backup_root.join(side_word(item.to_right));
        progress.set_phase(format!("{} {}", item.act.words(), item.path));
        let done = safe_path(&item.path).and_then(|()| match item.act {
            Act::MakeFolder => target.make_folder(&item.path).map(|()| report.folders_made += 1),
            Act::Create => still_as_compared(target, &item.path, None).and_then(|()| put_file(source, target, &item.path, item.modified)).map(|()| report.created += 1),
            Act::Replace | Act::Delete => {
                still_as_compared(target, &item.path, item.expected)?;
                keep_backup(target, &item.path, &backups).map_err(|e| format!("It couldn't be backed up first ({e}), so it was left as it was."))?;
                report.backed_up += 1;
                report.backup_folder = Some(backup_root.to_path_buf());
                if item.act == Act::Replace {
                    put_file(source, target, &item.path, item.modified).map(|()| report.replaced += 1)
                } else {
                    target.remove_file(&item.path).map(|()| report.deleted += 1)
                }
            }
            Act::RemoveFolder => target.remove_folder(&item.path).map(|()| report.folders_removed += 1),
        });
        if let Err(problem) = done {
            report.failed.push((item.path.clone(), problem));
        }
    }
    report
}

#[cfg(test)]
#[path = "compare_sync_tests.rs"]
mod compare_sync_tests;
