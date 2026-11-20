//! A folder on a server opened as a project, as Sublime SFTP and VS Code's SFTP extension do: the folder is
//! fetched into a copy on this computer (a mirror), and the project is opened on that, so the Explorer, Go to
//! file, Search, language servers and everything else work on it as on any folder. Each save is sent back at once;
//! the server's changes are fetched now and then, and when asked.
//!
//! The work is the folder comparison and sync's: the mirror and the server are compared as two sides, and synced
//! by updating both ways with the last sync remembered, so a file changed on both sides since then is a conflict,
//! left for the person to decide. What .gitignore files leave out is left on the server, and files larger than
//! `LARGE` are left there until they're opened, with an empty stand-in in the mirror so they can be seen.

use crate::compare_sync::{self, Act, SnapEntry, SnapMeta, Snapshot, SyncPlan, SyncReport};
use crate::diff_engine::{FolderResult, Filters, Meta, Mode, Progress, Side, Tree, compare_trees, same_size_and_time};
use crate::remote::Site;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Files bigger than this are left on the server until they're opened.
pub const LARGE: u64 = 4 * 1024 * 1024;
/// The most a file may be for its fingerprint to be kept, to tell a file only touched from one changed.
const MOST_FINGERPRINTED: u64 = 16 * 1024 * 1024;

/// Where a remote project's files are, on the server.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Origin {
    pub site: Site,
    /// The folder on the server, written whole.
    pub folder: String,
}

impl Origin {
    pub fn url(&self) -> String {
        self.site.url(&self.folder)
    }

    /// The folder's own name, or the server's for its top.
    pub fn name(&self) -> String {
        self.folder.trim_end_matches('/').rsplit('/').next().filter(|name| !name.is_empty()).map_or_else(|| self.site.host.clone(), str::to_string)
    }

    pub fn to_json(&self) -> Value {
        let mut value = crate::compare_session::site_to_json(&self.site);
        value["folder"] = json!(self.folder);
        value
    }

    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Self { site: crate::compare_session::site_from_json(value)?, folder: value.get("folder")?.as_str()?.to_string() })
    }
}

/// Where a remote project's mirror is kept, under a folder of them all: the files, in a folder named as the one
/// on the server; beside them (outside the project) what's known about them, and the backups the syncs keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    pub files: PathBuf,
    pub state: PathBuf,
    pub backups: PathBuf,
}

pub fn places(base: &Path, origin: &Origin) -> Places {
    let digest: String = Sha256::digest(origin.url().as_bytes()).iter().take(6).map(|b| format!("{b:02x}")).collect();
    let tidy: String = origin.name().chars().map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    let folder = base.join(format!("{tidy}-{}-{digest}", origin.site.host.replace([':', '[', ']'], "_")));
    Places { files: folder.join(&tidy), state: folder.join("mirror.json"), backups: folder.join("backups") }
}

/// What's known about a mirror between syncs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MirrorState {
    pub origin: Origin,
    /// Each file as it was on both sides when they were last alike.
    pub snapshot: Snapshot,
    /// Files left on the server until they're opened, with their size and time there. Each has an empty stand-in
    /// in the mirror.
    pub left_there: BTreeMap<String, Meta>,
}

impl MirrorState {
    pub fn new(origin: Origin) -> Self {
        Self { origin, ..Self::default() }
    }

    pub fn to_text(&self) -> String {
        let left_there: serde_json::Map<String, Value> = self.left_there.iter().map(|(path, meta)| (path.clone(), json!([meta.size, meta.modified]))).collect();
        let value = json!({ "limoniter_mirror": 1, "origin": self.origin.to_json(), "last_sync": crate::compare_session::snapshot_to_json(&self.snapshot), "left_there": left_there });
        serde_json::to_string_pretty(&value).unwrap_or_default() + "\n"
    }

    pub fn from_text(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        let origin = Origin::from_json(value.get("origin")?)?;
        let snapshot = value.get("last_sync").and_then(crate::compare_session::snapshot_from_json).unwrap_or_default();
        let left_there = value
            .get("left_there")
            .and_then(Value::as_object)
            .map(|map| map.iter().filter_map(|(path, meta)| Some((path.clone(), Meta { is_dir: false, size: meta.get(0)?.as_u64()?, modified: meta.get(1).and_then(Value::as_u64) }))).collect())
            .unwrap_or_default();
        Some(Self { origin, snapshot, left_there })
    }

