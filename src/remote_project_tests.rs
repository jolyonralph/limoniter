//! Remote projects' mirrors: against a server held in memory, and over FTP to the test server.

use super::*;
use crate::diff_engine::{LocalTree, MemoryTree};
use crate::remote::{Scheme, Secret};
use std::sync::atomic::{AtomicU64, Ordering};

fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let folder = std::env::temp_dir().join(format!("limoniter-mirror-{name}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn origin() -> Origin {
    Origin { site: Site { host: "example.org".into(), port: 22, user: "ann".into(), ..Site::default() }, folder: "/srv/www".into() }
}

/// A server whose files are in memory. Its times are given, as a real server's are.
fn server() -> Arc<MemoryTree> {
    let tree = MemoryTree::writable_with(&[
        ("src/main.c", "int main(void) { return 0; }\n", Some(1_700_000_000)),
        ("docs/readme.txt", "Read me\n", Some(1_700_000_100)),
        (".gitignore", "build/\n*.o\n", Some(1_700_000_000)),
        ("build/out.bin", "built", Some(1_700_000_000)),
        ("main.o", "object", Some(1_700_000_000)),
    ]);
    tree.files.lock().unwrap().insert("media/big.bin".into(), (vec![7u8; LARGE as usize + 1], Some(1_700_000_200)));
    Arc::new(tree)
}

/// Fetches what can be fetched without asking, as the window does now and then.
fn fetch_now(local: &Arc<dyn Tree>, remote: &Arc<dyn Tree>, root: &Path, state: &mut MirrorState, backups: &Path) -> SyncPlan {
    let (progress, cancel) = (Progress::default(), AtomicBool::new(false));
    let plan = refresh(local.clone(), remote.clone(), root, state, &progress, &cancel).unwrap();
    let part = fetch_part(&plan);
    let report = carry_out(&part, &**local, &**remote, state, backups, &progress, &cancel);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    without(&plan, &part)
}

#[test]
fn a_server_folder_is_mirrored_keeping_to_gitignore_with_large_files_left_there() {
    let folder = scratch("first");
    let root = folder.join("www");
    std::fs::create_dir_all(&root).unwrap();
    let remote = server();
    let (local, remote_tree): (Arc<dyn Tree>, Arc<dyn Tree>) = (Arc::new(LocalTree { root: root.clone() }), remote.clone());
    let mut state = MirrorState::new(origin());
    let left = fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups"));
    assert!(left.items.is_empty(), "nothing left to decide: {:?}", left.items);
    assert_eq!(std::fs::read_to_string(root.join("src/main.c")).unwrap(), "int main(void) { return 0; }\n");
    assert_eq!(std::fs::read_to_string(root.join("docs/readme.txt")).unwrap(), "Read me\n");
    assert!(root.join(".gitignore").is_file());
    assert!(!root.join("build").exists() && !root.join("main.o").exists(), "what .gitignore leaves out stays on the server");
    // The large file is a stand-in here, and known to be one.
    assert_eq!(std::fs::metadata(root.join("media/big.bin")).unwrap().len(), 0);
    assert!(state.is_left_there("media/big.bin"));
    assert_eq!(state.left_there["media/big.bin"].size, LARGE + 1);
    // The copies have the server's times, and are remembered as alike.
    assert_eq!(local.stat("src/main.c").unwrap().unwrap().modified, Some(1_700_000_000));
    assert!(state.snapshot.files.contains_key("src/main.c") && !state.snapshot.files.contains_key("media/big.bin"));
    // Again: nothing to do, and the stand-in isn't sent over the real file.
    let again = fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups"));
    assert!(again.items.is_empty(), "{:?}", again.items);
    assert_eq!(remote.files.lock().unwrap()["media/big.bin"].0.len(), LARGE as usize + 1);
    // Opened: fetched whole.
    fetch("media/big.bin", &*local, &*remote_tree, &mut state, &AtomicBool::new(false)).unwrap();
    assert_eq!(std::fs::metadata(root.join("media/big.bin")).unwrap().len(), LARGE + 1);
    assert!(!state.is_left_there("media/big.bin") && state.snapshot.files.contains_key("media/big.bin"));
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn changes_on_the_server_are_fetched_and_saves_here_sent_with_conflicts_left_to_decide() {
    let folder = scratch("changes");
    let root = folder.join("www");
    std::fs::create_dir_all(&root).unwrap();
    let remote = server();
    let (local, remote_tree): (Arc<dyn Tree>, Arc<dyn Tree>) = (Arc::new(LocalTree { root: root.clone() }), remote.clone());
    let mut state = MirrorState::new(origin());
    fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups"));
    let cancel = AtomicBool::new(false);

    // Changed on the server, and a new file there: both fetched.
    remote.write("docs/readme.txt", b"Read me, now\n", Some(1_700_100_000)).unwrap();
    remote.write("docs/new.txt", b"New\n", Some(1_700_100_000)).unwrap();
    let left = fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups"));
    assert!(left.items.is_empty(), "{:?}", left.items);
    assert_eq!(std::fs::read_to_string(root.join("docs/readme.txt")).unwrap(), "Read me, now\n");
    assert_eq!(std::fs::read_to_string(root.join("docs/new.txt")).unwrap(), "New\n");

    // Saved here: sent at once, and then alike.
    std::fs::write(root.join("src/main.c"), "int main(void) { return 1; }\n").unwrap();
    push("src/main.c", &*local, &*remote_tree, &mut state, false, &cancel).unwrap();
    assert_eq!(remote.get("src/main.c").unwrap().0, "int main(void) { return 1; }\n");
    assert!(fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups")).items.is_empty());
    // A new file saved here goes too.
    std::fs::create_dir_all(root.join("src/lib")).unwrap();
    std::fs::write(root.join("src/lib/util.c"), "// util\n").unwrap();
    push("src/lib/util.c", &*local, &*remote_tree, &mut state, false, &cancel).unwrap();
    assert_eq!(remote.get("src/lib/util.c").unwrap().0, "// util\n");

    // Someone else changed it on the server since: the save asks first, and goes if told to.
    remote.write("src/main.c", b"theirs\n", Some(1_700_200_000)).unwrap();
    std::fs::write(root.join("src/main.c"), "mine\n").unwrap();
    assert_eq!(push("src/main.c", &*local, &*remote_tree, &mut state, false, &cancel), Err(PushError::ChangedOnServer));
    // Changed on both sides: a conflict, not fetched over the change here.
    let left = fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups"));
    assert_eq!(std::fs::read_to_string(root.join("src/main.c")).unwrap(), "mine\n");
    let conflict = left.items.iter().find(|item| item.path == "src/main.c").expect("shown to decide");
    assert!(conflict.conflict && !conflict.ticked);
    push("src/main.c", &*local, &*remote_tree, &mut state, true, &cancel).unwrap();
    assert_eq!(remote.get("src/main.c").unwrap().0, "mine\n");
    assert!(fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups")).items.is_empty());

    // Renamed and taken away here, so there too, with what's remembered following.
    rename("docs", "manual", &*remote_tree, &mut state).unwrap();
    std::fs::rename(root.join("docs"), root.join("manual")).unwrap();
    assert!(remote.get("manual/new.txt").is_some() && state.snapshot.files.contains_key("manual/new.txt") && !state.snapshot.files.contains_key("docs/new.txt"));
    remove("manual", &*remote_tree, &mut state).unwrap();
    std::fs::remove_dir_all(root.join("manual")).unwrap();
    assert!(remote.get("manual/readme.txt").is_none() && !state.snapshot.files.keys().any(|path| path.starts_with("manual/")));
    assert!(fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups")).items.is_empty());

    // Taken away on the server while the copy here is unchanged: offered, unticked, not done by itself.
    remote.remove_file("src/lib/util.c").unwrap();
    let left = fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups"));
    assert!(root.join("src/lib/util.c").is_file());
    assert!(left.items.iter().any(|item| item.path == "src/lib/util.c" && !item.ticked));
    let _ = std::fs::remove_dir_all(&folder);
}

/// A server that can't set a file's time: each file it's sent has the time it arrived.
struct NoTimes(MemoryTree, AtomicU64);

impl Tree for NoTimes {
    fn list(&self, folder: &str) -> Result<Vec<crate::diff_engine::Item>, String> {
        self.0.list(folder)
    }
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        self.0.read(path)
    }
    fn can_write(&self) -> bool {
        true
    }
    fn stat(&self, path: &str) -> Result<Option<Meta>, String> {
        self.0.stat(path)
    }
    fn write(&self, path: &str, bytes: &[u8], _: Option<u64>) -> Result<(), String> {
        self.0.write(path, bytes, Some(self.1.fetch_add(1000, Ordering::Relaxed)))
    }
    fn make_folder(&self, path: &str) -> Result<(), String> {
        self.0.make_folder(path)
    }
}

#[test]
fn a_server_that_cant_set_times_doesnt_make_files_look_changed() {
    let folder = scratch("times");
    let root = folder.join("www");
    std::fs::create_dir_all(&root).unwrap();
    let remote = Arc::new(NoTimes(MemoryTree::writable_with(&[("a.txt", "one\n", Some(1_000))]), AtomicU64::new(2_000_000_000)));
    let (local, remote_tree): (Arc<dyn Tree>, Arc<dyn Tree>) = (Arc::new(LocalTree { root: root.clone() }), remote.clone());
    let mut state = MirrorState::new(origin());
    fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups"));
    std::fs::write(root.join("a.txt"), "two\n").unwrap();
    push("a.txt", &*local, &*remote_tree, &mut state, false, &AtomicBool::new(false)).unwrap();
    // The times disagree by years now, but neither side has changed since the save.
    let left = fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups"));
    assert!(left.items.is_empty(), "{:?}", left.items);
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "two\n");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn what_is_known_of_a_mirror_is_kept_beside_it() {
    let mut state = MirrorState::new(Origin { site: Site { scheme: Scheme::Ftpes, host: "nas".into(), port: 21, user: "ann".into(), ..Site::default() }, folder: "/share/code".into() });
    state.snapshot.files.insert("a/b.txt".into(), SnapEntry { left: SnapMeta { size: 3, modified: Some(10) }, right: SnapMeta { size: 3, modified: Some(12) }, hash: Some([1; 32]) });
    state.left_there.insert("big.iso".into(), Meta { is_dir: false, size: 1 << 30, modified: Some(99) });
    let back = MirrorState::from_text(&state.to_text()).unwrap();
    assert_eq!(back.origin, state.origin);
    assert_eq!(back.snapshot.files, state.snapshot.files);
    assert_eq!(back.left_there, state.left_there);
    assert!(!state.to_text().to_lowercase().contains("password"));
    let places = places(Path::new("/data/remote-projects"), &state.origin);
    assert_eq!(places.files.file_name().unwrap(), "code", "the project is named as the folder on the server");
    assert_eq!(places.state.parent(), places.files.parent(), "what's known is kept beside the files, not among them");
    assert_ne!(places, super::places(Path::new("/data/remote-projects"), &origin()));
    assert_eq!(relative(Path::new("/m/code"), Path::new("/m/code/src/a.c")).as_deref(), Some("src/a.c"));
    assert_eq!(relative(Path::new("/m/code"), Path::new("/elsewhere/a.c")), None);
    assert_eq!(local_place(Path::new("/m/code"), "src/a.c"), Path::new("/m/code").join("src").join("a.c"));
}

/// The whole round over real FTP, to the test server: a modern one that sets times, and an older one that can't.
#[test]
fn a_folder_on_an_ftp_server_is_mirrored_and_saves_go_back() {
    use crate::ftp::ftp_tests::{Offers, PASSWORD, USER, start};
    for offers in [Offers::default(), Offers { modern: false, epsv: false, ..Offers::default() }] {
        let server = start(offers);
        let served = server.root.join("home/ann/site");
        std::fs::create_dir_all(served.join("css")).unwrap();
        std::fs::write(served.join("index.html"), "<p>Hello</p>\n").unwrap();
        std::fs::write(served.join("css/site.css"), "p { color: red }\n").unwrap();
        std::fs::write(served.join(".gitignore"), "*.log\n").unwrap();
        std::fs::write(served.join("access.log"), "lots\n").unwrap();
        let site = Site { scheme: Scheme::Ftp, host: "127.0.0.1".into(), port: server.port, user: USER.into(), key_file: None };
        let origin = Origin { site: site.clone(), folder: "/home/ann/site".into() };
        let connection = crate::remote::connect(&site, &Secret::Password(PASSWORD.into()), &crate::remote::KnownHosts::default()).unwrap();
        let remote_tree: Arc<dyn Tree> = Arc::new(crate::diff_engine::SftpTree::with(Box::new(connection), &site, &origin.folder).unwrap());
        let folder = scratch("ftp");
        let root = folder.join("site");
        std::fs::create_dir_all(&root).unwrap();
        let local: Arc<dyn Tree> = Arc::new(LocalTree { root: root.clone() });
        let mut state = MirrorState::new(origin);
        assert!(fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups")).items.is_empty());
        assert_eq!(std::fs::read_to_string(root.join("css/site.css")).unwrap(), "p { color: red }\n");
        assert!(!root.join("access.log").exists());
        std::fs::write(root.join("index.html"), "<p>Hello again</p>\n").unwrap();
        push("index.html", &*local, &*remote_tree, &mut state, false, &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read_to_string(served.join("index.html")).unwrap(), "<p>Hello again</p>\n");
        assert!(fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups")).items.is_empty(), "{offers:?}: alike once sent");
        // Changed on the server: fetched.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(served.join("css/site.css"), "p { color: blue; margin: 0 }\n").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
        std::fs::File::options().write(true).open(served.join("css/site.css")).unwrap().set_modified(later).unwrap();
        assert!(fetch_now(&local, &remote_tree, &root, &mut state, &folder.join("backups")).items.is_empty());
        assert_eq!(std::fs::read_to_string(root.join("css/site.css")).unwrap(), "p { color: blue; margin: 0 }\n", "{offers:?}");
        let _ = std::fs::remove_dir_all(&folder);
    }
}

/// Against a real SFTP server: LIMONITER_SFTP is sftp://user@host:port/folder, LIMONITER_SFTP_KEY a private key it
/// accepts, and LIMONITER_SFTP_ROOT that folder as this computer sees it (the server being on this computer).
#[test]
#[ignore = "needs an SFTP server named in LIMONITER_SFTP"]
fn a_folder_on_a_real_sftp_server_is_mirrored_and_saves_go_back() {
    let (Ok(url), Ok(key), Ok(served)) = (std::env::var("LIMONITER_SFTP"), std::env::var("LIMONITER_SFTP_KEY"), std::env::var("LIMONITER_SFTP_ROOT")) else {
        return;
    };
    let served = PathBuf::from(served).join("mirror-test");
    let _ = std::fs::remove_dir_all(&served);
    std::fs::create_dir_all(served.join("src")).unwrap();
    std::fs::write(served.join("src/main.c"), "int main(void) { return 0; }\n").unwrap();
    std::fs::write(served.join(".gitignore"), "*.o\n").unwrap();
    std::fs::write(served.join("src/main.o"), "object").unwrap();
    std::fs::write(served.join("big.bin"), vec![0u8; LARGE as usize + 1]).unwrap();
    let (mut site, folder) = crate::remote::parse_url(&url).unwrap();
    site.key_file = Some(PathBuf::from(key));
    let mut known = crate::remote::KnownHosts::default();
    let Err(crate::remote::Refused::UnknownHost(print)) = crate::remote::connect(&site, &Secret::KeyPassphrase(None), &known) else { panic!("a new server's key is asked about first") };
    known.trust(&site, &print);
    let connection = crate::remote::connect(&site, &Secret::KeyPassphrase(None), &known).unwrap();
    let origin = Origin { site: site.clone(), folder: crate::remote::join(&folder, "mirror-test") };
    let remote_tree: Arc<dyn Tree> = Arc::new(crate::diff_engine::SftpTree::with(Box::new(connection), &site, &origin.folder).unwrap());
    let scratch_folder = scratch("sftp");
    let root = scratch_folder.join("mirror-test");
    std::fs::create_dir_all(&root).unwrap();
    let local: Arc<dyn Tree> = Arc::new(LocalTree { root: root.clone() });
    let mut state = MirrorState::new(origin);
    assert!(fetch_now(&local, &remote_tree, &root, &mut state, &scratch_folder.join("backups")).items.is_empty());
    assert_eq!(std::fs::read_to_string(root.join("src/main.c")).unwrap(), "int main(void) { return 0; }\n");
    assert!(!root.join("src/main.o").exists());
    assert!(state.is_left_there("big.bin") && std::fs::metadata(root.join("big.bin")).unwrap().len() == 0);
    std::fs::write(root.join("src/main.c"), "int main(void) { return 2; }\n").unwrap();
    push("src/main.c", &*local, &*remote_tree, &mut state, false, &AtomicBool::new(false)).unwrap();
    assert_eq!(std::fs::read_to_string(served.join("src/main.c")).unwrap(), "int main(void) { return 2; }\n");
    assert!(std::fs::read_dir(served.join("src")).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("limoniter")), "nothing left beside it");
    assert!(fetch_now(&local, &remote_tree, &root, &mut state, &scratch_folder.join("backups")).items.is_empty(), "alike once sent: SFTP keeps the time");
    // Changed on the server since: asked about.
    std::fs::write(served.join("src/main.c"), "theirs\n").unwrap();
    std::fs::write(root.join("src/main.c"), "mine\n").unwrap();
    assert_eq!(push("src/main.c", &*local, &*remote_tree, &mut state, false, &AtomicBool::new(false)), Err(PushError::ChangedOnServer));
    fetch("big.bin", &*local, &*remote_tree, &mut state, &AtomicBool::new(false)).unwrap();
    assert_eq!(std::fs::metadata(root.join("big.bin")).unwrap().len(), LARGE + 1);
    rename("src", "code", &*remote_tree, &mut state).unwrap();
    assert!(served.join("code/main.c").is_file());
    remove("code", &*remote_tree, &mut state).unwrap();
    assert!(!served.join("code").exists());
    let _ = std::fs::remove_dir_all(&served);
    let _ = std::fs::remove_dir_all(&scratch_folder);
}
