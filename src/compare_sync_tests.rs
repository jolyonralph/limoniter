//! Syncing: what Update and Mirror plan each way, unticking, conflicts told from the last sync, backups of what's
//! replaced and deleted, nothing written outside the side, and a pretend server standing in for SFTP.

use super::*;
use crate::compare_rules::{Rule, RuleKind, Rules};
use crate::diff_engine::{Filters, MemoryTree, Mode, RemoteFs, Side, SftpTree, compare_trees};
use crate::remote::{Entry, Site, Stamp};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

type Files<'a> = &'a [(&'a str, &'a str, Option<u64>)];

fn compare(left: &Arc<MemoryTree>, right: &Arc<MemoryTree>) -> FolderResult {
    let side = || Side::Folder(PathBuf::from("memory"));
    compare_trees(side(), side(), left.clone(), right.clone(), Mode::Thorough, &Filters::default(), &Progress::default(), &AtomicBool::new(false)).unwrap()
}

fn pair(left: Files, right: Files) -> (Arc<MemoryTree>, Arc<MemoryTree>) {
    (Arc::new(MemoryTree::writable_with(left)), Arc::new(MemoryTree::writable_with(right)))
}

fn planned(plan: &SyncPlan) -> Vec<(String, Act, bool, bool)> {
    plan.items.iter().map(|item| (item.path.clone(), item.act, item.to_right, item.ticked)).collect()
}