    /// Reads what's kept for a mirror, or starts afresh if nothing is (or it's for another place).
    pub fn load(places: &Places, origin: &Origin) -> Self {
        std::fs::read_to_string(&places.state).ok().and_then(|text| Self::from_text(&text)).filter(|state| state.origin == *origin).unwrap_or_else(|| Self::new(origin.clone()))
    }

    pub fn save(&self, places: &Places) -> Result<(), String> {
        crate::codec::write_file(&places.state, self.to_text().as_bytes()).map_err(|e| e.to_string())
    }

    /// Whether a file in the mirror is only a stand-in for one left on the server.
    pub fn is_left_there(&self, path: &str) -> bool {
        self.left_there.contains_key(path)
    }
}

/// A file's path in the mirror as the comparison writes it: from the top, with forward slashes.
pub fn relative(root: &Path, file: &Path) -> Option<String> {
    let rest = file.strip_prefix(root).ok()?;
    let parts: Vec<String> = rest.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The other way: where a path from the top is in the mirror.
pub fn local_place(root: &Path, path: &str) -> PathBuf {
    path.split('/').filter(|part| !part.is_empty()).fold(root.to_path_buf(), |at, part| at.join(part))
}

fn snap(meta: Meta) -> SnapMeta {
    SnapMeta { size: meta.size, modified: meta.modified }
}

fn as_meta(snap: SnapMeta) -> Meta {
    Meta { is_dir: false, size: snap.size, modified: snap.modified }
}

/// Compares the mirror with the server: by size and time, leaving out what .gitignore files do.
pub fn compare(local: Arc<dyn Tree>, remote: Arc<dyn Tree>, root: &Path, origin: &Origin, progress: &Progress, cancel: &AtomicBool) -> Result<FolderResult, String> {
    let filters = Filters { gitignore: true, ..Filters::default() };
    compare_trees(Side::Folder(root.to_path_buf()), Side::Sftp { site: origin.site.clone(), folder: origin.folder.clone() }, local, remote, Mode::Quick, &filters, progress, cancel)
}

/// Brings what's known of files left on the server up to date from a comparison: one gone from the server goes
/// from here too; one whose stand-in has been written to here is a file of the mirror's own now.
fn settle_left_there(result: &FolderResult, state: &mut MirrorState) {
    let rows: BTreeMap<&str, &crate::diff_engine::Row> = result.rows.iter().map(|row| (row.path.as_str(), row)).collect();
    let local = &result.left_tree;
    state.left_there.retain(|path, kept| {
        let Some(row) = rows.get(path.as_str()) else {
            // Neither side lists it now (left out by a .gitignore, perhaps): forgotten, stand-in and all.
            if local.stat(path).ok().flatten().is_some_and(|meta| !meta.is_dir && meta.size == 0) {
                let _ = local.remove_file(path);
            }
            return false;
        };
        match (row.left, row.right) {
            (Some(here), Some(there)) if !here.is_dir && here.size == 0 && !there.is_dir => {
                *kept = there;
                true
            }
            (Some(here), None) if !here.is_dir && here.size == 0 => {
                let _ = local.remove_file(path);
                false
            }
            _ => false,
        }
    });
}

/// Remembers each file that's alike on both sides now, as the folder sync does after syncing. One remembered
/// already with the same size and times keeps its fingerprint, so nothing is read again.
fn remember_alike(result: &FolderResult, state: &mut MirrorState, cancel: &AtomicBool) {
    for row in &result.rows {
        if row.is_dir() || row.status != crate::diff_engine::Status::Same || state.left_there.contains_key(&row.path) {
            continue;
        }
        let (Some(here), Some(there)) = (row.left, row.right) else { continue };
        let entry = SnapEntry { left: snap(here), right: snap(there), hash: None };
        if state.snapshot.files.get(&row.path).is_some_and(|known| known.left == entry.left && known.right == entry.right) {
            continue;
        }
        let hash = (here.size <= MOST_FINGERPRINTED).then(|| result.left_tree.hash(&row.path, cancel).ok()).flatten();
        state.snapshot.files.insert(row.path.clone(), SnapEntry { hash, ..entry });
    }
    state.snapshot.taken = chrono::Utc::now().timestamp().max(0) as u64;
}

/// Whether a file is as it was on both sides at the last sync, by size and time.
fn unchanged_on_both(result: &FolderResult, state: &MirrorState, path: &str) -> bool {
    let Some(known) = state.snapshot.files.get(path) else { return false };
    let Some(row) = result.rows.iter().find(|row| row.path == path) else { return false };
    match (row.left, row.right) {
        (Some(here), Some(there)) => same_size_and_time(as_meta(known.left), here) && same_size_and_time(as_meta(known.right), there),
        _ => false,
    }
}

/// Compares, and works out what a sync would do: an update both ways, as the folder sync plans it, with the
/// last sync remembered. Files left on the server aren't in it, and nor is a file alike on both sides at the last
/// sync and changed on neither since, even if their times disagree (a server that can't set a file's time).
pub fn refresh(local: Arc<dyn Tree>, remote: Arc<dyn Tree>, root: &Path, state: &mut MirrorState, progress: &Progress, cancel: &AtomicBool) -> Result<SyncPlan, String> {
    let result = compare(local, remote, root, &state.origin, progress, cancel)?;
    settle_left_there(&result, state);
    remember_alike(&result, state, cancel);
    let mut plan = compare_sync::plan_sync(&result, compare_sync::SyncKind::Update, compare_sync::SyncWay::Both, Some(&state.snapshot), cancel)?;
    plan.items.retain(|item| !state.left_there.contains_key(&item.path) && !(item.act == Act::Replace && unchanged_on_both(&result, state, &item.path)));
    plan.skipped.retain(|(path, _)| !state.left_there.contains_key(path));
    for item in &mut plan.items {
        item.note = item.note.take().map(|note| in_server_words(&note));
    }
    Ok(plan)
}

/// A sync's note about a file, which speaks of the left and right sides, said of this computer and the server.
fn in_server_words(note: &str) -> String {
    let said = match note {
        "Deleted from the right since the last sync" => crate::i18n::tr("Deleted from the server since the last sync"),
        "Deleted from the left since the last sync" => crate::i18n::tr("Deleted here since the last sync"),
        "Changed on the left and deleted from the right since the last sync" => crate::i18n::tr("Changed here, and deleted from the server, since the last sync"),
        "Changed on the right and deleted from the left since the last sync" => crate::i18n::tr("Changed on the server, and deleted here, since the last sync"),
        "Changed on both sides since the last sync" => crate::i18n::tr("Changed here and on the server since the last sync"),
        other => crate::i18n::tr(other),
    };
    said.to_string()
}

/// What can be fetched from the server without asking anyone: new and changed files and new folders there,
/// that haven't changed here too.
pub fn fetch_part(plan: &SyncPlan) -> SyncPlan {
    SyncPlan { items: plan.items.iter().filter(|item| !item.to_right && item.ticked && !item.conflict && matches!(item.act, Act::Create | Act::Replace | Act::MakeFolder)).cloned().collect(), skipped: Vec::new(), ..plan.clone() }
}

/// What a plan still has once part of it has been done.
pub fn without(plan: &SyncPlan, done: &SyncPlan) -> SyncPlan {
    SyncPlan { items: plan.items.iter().filter(|item| !done.items.iter().any(|d| d.path == item.path && d.to_right == item.to_right)).cloned().collect(), ..plan.clone() }
}

/// Puts a file into the snapshot as both sides have it now.
fn remember_now(local: &dyn Tree, remote: &dyn Tree, path: &str, state: &mut MirrorState, cancel: &AtomicBool) {
    match (local.stat(path), remote.stat(path)) {
        (Ok(Some(here)), Ok(Some(there))) if !here.is_dir && !there.is_dir => {
            let hash = (here.size <= MOST_FINGERPRINTED).then(|| local.hash(path, cancel).ok()).flatten();
            state.snapshot.files.insert(path.to_string(), SnapEntry { left: snap(here), right: snap(there), hash });
        }
        _ => {
            state.snapshot.files.remove(path);
        }
    }
}

/// Does what's ticked in a plan, as a sync does, keeping each file replaced or taken away in `backups` first.
/// A large new file on the server is left there, with an empty stand-in here. Each file done is remembered as
/// both sides have it now.
pub fn carry_out(plan: &SyncPlan, local: &dyn Tree, remote: &dyn Tree, state: &mut MirrorState, backups: &Path, progress: &Progress, cancel: &AtomicBool) -> SyncReport {
    let mut rest = plan.clone();
    rest.items.retain(|item| {
        let large = item.ticked && item.act == Act::Create && !item.to_right && item.size > LARGE;
        if large && local.stat(&item.path).ok().flatten().is_none() && local.write(&item.path, b"", None).is_ok() {
            state.left_there.insert(item.path.clone(), Meta { is_dir: false, size: item.size, modified: item.modified });
        }
        !large
    });
    let backups = backups.join(chrono::Local::now().format("%Y-%m-%d %H.%M.%S").to_string());
    let report = compare_sync::run_sync(&rest, local, remote, &backups, progress, cancel);
    for item in rest.items.iter().filter(|item| item.ticked && !report.failed.iter().any(|(path, _)| *path == item.path)) {
        match item.act {
            Act::Create | Act::Replace => remember_now(local, remote, &item.path, state, cancel),
            Act::Delete => {
                state.snapshot.files.remove(&item.path);
            }
            Act::MakeFolder | Act::RemoveFolder => {}
        }
    }
    report
}

/// Why a saved file wasn't sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushError {
    /// The file on the server has changed (or gone, or appeared) since the mirror last had it as it was there.
    ChangedOnServer,
    Failed(String),
}

