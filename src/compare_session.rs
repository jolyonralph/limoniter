//! Saved comparisons: a folder comparison kept by name, with both its sides, how it compares, its filters and
//! rules, and which way it syncs, so it can be opened again as it was. Each also keeps how both sides were after
//! its last sync, for telling a conflict. Those belonging to a project are kept in the project's file, with paths
//! relative to it; the rest in a file of their own beside the settings.

use crate::compare_rules::Rules;
use crate::compare_sync::{SnapEntry, SnapMeta, Snapshot, SyncKind, SyncWay};
use crate::diff_engine::{Filters, Mode, Side};
use crate::remote::Site;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The name of the file the comparisons not in a project are kept in.
pub const FILE_NAME: &str = "compare-sessions.json";

/// A folder comparison, kept by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub name: String,
    pub left: Side,
    pub right: Side,
    pub mode: Mode,
    /// Its filters, and its own rules for how files are compared.
    pub filters: Filters,
    pub hide_same: bool,
    pub sync_kind: SyncKind,
    pub sync_way: SyncWay,
    /// How both sides were after the last sync, if there's been one.
    pub snapshot: Option<Snapshot>,
}

fn path_out(path: &Path, base: Option<&Path>) -> String {
    match base {
        Some(base) => crate::project::stored(base, path),
        None => path.display().to_string(),
    }
}

fn path_in(text: &str, base: Option<&Path>) -> PathBuf {
    match base {
        Some(base) => crate::project::resolved(base, text),
        None => PathBuf::from(text),
    }
}

fn side_to_json(side: &Side, base: Option<&Path>) -> Value {
    match side {
        Side::Folder(path) => json!({ "kind": "folder", "path": path_out(path, base) }),
        Side::Git { repo, revision, folder } => json!({ "kind": "git", "repo": path_out(repo, base), "revision": revision, "folder": folder }),
        Side::Image { file, folder } => json!({ "kind": "image", "file": path_out(file, base), "folder": folder }),
        // Never a password: that stays in the system's credential store.
        Side::Sftp { site, folder } => {
            let mut value = site_to_json(site);
            value["kind"] = json!("sftp");
            value["folder"] = json!(folder);
            value
        }
    }
}

/// A site as it's kept in a file: how it's reached, where and as whom. Never a password: that stays in the
/// system's credential store.
pub(crate) fn site_to_json(site: &Site) -> Value {
    json!({ "scheme": site.scheme.prefix(), "host": site.host, "port": site.port, "user": site.user, "key_file": site.key_file.as_ref().map(|key| key.display().to_string()) })
}

pub(crate) fn site_from_json(value: &Value) -> Option<Site> {
    let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_string);
    let scheme = text("scheme").and_then(|prefix| crate::remote::Scheme::from_prefix(&prefix)).unwrap_or_default();
    let port = value.get("port").and_then(Value::as_u64).and_then(|port| u16::try_from(port).ok()).unwrap_or(scheme.default_port());
    Some(Site { scheme, host: text("host")?, port, user: text("user")?, key_file: text("key_file").map(PathBuf::from) })
}

fn side_from_json(value: &Value, base: Option<&Path>) -> Option<Side> {
    let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_string);
    Some(match value.get("kind")?.as_str()? {
        "folder" => Side::Folder(path_in(&text("path")?, base)),
        "git" => Side::Git { repo: path_in(&text("repo")?, base), revision: text("revision").unwrap_or_else(|| "HEAD".into()), folder: text("folder").unwrap_or_default() },
        "image" => Side::Image { file: path_in(&text("file")?, base), folder: text("folder").unwrap_or_default() },
        "sftp" => Side::Sftp { site: site_from_json(value)?, folder: text("folder").unwrap_or_default() },
        _ => return None,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

pub(crate) fn snapshot_to_json(snapshot: &Snapshot) -> Value {
    let files: serde_json::Map<String, Value> = snapshot.files.iter().map(|(path, entry)| (path.clone(), json!([entry.left.size, entry.left.modified, entry.right.size, entry.right.modified, entry.hash.map(|h| hex(&h))]))).collect();
    json!({ "taken": snapshot.taken, "files": files })
}

pub(crate) fn snapshot_from_json(value: &Value) -> Option<Snapshot> {
    let mut snapshot = Snapshot { taken: value.get("taken")?.as_u64()?, ..Snapshot::default() };
    for (path, entry) in value.get("files")?.as_object()? {
        let Some(parts) = entry.as_array().filter(|parts| parts.len() == 5) else { continue };
        let (Some(left_size), Some(right_size)) = (parts[0].as_u64(), parts[2].as_u64()) else { continue };
        snapshot.files.insert(
            path.clone(),
            SnapEntry { left: SnapMeta { size: left_size, modified: parts[1].as_u64() }, right: SnapMeta { size: right_size, modified: parts[3].as_u64() }, hash: parts[4].as_str().and_then(unhex) },
        );
    }
    Some(snapshot)
}

impl Session {
    /// As it's kept: with paths relative to `base` when it's in a project.
    pub fn to_json(&self, base: Option<&Path>) -> Value {
        json!({
            "name": self.name,
            "left": side_to_json(&self.left, base),
            "right": side_to_json(&self.right, base),
            "mode": if self.mode == Mode::Thorough { "contents" } else { "quick" },
            "include": self.filters.include,
            "exclude": self.filters.exclude,
            "gitignore": self.filters.gitignore,
            "rules": self.filters.rules.to_json(),
            "hide_same": self.hide_same,
            "sync_kind": self.sync_kind.id(),
            "sync_way": self.sync_way.id(),
            "last_sync": self.snapshot.as_ref().map(snapshot_to_json),
        })
    }

    pub fn from_json(value: &Value, base: Option<&Path>) -> Option<Self> {
        let text = |name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
        let flag = |name: &str| value.get(name).and_then(Value::as_bool).unwrap_or(false);
        let name = text("name");
        if name.trim().is_empty() {
            return None;
        }
        Some(Self {
            name,
            left: side_from_json(value.get("left")?, base)?,
            right: side_from_json(value.get("right")?, base)?,
            mode: if text("mode") == "contents" { Mode::Thorough } else { Mode::Quick },
            filters: Filters { include: text("include"), exclude: text("exclude"), gitignore: flag("gitignore"), rules: value.get("rules").map_or_else(Rules::usual, Rules::from_json) },
            hide_same: flag("hide_same"),
            sync_kind: SyncKind::from_id(&text("sync_kind")),
            sync_way: SyncWay::from_id(&text("sync_way")),
            snapshot: value.get("last_sync").and_then(snapshot_from_json),
        })
    }
}

/// A list of comparisons as it's kept, with paths relative to `base` if given.
pub fn list_to_json(sessions: &[Session], base: Option<&Path>) -> Value {
    Value::Array(sessions.iter().map(|session| session.to_json(base)).collect())
}

pub fn list_from_json(value: Option<&Value>, base: Option<&Path>) -> Vec<Session> {
    value.and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(|item| Session::from_json(item, base)).collect())
}

/// The comparisons kept in a file of their own; none if it isn't there or can't be read.
pub fn load(file: &Path) -> Vec<Session> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let value: Option<Value> = serde_json::from_str(&text).ok();
    list_from_json(value.as_ref().and_then(|value| value.get("sessions")), None)
}