fn entry(path: &str, act: Act, to_right: bool, ticked: bool) -> (String, Act, bool, bool) {
    (path.to_string(), act, to_right, ticked)
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limoniter-sync-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn no() -> AtomicBool {
    AtomicBool::new(false)
}

const LEFT: Files = &[("same.txt", "same", Some(100)), ("newer-left.txt", "new!", Some(900)), ("newer-right.txt", "old", Some(100)), ("only-left.txt", "l", Some(5)), ("sub/deep.txt", "d", Some(5))];
const RIGHT: Files = &[("same.txt", "same", Some(100)), ("newer-left.txt", "old", Some(100)), ("newer-right.txt", "new!", Some(900)), ("only-right.txt", "r", Some(5)), ("gone/inner.txt", "i", Some(5))];

#[test]
fn update_copies_newer_and_missing_files_the_way_chosen() {
    let (left, right) = pair(LEFT, RIGHT);
    let result = compare(&left, &right);
    let to_right = plan_sync(&result, SyncKind::Update, SyncWay::LeftToRight, None, &no()).unwrap();
    assert_eq!(planned(&to_right), [entry("sub", Act::MakeFolder, true, true), entry("sub/deep.txt", Act::Create, true, true), entry("newer-left.txt", Act::Replace, true, true), entry("only-left.txt", Act::Create, true, true)]);
    assert_eq!(to_right.deletions(), 0, "an update never deletes");
    let to_left = plan_sync(&result, SyncKind::Update, SyncWay::RightToLeft, None, &no()).unwrap();
    assert_eq!(planned(&to_left), [entry("gone", Act::MakeFolder, false, true), entry("gone/inner.txt", Act::Create, false, true), entry("newer-right.txt", Act::Replace, false, true), entry("only-right.txt", Act::Create, false, true)]);
    let both = plan_sync(&result, SyncKind::Update, SyncWay::Both, None, &no()).unwrap();
    let counts = both.counts();
    assert_eq!((counts.create, counts.replace, counts.delete, counts.folders_made), (4, 2, 0, 2));
    assert!(both.items.iter().any(|item| item.path == "newer-left.txt" && item.to_right) && both.items.iter().any(|item| item.path == "newer-right.txt" && !item.to_right), "each from the side it's newer on");
    // Doing it: the replaced file is backed up, under its side's folder.
    let dir = scratch("update");
    let report = run_sync(&both, &*left, &*right, &dir, &Progress::default(), &no());
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!((report.created, report.replaced, report.backed_up), (4, 2, 2));
    assert_eq!(std::fs::read_to_string(dir.join("right/newer-left.txt")).unwrap(), "old");
    assert_eq!(std::fs::read_to_string(dir.join("left/newer-right.txt")).unwrap(), "old");
    assert_eq!(right.get("newer-left.txt"), Some(("new!".into(), Some(900))), "with its time");
    assert_eq!(left.get("gone/inner.txt").unwrap().0, "i");
    let again = compare(&left, &right);
    assert_eq!(again.counts().same, 7, "{:?}", again.counts());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn different_files_with_no_telling_which_is_newer_wait_to_be_ticked() {
    let (left, right) = pair(&[("a.txt", "one", None)], &[("a.txt", "two", None)]);
    let result = compare(&left, &right);
    let plan = plan_sync(&result, SyncKind::Update, SyncWay::LeftToRight, None, &no()).unwrap();
    assert_eq!(planned(&plan), [entry("a.txt", Act::Replace, true, false)]);
    assert!(plan.items[0].note.as_deref().unwrap().contains("no telling"));
}

#[test]
fn mirror_makes_one_side_like_the_other_and_backs_up_what_it_deletes() {
    let (left, right) = pair(LEFT, RIGHT);
    let result = compare(&left, &right);
    let plan = plan_sync(&result, SyncKind::Mirror, SyncWay::LeftToRight, None, &no()).unwrap();
    assert_eq!(
        planned(&plan),
        [
            entry("sub", Act::MakeFolder, true, true),
            entry("sub/deep.txt", Act::Create, true, true),
            entry("newer-left.txt", Act::Replace, true, true),
            entry("newer-right.txt", Act::Replace, true, true),
            entry("only-left.txt", Act::Create, true, true),
            entry("gone/inner.txt", Act::Delete, true, true),
            entry("only-right.txt", Act::Delete, true, true),
            entry("gone", Act::RemoveFolder, true, true),
        ]
    );
    assert_eq!(plan.deletions(), 3);
    assert!(plan.items[3].note.as_deref().unwrap().contains("right one is newer"), "a mirror says when it replaces a newer file");
    assert!(plan_sync(&result, SyncKind::Mirror, SyncWay::Both, None, &no()).is_err(), "a mirror goes one way");
    let dir = scratch("mirror");
    let report = run_sync(&plan, &*left, &*right, &dir, &Progress::default(), &no());
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!((report.deleted, report.folders_removed, report.replaced, report.backed_up), (2, 1, 2, 4));
    assert_eq!(std::fs::read_to_string(dir.join("right/only-right.txt")).unwrap(), "r", "deleted, but kept first");
    assert_eq!(std::fs::read_to_string(dir.join("right/gone/inner.txt")).unwrap(), "i");
    assert_eq!(std::fs::read_to_string(dir.join("right/newer-right.txt")).unwrap(), "new!");
    assert!(right.get("only-right.txt").is_none());
    let after = compare(&left, &right);
    assert!(after.rows.iter().all(|row| row.status == Status::Same), "exactly alike");
    assert!(report.describe().contains("kept in"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unticked_items_are_left_and_a_folder_still_holding_files_is_kept() {
    let (left, right) = pair(&[("keep.txt", "k", Some(1))], &[("keep.txt", "k", Some(1)), ("old/a.txt", "a", Some(1)), ("old/b.txt", "b", Some(1))]);
    let result = compare(&left, &right);
    let mut plan = plan_sync(&result, SyncKind::Mirror, SyncWay::LeftToRight, None, &no()).unwrap();
    assert_eq!(plan.deletions(), 3);
    plan.items.iter_mut().find(|item| item.path == "old/b.txt").unwrap().ticked = false;
    assert_eq!(plan.deletions(), 2);
    let dir = scratch("untick");
    let report = run_sync(&plan, &*left, &*right, &dir, &Progress::default(), &no());
    assert_eq!(report.deleted, 1);
    assert!(right.get("old/b.txt").is_some(), "unticked, so left");
    assert_eq!(report.failed.len(), 1, "the folder isn't empty");
    assert!(report.failed[0].1.contains("kept"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_changed_on_both_sides_since_the_last_sync_is_a_conflict() {
    let (left, right) = pair(&[("both.txt", "base", Some(100)), ("one.txt", "base", Some(100)), ("dropped.txt", "base", Some(100))], &[("both.txt", "base", Some(100)), ("one.txt", "base", Some(100)), ("dropped.txt", "base", Some(100))]);
    let synced = compare(&left, &right);
    let snapshot = take_snapshot(&synced, None, &Progress::default(), &no());
    assert_eq!(snapshot.files.len(), 3);
    // Since then: both changed on both sides, one only on the right (with an older time), dropped deleted from the right.
    left.write("both.txt", b"left edit", Some(200)).unwrap();
    right.write("both.txt", b"right edit", Some(300)).unwrap();
    right.write("one.txt", b"right only", Some(50)).unwrap();
    right.remove_file("dropped.txt").unwrap();
    let now = compare(&left, &right);
    let plan = plan_sync(&now, SyncKind::Update, SyncWay::Both, Some(&snapshot), &no()).unwrap();
    let both = plan.items.iter().find(|item| item.path == "both.txt").unwrap();
    assert!(both.conflict && !both.ticked, "shown, and skipped unless ticked");
    assert!(both.note.as_deref().unwrap().contains("both sides"));
    let one = plan.items.iter().find(|item| item.path == "one.txt").unwrap();
    assert!(!one.to_right && one.ticked && !one.conflict, "the side that changed wins, whatever the times say");
    let dropped = plan.items.iter().find(|item| item.path == "dropped.txt").unwrap();
    assert!(dropped.act == Act::Create && !dropped.ticked && dropped.note.as_deref().unwrap().contains("Deleted from the right"));
    assert_eq!(plan.counts().conflicts, 1);
    // Without the snapshot there's no telling, and times decide.
    let blind = plan_sync(&now, SyncKind::Update, SyncWay::Both, None, &no()).unwrap();
    assert!(blind.items.iter().all(|item| !item.conflict));
    // A file only touched, its contents the same, isn't a change.
    let dir = scratch("conflict");
    let local_dir = dir.join("left");
    std::fs::create_dir_all(&local_dir).unwrap();
    std::fs::write(local_dir.join("t.txt"), "same").unwrap();
    let local = crate::diff_engine::open_tree(&Side::Folder(local_dir.clone()), &Default::default()).unwrap();
    let mirror = Arc::new(MemoryTree::writable_with(&[("t.txt", "same", None)]));
    let result = compare_trees(Side::Folder(local_dir.clone()), Side::Folder(PathBuf::from("memory")), local.clone(), mirror.clone(), Mode::Thorough, &Filters::default(), &Progress::default(), &no()).unwrap();
    let snapshot = take_snapshot(&result, None, &Progress::default(), &no());
    assert!(snapshot.files["t.txt"].hash.is_some(), "a file here has its fingerprint kept");
    let file = std::fs::OpenOptions::new().write(true).open(local_dir.join("t.txt")).unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(5_000_000)).unwrap();
    drop(file);
    let entry = snapshot.files["t.txt"];
    let now = local.stat("t.txt").unwrap().unwrap();
    assert!(!changed_since(&*local, "t.txt", entry.left, entry.hash, now, &no()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_snapshot_keeps_what_it_knew_of_files_still_not_alike() {
    let (left, right) = pair(&[("a.txt", "x", Some(1)), ("b.txt", "y", Some(1))], &[("a.txt", "x", Some(1)), ("b.txt", "y", Some(1))]);
    let first = take_snapshot(&compare(&left, &right), None, &Progress::default(), &no());
    right.write("b.txt", b"changed", Some(9)).unwrap();
    let second = take_snapshot(&compare(&left, &right), Some(&first), &Progress::default(), &no());
    assert_eq!(second.files.get("b.txt"), first.files.get("b.txt"), "a conflict left alone is still known the next time");
    left.remove_file("b.txt").unwrap();
    right.remove_file("b.txt").unwrap();
    let third = take_snapshot(&compare(&left, &right), Some(&second), &Progress::default(), &no());
    assert!(!third.files.contains_key("b.txt"), "gone from both, forgotten");
}

#[test]
fn a_file_changed_since_the_comparison_is_left_alone() {
    let (left, right) = pair(&[("a.txt", "new", Some(900)), ("b.txt", "fresh", Some(1))], &[("a.txt", "old", Some(100))]);
    let result = compare(&left, &right);
    let plan = plan_sync(&result, SyncKind::Update, SyncWay::LeftToRight, None, &no()).unwrap();
    right.write("a.txt", b"edited meanwhile", Some(950)).unwrap();
    right.write("b.txt", b"appeared", Some(2)).unwrap();
    let dir = scratch("changed");
    let report = run_sync(&plan, &*left, &*right, &dir, &Progress::default(), &no());
    assert_eq!(report.failed.len(), 2, "{report:?}");
    assert_eq!(right.get("a.txt").unwrap().0, "edited meanwhile");
    assert_eq!(right.get("b.txt").unwrap().0, "appeared");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_is_written_outside_the_folder() {
    let dir = scratch("outside");
    let (inside, outside) = (dir.join("inside"), dir.join("outside"));
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let tree = crate::diff_engine::open_tree(&Side::Folder(inside.clone()), &Default::default()).unwrap();
    for path in ["../outside/x.txt", "a/../../outside/x.txt", "/etc/x", "", "a//b", "a\\..\\b"] {
        assert!(tree.write(path, b"no", None).is_err(), "{path}");
        assert!(tree.make_folder(path).is_err(), "{path}");
    }
    // A link inside that leads outside isn't followed.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, inside.join("link")).unwrap();
        assert!(tree.write("link/x.txt", b"no", None).is_err());
        assert!(tree.remove_file("link").is_err());
    }
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    // A sync item with such a path is refused too, and the backup folder is never left by one.
    let source = MemoryTree::writable_with(&[("x.txt", "x", None)]);
    let plan = SyncPlan { kind: SyncKind::Update, way: SyncWay::LeftToRight, skipped: Vec::new(), items: vec![SyncItem { path: "../outside/x.txt".into(), act: Act::Replace, to_right: true, ticked: true, conflict: false, note: None, size: 1, modified: None, expected: None }] };
    let report = run_sync(&plan, &source, &*tree, &dir.join("backups"), &Progress::default(), &no());
    assert_eq!(report.failed.len(), 1);
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_real_folder_syncs_with_backups_beside_the_settings() {
    let dir = scratch("real");
    let (l, r) = (dir.join("l"), dir.join("r"));
    std::fs::create_dir_all(l.join("docs")).unwrap();
    std::fs::create_dir_all(r.join("extra")).unwrap();
    std::fs::write(l.join("docs/a.txt"), "A").unwrap();
    std::fs::write(r.join("extra/z.txt"), "Z").unwrap();
    std::fs::write(r.join("left-out.log"), "not compared").unwrap();
    let (lt, rt) = (crate::diff_engine::open_tree(&Side::Folder(l.clone()), &Default::default()).unwrap(), crate::diff_engine::open_tree(&Side::Folder(r.clone()), &Default::default()).unwrap());
    let filters = Filters { exclude: "*.log".into(), ..Filters::default() };
    let result = compare_trees(Side::Folder(l.clone()), Side::Folder(r.clone()), lt.clone(), rt.clone(), Mode::Quick, &filters, &Progress::default(), &no()).unwrap();
    let plan = plan_sync(&result, SyncKind::Mirror, SyncWay::LeftToRight, None, &no()).unwrap();
    let backups = dir.join("backups");
    let report = run_sync(&plan, &*lt, &*rt, &backups, &Progress::default(), &no());
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!(std::fs::read_to_string(r.join("docs/a.txt")).unwrap(), "A");
    assert!(!r.join("extra").exists());
    assert_eq!(std::fs::read_to_string(backups.join("right/extra/z.txt")).unwrap(), "Z");
    assert!(r.join("left-out.log").exists(), "what wasn't compared is never deleted");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rules_skip_files_and_decide_how_pairs_compare() {
    let mut rules = Rules::usual();
    rules.list.insert(0, Rule { patterns: "*.txt".into(), kind: RuleKind::Text, text: crate::diff_engine::TextOptions { ignore_whitespace: true, ..Default::default() }, ignore_lines: r"^Stamp:".into() });
    rules.list.insert(0, Rule::new(RuleKind::SizeAndTime, "*.dat"));
    let left = Arc::new(MemoryTree::with(&[("a.txt", "x y\nStamp: 1\n", Some(1)), ("b.dat", "1234", Some(10)), ("main.o", "obj", Some(1)), ("target/out", "o", Some(1)), ("c.txt", "real", Some(1))]));
    let right = Arc::new(MemoryTree::with(&[("a.txt", "xy\nStamp: 2222\n", Some(50)), ("b.dat", "abcd", Some(10)), ("c.txt", "change", Some(1))]));
    let side = || Side::Folder(PathBuf::from("memory"));
    for mode in [Mode::Quick, Mode::Thorough] {
        let filters = Filters { rules: rules.clone(), ..Filters::default() };
        let result = compare_trees(side(), side(), left.clone(), right.clone(), mode, &filters, &Progress::default(), &no()).unwrap();
        let status = |path: &str| result.rows.iter().find(|row| row.path == path).map(|row| row.status);
        assert_eq!(status("a.txt"), Some(Status::Same), "spaces and the stamp line left out ({mode:?})");
        assert_eq!(status("b.dat"), Some(Status::Same), "size and time only, never read ({mode:?})");
        assert_eq!(status("c.txt"), Some(Status::Different));
        assert_eq!(status("main.o"), None, "always left out");
        assert_eq!(status("target"), None);
    }
}

// ---------- A pretend server ----------

/// An SFTP server in memory: paths are whole, from /.
#[derive(Default)]
struct FakeServer {
    files: Arc<Mutex<BTreeMap<String, (Vec<u8>, u64)>>>,
    folders: Arc<Mutex<std::collections::BTreeSet<String>>>,
}

impl RemoteFs for FakeServer {
    fn home(&self) -> Result<String, String> {
        Ok("/home/me".into())
    }
    fn list(&self, folder: &str) -> Result<Vec<Entry>, String> {
        if !self.folders.lock().unwrap().contains(folder) {
            return Err(crate::i18n::tf!("{folder} couldn't be listed: no such folder"));
        }
        let prefix = format!("{folder}/");
        let mut out = Vec::new();
        for path in self.folders.lock().unwrap().iter() {
            if let Some(name) = path.strip_prefix(&prefix).filter(|name| !name.contains('/')) {
                out.push(Entry { name: name.into(), folder: true, size: 0, modified: 0 });
            }
        }
        for (path, (bytes, time)) in self.files.lock().unwrap().iter() {
            if let Some(name) = path.strip_prefix(&prefix).filter(|name| !name.contains('/')) {
                out.push(Entry { name: name.into(), folder: false, size: bytes.len() as u64, modified: *time });
            }
        }
        Ok(out)
    }
    fn look(&self, path: &str) -> Result<Option<(bool, Stamp)>, String> {
        if self.folders.lock().unwrap().contains(path) {
            return Ok(Some((true, Stamp::default())));
        }
        Ok(self.files.lock().unwrap().get(path).map(|(bytes, time)| (false, Stamp { size: bytes.len() as u64, modified: *time })))
    }
    fn download(&self, path: &str, local: &Path) -> Result<(), String> {
        let bytes = self.files.lock().unwrap().get(path).map(|(bytes, _)| bytes.clone()).ok_or(crate::i18n::tr("no such file"))?;
        std::fs::write(local, bytes).map_err(|e| e.to_string())
    }
    fn upload(&self, local: &Path, path: &str) -> Result<(), String> {
        let parent = crate::remote::parent(path);
        if !self.folders.lock().unwrap().contains(&parent) {
            return Err(crate::i18n::tf!("{parent} isn't there"));
        }
        let bytes = std::fs::read(local).map_err(|e| e.to_string())?;
        self.files.lock().unwrap().insert(path.into(), (bytes, 0));
        Ok(())
    }
    fn set_modified(&self, path: &str, time: u64) -> Result<(), String> {
        self.files.lock().unwrap().get_mut(path).ok_or(crate::i18n::tr("no such file"))?.1 = time;
        Ok(())
    }
    fn make_folder(&self, path: &str) -> Result<(), String> {
        self.folders.lock().unwrap().insert(path.into());
        Ok(())
    }
    fn remove_file(&self, path: &str) -> Result<(), String> {
        self.files.lock().unwrap().remove(path).map(|_| ()).ok_or_else(|| "no such file".into())
    }
    fn remove_folder(&self, path: &str) -> Result<(), String> {
        self.folders.lock().unwrap().remove(path);
        Ok(())
    }
}

#[test]
fn a_folder_on_a_server_is_compared_and_synced_into() {
    let server = FakeServer::default();
    let (files, folders) = (server.files.clone(), server.folders.clone());
    for folder in ["/srv", "/srv/www", "/srv/www/old", "/home/me"] {
        folders.lock().unwrap().insert(folder.into());
    }
    files.lock().unwrap().insert("/srv/www/index.html".into(), (b"old page".to_vec(), 100));
    files.lock().unwrap().insert("/srv/www/old/stale.css".into(), (b"stale".to_vec(), 100));
    let site = Site { host: "example.com".into(), port: 22, user: "me".into(), ..Site::default() };
    let remote: Arc<dyn Tree> = Arc::new(SftpTree::with(Box::new(server), &site, "/srv/www").unwrap());
    let local = Arc::new(MemoryTree::with(&[("index.html", "new page", Some(500)), ("css/site.css", "body{}", Some(500))]));
    let sftp = Side::Sftp { site: site.clone(), folder: "/srv/www".into() };
    assert!(sftp.writable(), "a folder on a server can be written to");
    let result = compare_trees(Side::Folder(PathBuf::from("memory")), sftp, local.clone(), remote.clone(), Mode::Quick, &Filters::default(), &Progress::default(), &no()).unwrap();
    assert_eq!(result.counts().left_only, 1);
    assert_eq!(result.rows.iter().find(|row| row.path == "index.html").unwrap().status, Status::NewerLeft);
    let plan = plan_sync(&result, SyncKind::Mirror, SyncWay::LeftToRight, None, &no()).unwrap();
    assert_eq!(plan.deletions(), 2);
    let dir = scratch("server");
    let report = run_sync(&plan, &*local, &*remote, &dir, &Progress::default(), &no());
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!(files.lock().unwrap().get("/srv/www/index.html").cloned(), Some((b"new page".to_vec(), 500)), "sent whole, with its time");
    assert_eq!(files.lock().unwrap().get("/srv/www/css/site.css").map(|f| f.0.clone()), Some(b"body{}".to_vec()));
    assert!(folders.lock().unwrap().contains("/srv/www/css"));
    assert!(!files.lock().unwrap().contains_key("/srv/www/old/stale.css"));
    assert!(!folders.lock().unwrap().contains("/srv/www/old"));
    assert_eq!(std::fs::read_to_string(dir.join("right/index.html")).unwrap(), "old page", "fetched and kept before it was replaced");
    assert_eq!(std::fs::read_to_string(dir.join("right/old/stale.css")).unwrap(), "stale");
    // The home folder is where a side with no folder starts.
    let home = SftpTree::with(Box::new(FakeServer { files: files.clone(), folders: folders.clone() }), &site, "").unwrap();
    assert!(home.list("").unwrap().is_empty());
    assert!(home.write("../escape", b"x", None).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn git_revisions_and_images_are_refused_as_targets() {
    let left = Arc::new(MemoryTree::writable_with(&[("a.txt", "a", Some(1))]));
    let right = Arc::new(MemoryTree::writable_with(&[]));
    let image = Side::Image { file: PathBuf::from("/disks/game.adf"), folder: String::new() };
    let result = compare_trees(Side::Folder(PathBuf::from("memory")), image, left, right, Mode::Quick, &Filters::default(), &Progress::default(), &no()).unwrap();
    let refused = plan_sync(&result, SyncKind::Update, SyncWay::LeftToRight, None, &no()).unwrap_err();
    assert!(refused.contains("game.adf") && refused.contains("tab of its own"), "{refused}");
    assert!(plan_sync(&result, SyncKind::Update, SyncWay::RightToLeft, None, &no()).is_ok(), "the folder side can still be written to");
    let git = Side::Git { repo: PathBuf::from("/repo"), revision: "HEAD".into(), folder: String::new() };
    assert!(git.refusal().unwrap().contains("history"));
}