/// Sends a file saved in the mirror to the server, unless the server's copy has changed since the mirror last
/// matched it (or `force`). It's written beside the old one there first, then put in its place.
pub fn push(path: &str, local: &dyn Tree, remote: &dyn Tree, state: &mut MirrorState, force: bool, cancel: &AtomicBool) -> Result<(), PushError> {
    let failed = PushError::Failed;
    let here = local.stat(path).map_err(failed)?.filter(|meta| !meta.is_dir).ok_or_else(|| PushError::Failed(crate::i18n::tf!("{path} isn't in the project's folder.")))?;
    if !force {
        let there = remote.stat(path).map_err(failed)?;
        let expected = state.snapshot.files.get(path).map(|known| as_meta(known.right));
        let unchanged = match (expected, there) {
            (Some(expected), Some(now)) => !now.is_dir && same_size_and_time(expected, now),
            (None, None) => true,
            _ => false,
        };
        if !unchanged {
            return Err(PushError::ChangedOnServer);
        }
    }
    let bytes = local.read(path).map_err(failed)?;
    remote.write(path, &bytes, here.modified).map_err(failed)?;
    state.left_there.remove(path);
    remember_now(local, remote, path, state, cancel);
    Ok(())
}

/// Fetches one file from the server into the mirror: a file left there, to be opened.
pub fn fetch(path: &str, local: &dyn Tree, remote: &dyn Tree, state: &mut MirrorState, cancel: &AtomicBool) -> Result<(), String> {
    let there = remote.stat(path)?.filter(|meta| !meta.is_dir).ok_or_else(|| crate::i18n::tf!("{path} isn't on the server any more."))?;
    let bytes = remote.read(path)?;
    local.write(path, &bytes, there.modified)?;
    state.left_there.remove(path);
    remember_now(local, remote, path, state, cancel);
    Ok(())
}