pub fn save(file: &Path, sessions: &[Session]) -> Result<(), String> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&json!({ "limoniter_compare_sessions": 1, "sessions": list_to_json(sessions, None) })).unwrap_or_default() + "\n";
    crate::codec::write_file(file, text.as_bytes()).map_err(|e| e.to_string())
}

/// A name not yet taken in a list: the one asked for, or it with a number after.
pub fn free_name(wanted: &str, sessions: &[Session]) -> String {
    let wanted = wanted.trim();
    let wanted = if wanted.is_empty() { "Comparison" } else { wanted };
    let taken = |name: &str| sessions.iter().any(|session| session.name.eq_ignore_ascii_case(name));
    if !taken(wanted) {
        return wanted.to_string();
    }
    (2..).map(|n| format!("{wanted} ({n})")).find(|name| !taken(name)).unwrap_or_else(|| wanted.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare_rules::{Rule, RuleKind};

    pub(crate) fn sample(name: &str, base: &Path) -> Session {
        let mut rules = Rules::usual();
        rules.list.push(Rule::new(RuleKind::SizeAndTime, "*.log"));
        let mut snapshot = Snapshot { taken: 1_700_000_000, ..Snapshot::default() };
        snapshot.files.insert("a/b.txt".into(), SnapEntry { left: SnapMeta { size: 3, modified: Some(10) }, right: SnapMeta { size: 3, modified: None }, hash: Some([7; 32]) });
        snapshot.files.insert("c.txt".into(), SnapEntry { left: SnapMeta { size: 1, modified: Some(1) }, right: SnapMeta { size: 1, modified: Some(2) }, hash: None });
        Session {
            name: name.into(),
            left: Side::Folder(base.join("src")),
            right: Side::Sftp { site: Site { host: "example.com".into(), port: 2222, user: "me".into(), key_file: Some(PathBuf::from("/keys/id")), ..Site::default() }, folder: "/srv/www".into() },
            mode: Mode::Thorough,
            filters: Filters { include: "*.rs".into(), exclude: "*.tmp".into(), gitignore: true, rules },
            hide_same: true,
            sync_kind: SyncKind::Mirror,
            sync_way: SyncWay::RightToLeft,
            snapshot: Some(snapshot),
        }
    }

    #[test]
    fn a_session_is_kept_whole_and_never_with_a_password() {
        let base = PathBuf::from("/work/project");
        let session = sample("Site upload", &base);
        let kept = session.to_json(Some(&base));
        assert_eq!(kept["left"]["path"], "src", "a project keeps paths relative to itself");
        assert!(!kept.to_string().to_lowercase().contains("password"));
        assert_eq!(Session::from_json(&kept, Some(&base)), Some(session.clone()));
        // Moved with its project, it finds its folders in the new place.
        let moved = Session::from_json(&kept, Some(Path::new("/elsewhere/project"))).unwrap();
        assert_eq!(moved.left, Side::Folder(PathBuf::from("/elsewhere/project/src")));
        // Every kind of side.
        for side in [Side::Git { repo: base.clone(), revision: "main".into(), folder: "docs".into() }, Side::Image { file: base.join("disk.adf"), folder: "s".into() }] {
            let mut other = session.clone();
            other.left = side;
            assert_eq!(Session::from_json(&other.to_json(None), None), Some(other));
        }
        assert!(Session::from_json(&json!({ "name": "", "left": {}, "right": {} }), None).is_none());
    }

    #[test]
    fn sessions_outside_a_project_have_a_file_of_their_own() {
        let dir = std::env::temp_dir().join(format!("limoniter-sessions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join(FILE_NAME);
        assert!(load(&file).is_empty(), "none until there's a file");
        let sessions = vec![sample("One", Path::new("/a")), sample("Two", Path::new("/b"))];
        save(&file, &sessions).unwrap();
        assert_eq!(load(&file), sessions);
        assert_eq!(free_name("one", &sessions), "one (2)");
        assert_eq!(free_name("Three", &sessions), "Three");
        assert_eq!(free_name("  ", &[]), "Comparison");
        std::fs::write(&file, "not json").unwrap();
        assert!(load(&file).is_empty(), "a broken file is passed over");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