/// Renames a file or folder on the server, as it was renamed here, and what's remembered of it with it.
pub fn rename(from: &str, to: &str, remote: &dyn Tree, state: &mut MirrorState) -> Result<(), String> {
    remote.rename(from, to)?;
    let inside = format!("{from}/");
    let moved = |map_keys: Vec<String>| -> Vec<(String, String)> { map_keys.into_iter().filter(|path| path == from || path.starts_with(&inside)).map(|path| (path.clone(), format!("{to}{}", &path[from.len()..]))).collect() };
    for (old, new) in moved(state.snapshot.files.keys().cloned().collect()) {
        if let Some(entry) = state.snapshot.files.remove(&old) {
            state.snapshot.files.insert(new, entry);
        }
    }
    for (old, new) in moved(state.left_there.keys().cloned().collect()) {
        if let Some(meta) = state.left_there.remove(&old) {
            state.left_there.insert(new, meta);
        }
    }
    Ok(())
}

/// Takes a file, or a folder and everything in it, away from the server, as it was here.
pub fn remove(path: &str, remote: &dyn Tree, state: &mut MirrorState) -> Result<(), String> {
    fn remove_tree(remote: &dyn Tree, path: &str) -> Result<(), String> {
        match remote.stat(path)? {
            None => Ok(()),
            Some(meta) if !meta.is_dir => remote.remove_file(path),
            Some(_) => {
                for item in remote.list(path)? {
                    remove_tree(remote, &format!("{path}/{}", item.name))?;
                }
                remote.remove_folder(path)
            }
        }
    }
    remove_tree(remote, path)?;
    let inside = format!("{path}/");
    state.snapshot.files.retain(|known, _| known != path && !known.starts_with(&inside));
    state.left_there.retain(|known, _| known != path && !known.starts_with(&inside));
    Ok(())
}

#[cfg(test)]
#[path = "remote_project_tests.rs"]
mod remote_project_tests;
