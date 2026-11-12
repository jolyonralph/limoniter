//! Offering Limoniter to Windows as an editor for chosen file types.
//!
//! Windows doesn't let a program make itself the default: it keeps that choice protected, and only the person
//! can make it, in Windows' own settings. So this does what a program may do. It registers Limoniter for the
//! types, for this Windows user only, then opens the page where the choice is made. Nothing here replaces
//! another program's registration, and removing takes away only what this wrote.

// Only Windows has a registry to write to; elsewhere this is what the settings box shows, and nothing more.
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

use std::collections::BTreeSet;
use std::path::Path;

/// The types offered as check boxes, in groups.
pub const COMMON_TYPES: [(&str, &[&str]); 5] = [
    ("Text", &["txt", "log", "md", "rst", "tex"]),
    ("Data", &["csv", "tsv", "json", "xml", "yaml", "yml", "toml", "ini", "cfg", "conf"]),
    ("Web", &["html", "htm", "css", "js", "ts", "php"]),
    ("Code", &["c", "h", "cpp", "hpp", "cs", "java", "py", "rb", "rs", "go", "sql", "lua"]),
    ("Assembler and scripts", &["asm", "s", "inc", "a65", "z80", "ps1", "sh"]),
];

/// Types that Windows runs. If an editor opened these instead, programs would stop starting, so they're refused.
const REFUSED_TYPES: [&str; 21] = ["exe", "com", "bat", "cmd", "lnk", "msi", "msp", "scr", "pif", "cpl", "dll", "sys", "url", "reg", "hta", "vbs", "vbe", "wsf", "wsh", "jse", "appx"];

const PROG_ID: &str = "Limoniter.File";
const APP_NAME: &str = "Limoniter";
const CAPABILITIES: &str = r"Software\Limoniter\Capabilities";
const APP_KEY: &str = r"Software\Limoniter";
const REGISTERED_APPLICATIONS: &str = r"Software\RegisteredApplications";
const CLASSES: &str = r"Software\Classes";

fn clean_type(text: &str) -> Result<Option<String>, String> {
    let name = text.trim().trim_start_matches("*.").trim_start_matches('.').to_lowercase();
    if name.is_empty() {
        return Ok(None);
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) || name.ends_with('.') {
        return Err(format!("\"{}\" isn't a file type. Use something like log or .log.", text.trim()));
    }
    if REFUSED_TYPES.contains(&name.as_str()) {
        return Err(format!(".{name} files are run by Windows, so an editor shouldn't take them over."));
    }
    Ok(Some(name))
}

/// The types chosen, tidied and without repeats. Says what's wrong if one of the others can't be used.
pub fn parse_types(ticked: &BTreeSet<String>, others: &str) -> Result<Vec<String>, String> {
    let mut types = BTreeSet::new();
    for text in ticked.iter().map(String::as_str).chain(others.split([',', ';', ' '])) {
        if let Some(name) = clean_type(text)? {
            types.insert(name);
        }
    }
    Ok(types.into_iter().collect())
}

/// Splits what's registered into the check boxes and the rest, for showing.
pub fn split_registered(registered: &[String]) -> (BTreeSet<String>, String) {
    let common = |name: &str| COMMON_TYPES.iter().any(|(_, types)| types.contains(&name));
    let ticked = registered.iter().filter(|name| common(name)).cloned().collect();
    let others = registered.iter().filter(|name| !common(name)).cloned().collect::<Vec<_>>().join(", ");
    (ticked, others)
}

/// One value to write. A key's own default value has no name.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    key: String,
    name: Option<String>,
    data: String,
}

fn entry(key: impl Into<String>, name: Option<&str>, data: impl Into<String>) -> Entry {
    Entry { key: key.into(), name: name.map(str::to_string), data: data.into() }
}

/// Everything registering writes. Note what's not here: a type's own key is never given a new default, because
/// that would replace whatever now opens it. The type only gains a line saying that Limoniter can too.
fn plan(exe: &str, exe_name: &str, types: &[String]) -> Vec<Entry> {
    let command = format!("\"{exe}\" \"%1\"");
    let prog = format!(r"{CLASSES}\{PROG_ID}");
    let app = format!(r"{CLASSES}\Applications\{exe_name}");
    let mut entries = vec![
        entry(&prog, None, "File for Limoniter"),
        entry(format!(r"{prog}\DefaultIcon"), None, format!("\"{exe}\",0")),
        entry(format!(r"{prog}\shell\open\command"), None, &command),
        entry(&app, Some("FriendlyAppName"), APP_NAME),
        entry(format!(r"{app}\shell\open\command"), None, &command),
        entry(CAPABILITIES, Some("ApplicationName"), APP_NAME),
        entry(CAPABILITIES, Some("ApplicationDescription"), "A text and hex editor"),
        entry(REGISTERED_APPLICATIONS, Some(APP_NAME), CAPABILITIES),
    ];
    for name in types {
        let dotted = format!(".{name}");
        entries.push(entry(format!(r"{CLASSES}\{dotted}\OpenWithProgids"), Some(PROG_ID), ""));
        entries.push(entry(format!(r"{app}\SupportedTypes"), Some(&dotted), ""));
        entries.push(entry(format!(r"{CAPABILITIES}\FileAssociations"), Some(&dotted), PROG_ID));
    }
    entries
}

#[cfg(windows)]
mod registry {
    use std::ffi::c_void;

    const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
    const KEY_READ: u32 = 0x0002_0019;
    const KEY_WRITE: u32 = 0x0002_0006;
    const REG_SZ: u32 = 1;
    const OK: i32 = 0;
    const NOT_FOUND: i32 = 2;
    const NO_MORE_ITEMS: i32 = 259;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegCreateKeyExW(key: isize, sub: *const u16, reserved: u32, class: *const u16, options: u32, access: u32, security: *const c_void, result: *mut isize, disposition: *mut u32) -> i32;
        fn RegOpenKeyExW(key: isize, sub: *const u16, options: u32, access: u32, result: *mut isize) -> i32;
        fn RegCloseKey(key: isize) -> i32;
        fn RegSetValueExW(key: isize, name: *const u16, reserved: u32, kind: u32, data: *const u8, size: u32) -> i32;
        fn RegDeleteValueW(key: isize, name: *const u16) -> i32;
        fn RegEnumValueW(key: isize, index: u32, name: *mut u16, name_size: *mut u32, reserved: *mut u32, kind: *mut u32, data: *mut u8, data_size: *mut u32) -> i32;
        fn RegEnumKeyExW(key: isize, index: u32, name: *mut u16, name_size: *mut u32, reserved: *mut u32, class: *mut u16, class_size: *mut u32, time: *mut c_void) -> i32;
        fn RegDeleteTreeW(key: isize, sub: *const u16) -> i32;
        fn RegDeleteKeyW(key: isize, sub: *const u16) -> i32;
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn problem(what: &str, path: &str, code: i32) -> String {
        format!("Windows wouldn't let this {what} {path} (error {code}).")
    }

    /// A key under this Windows user's part of the registry. Closed when dropped.
    pub struct Key(isize);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful open or create, and is closed only here.
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }

    pub fn create(path: &str) -> Result<Key, String> {
        let mut key = 0;
        // SAFETY: the path is a terminated string that outlives the call, the out pointer is a live local, and the rest may be null.
        let code = unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, std::ptr::null(), 0, KEY_READ | KEY_WRITE, std::ptr::null(), &raw mut key, std::ptr::null_mut()) };
        if code == OK { Ok(Key(key)) } else { Err(problem("create", path, code)) }
    }

    /// None if the key isn't there.
    pub fn open(path: &str) -> Result<Option<Key>, String> {
        let mut key = 0;
        // SAFETY: as for create.
        let code = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, KEY_READ | KEY_WRITE, &raw mut key) };
        match code {
            OK => Ok(Some(Key(key))),
            NOT_FOUND => Ok(None),
            _ => Err(problem("open", path, code)),
        }
    }

    impl Key {
        pub fn set(&self, name: Option<&str>, data: &str) -> Result<(), String> {
            let name = name.map(wide);
            let data = wide(data);
            // A Windows string's size is in bytes, and includes its terminator.
            let size = u32::try_from(data.len() * 2).map_err(|_| "That value is too long.".to_string())?;
            // SAFETY: the name is terminated or null, the data is `size` readable bytes, and both outlive the call.
            let code = unsafe { RegSetValueExW(self.0, name.as_ref().map_or(std::ptr::null(), |text| text.as_ptr()), 0, REG_SZ, data.as_ptr().cast(), size) };
            if code == OK { Ok(()) } else { Err(problem("set", "a value in", code)) }
        }

        /// It not being there already is fine.
        pub fn delete_value(&self, name: &str) -> Result<(), String> {
            // SAFETY: the name is a terminated string that outlives the call.
            let code = unsafe { RegDeleteValueW(self.0, wide(name).as_ptr()) };
            if code == OK || code == NOT_FOUND { Ok(()) } else { Err(problem("delete", name, code)) }
        }

        pub fn value_names(&self) -> Vec<String> {
            let mut names = Vec::new();
            let mut buffer = [0u16; 16384];
            for index in 0.. {
                let mut size = buffer.len() as u32;
                // SAFETY: the buffer holds `size` characters; the other outputs are optional and left null.
                let code = unsafe { RegEnumValueW(self.0, index, buffer.as_mut_ptr(), &raw mut size, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
                if code != OK {
                    break;
                }
                names.push(String::from_utf16_lossy(&buffer[..size as usize]));
            }
            names
        }

        fn has_subkeys(&self) -> bool {
            let mut buffer = [0u16; 256];
            let mut size = buffer.len() as u32;
            // SAFETY: the buffer holds `size` characters; the other outputs are optional and left null.
            let code = unsafe { RegEnumKeyExW(self.0, 0, buffer.as_mut_ptr(), &raw mut size, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
            code != NO_MORE_ITEMS
        }

        /// Whether nothing's left in it, its default value included.
        fn is_empty(&self) -> bool {
            self.value_names().is_empty() && !self.has_subkeys()
        }
    }

    /// Takes away a key this wrote into, but only if nothing of anyone else's is in it.
    pub fn delete_if_empty(path: &str) -> Result<(), String> {
        let Some(key) = open(path)? else {
            return Ok(());
        };
        if !key.is_empty() {
            return Ok(());
        }
        drop(key);
        // SAFETY: the path is a terminated string that outlives the call.
        let code = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, wide(path).as_ptr()) };
        if code == OK || code == NOT_FOUND { Ok(()) } else { Err(problem("delete", path, code)) }
    }

    /// Takes away a key that's entirely this app's own. It not being there already is fine.
    pub fn delete_tree(path: &str) -> Result<(), String> {
        // SAFETY: the path is a terminated string that outlives the call.
        let code = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(path).as_ptr()) };
        if code == OK || code == NOT_FOUND { Ok(()) } else { Err(problem("delete", path, code)) }
    }

    /// Reads a key's value, if it's there.
    pub fn value(path: &str, name: Option<&str>) -> Option<String> {
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn RegGetValueW(key: isize, sub: *const u16, name: *const u16, flags: u32, kind: *mut u32, data: *mut u8, size: *mut u32) -> i32;
        }
        const RRF_RT_REG_SZ: u32 = 2;
        let mut buffer = [0u16; 2048];
        let mut size = (buffer.len() * 2) as u32;
        let name = name.map(wide);
        // SAFETY: the strings are terminated and outlive the call, and the buffer holds `size` bytes.
        let code = unsafe { RegGetValueW(HKEY_CURRENT_USER, wide(path).as_ptr(), name.as_ref().map_or(std::ptr::null(), |text| text.as_ptr()), RRF_RT_REG_SZ, std::ptr::null_mut(), buffer.as_mut_ptr().cast(), &raw mut size) };
        (code == OK).then(|| String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]))
    }
}

#[cfg(windows)]
fn registered_under(root: &str) -> Result<Vec<String>, String> {
    let path = format!(r"{root}{CAPABILITIES}\FileAssociations");
    let Some(key) = registry::open(&path)? else {
        return Ok(Vec::new());
    };
    let mut types: Vec<String> = key.value_names().iter().map(|name| name.trim_start_matches('.').to_string()).collect();
    types.sort();
    Ok(types)
}

#[cfg(windows)]
fn drop_type(root: &str, exe_name: &str, name: &str) -> Result<(), String> {
    let dotted = format!(".{name}");
    let type_key = format!(r"{root}{CLASSES}\{dotted}");
    let progids = format!(r"{type_key}\OpenWithProgids");
    if let Some(key) = registry::open(&progids)? {
        key.delete_value(PROG_ID)?;
    }
    registry::delete_if_empty(&progids)?;
    registry::delete_if_empty(&type_key)?;
    if let Some(key) = registry::open(&format!(r"{root}{CLASSES}\Applications\{exe_name}\SupportedTypes"))? {
        key.delete_value(&dotted)?;
    }
    if let Some(key) = registry::open(&format!(r"{root}{CAPABILITIES}\FileAssociations"))? {
        key.delete_value(&dotted)?;
    }
    Ok(())
}

/// Registers for exactly these types, taking away any others this registered earlier.
#[cfg(windows)]
fn register_under(root: &str, exe: &str, exe_name: &str, types: &[String]) -> Result<(), String> {
    for earlier in registered_under(root)? {
        if !types.contains(&earlier) {
            drop_type(root, exe_name, &earlier)?;
        }
    }
    for item in plan(exe, exe_name, types) {
        registry::create(&format!("{root}{}", item.key))?.set(item.name.as_deref(), &item.data)?;
    }
    Ok(())
}

#[cfg(windows)]
fn remove_under(root: &str, exe_name: &str) -> Result<(), String> {
    for earlier in registered_under(root)? {
        drop_type(root, exe_name, &earlier)?;
    }
    if let Some(key) = registry::open(&format!("{root}{REGISTERED_APPLICATIONS}"))? {
        key.delete_value(APP_NAME)?;
    }
    registry::delete_tree(&format!(r"{root}{CLASSES}\{PROG_ID}"))?;
    registry::delete_tree(&format!(r"{root}{CLASSES}\Applications\{exe_name}"))?;
    registry::delete_tree(&format!("{root}{APP_KEY}"))
}

/// Whether what's registered to open files runs this exact program. Windows doesn't mind about capitals in a path.
fn command_runs(command: &str, exe: &str) -> bool {
    command.to_lowercase().starts_with(&format!("\"{}\"", exe.to_lowercase()))
}

/// Takes away what's registered, but only if it's this copy of the program that registered it, and says if it did.
#[cfg(windows)]
fn remove_if_mine_under(root: &str, exe: &str, exe_name: &str) -> Result<bool, String> {
    let command = registry::value(&format!(r"{root}{CLASSES}\{PROG_ID}\shell\open\command"), None);
    if !command.is_some_and(|command| command_runs(&command, exe)) {
        return Ok(false);
    }
    remove_under(root, exe_name)?;
    Ok(true)
}

/// For uninstalling: Windows shouldn't be left offering a program that's gone. Another copy's registration, such as
/// one installed somewhere else, is none of this copy's business.
#[cfg(windows)]
pub fn remove_if_mine() -> Result<bool, String> {
    let (exe, exe_name) = this_program()?;
    let removed = remove_if_mine_under("", &exe, &exe_name)?;
    if removed {
        tell_explorer();
    }
    Ok(removed)
}

#[cfg(not(windows))]
pub fn remove_if_mine() -> Result<bool, String> {
    Ok(false)
}

/// This copy of the program, and the name Windows knows it by.
#[cfg(windows)]
fn this_program() -> Result<(String, String), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok((exe.display().to_string(), program_name(&exe)))
}

#[cfg(windows)]
fn program_name(exe: &Path) -> String {
    exe.file_name().map_or_else(|| "limoniter.exe".to_string(), |name| name.to_string_lossy().into_owned())
}

/// Tells Explorer to look again, so it doesn't go on showing the old choices.
#[cfg(windows)]
fn tell_explorer() {
    const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHChangeNotify(event: i32, flags: u32, one: *const std::ffi::c_void, two: *const std::ffi::c_void);
    }
    // SAFETY: this event takes no items, so both are null.
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, 0, std::ptr::null(), std::ptr::null());
    }
}

/// What's registered now, as bare types such as "txt".
#[cfg(windows)]
pub fn registered() -> Vec<String> {
    registered_under("").unwrap_or_default()
}

/// Registers this copy of the program for exactly these types, for this Windows user.
#[cfg(windows)]
pub fn register(types: &[String]) -> Result<(), String> {
    let (exe, exe_name) = this_program()?;
    let result = register_under("", &exe, &exe_name, types);
    tell_explorer();
    result
}

/// Takes away everything registering wrote.
#[cfg(windows)]
pub fn remove() -> Result<(), String> {
    let (_, exe_name) = this_program()?;
    let result = remove_under("", &exe_name);
    tell_explorer();
    result
}

/// Opens the page in Windows settings where defaults are chosen: on Windows 11, Limoniter's own page.
#[cfg(windows)]
pub fn open_windows_page() -> Result<(), String> {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(window: isize, verb: *const u16, file: *const u16, parameters: *const u16, folder: *const u16, show: i32) -> isize;
    }
    const SW_SHOWNORMAL: i32 = 1;
    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, page) = (wide("open"), wide(&format!("ms-settings:defaultapps?registeredAppUser={APP_NAME}")));
    // SAFETY: the strings are terminated and outlive the call, and the rest may be null.
    let result = unsafe { ShellExecuteW(0, verb.as_ptr(), page.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    if result > 32 { Ok(()) } else { Err("Windows wouldn't open its settings. Look for Default apps in Windows Settings.".to_string()) }
}

#[cfg(not(windows))]
pub fn registered() -> Vec<String> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn register(_types: &[String]) -> Result<(), String> {
    Err("This only works on Windows.".to_string())
}

#[cfg(not(windows))]
pub fn remove() -> Result<(), String> {
    Err("This only works on Windows.".to_string())
}

#[cfg(not(windows))]
pub fn open_windows_page() -> Result<(), String> {
    Err("This only works on Windows.".to_string())
}

/// This copy of the program, for showing which one would be registered.
pub fn this_program_shown() -> String {
    std::env::current_exe().map_or_else(|_| String::new(), |path: std::path::PathBuf| path.as_path().display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticked(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn typed_types_are_tidied_and_repeats_dropped() {
        assert_eq!(parse_types(&ticked(&["txt"]), " .LOG, *.md;txt  tar.gz").unwrap(), ["log", "md", "tar.gz", "txt"]);
        assert_eq!(parse_types(&ticked(&[]), "").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn anything_that_could_get_out_of_its_registry_key_is_refused() {
        for bad in [r"a\b", "a/b", "*", "a\"b", "%1", "a.", "..\\x", "caf\u{e9}"] {
            assert!(parse_types(&ticked(&[]), bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn programs_windows_runs_are_refused_however_they_are_written() {
        for bad in ["exe", ".EXE", "*.bat", "cmd", "lnk", "msi", "reg", "vbs"] {
            let problem = parse_types(&ticked(&[]), bad).unwrap_err();
            assert!(problem.contains("run by Windows"), "{bad}: {problem}");
        }
    }

    #[test]
    fn the_check_boxes_offer_nothing_refused_and_no_repeats() {
        let mut seen = BTreeSet::new();
        for (_, types) in COMMON_TYPES {
            for name in types {
                assert!(seen.insert(*name), "{name} twice");
                assert_eq!(clean_type(name), Ok(Some((*name).to_string())));
            }
        }
    }

    #[test]
    fn whats_registered_splits_into_boxes_and_the_rest() {
        let (boxes, others) = split_registered(&["txt".to_string(), "weird".to_string(), "rs".to_string(), "other".to_string()]);
        assert_eq!(boxes, ticked(&["rs", "txt"]));
        assert_eq!(others, "weird, other");
    }

    #[test]
    fn the_plan_opens_the_file_by_quoted_path_and_never_replaces_a_types_default() {
        let entries = plan(r"C:\Program Files\EC\limoniter.exe", "limoniter.exe", &["txt".to_string()]);
        let command = entries.iter().find(|item| item.key.ends_with(r"Limoniter.File\shell\open\command")).unwrap();
        assert_eq!(command.data, r#""C:\Program Files\EC\limoniter.exe" "%1""#);
        assert!(entries.iter().any(|item| item.key == r"Software\Classes\.txt\OpenWithProgids" && item.name.as_deref() == Some("Limoniter.File")));
        assert!(!entries.iter().any(|item| item.key == r"Software\Classes\.txt"), "whatever opens .txt now is left alone");
        assert!(!entries.iter().any(|item| item.key.to_lowercase().contains("userchoice")), "that's the person's own choice");
        assert!(entries.iter().all(|item| item.key.starts_with(r"Software\")), "only this user's part of the registry");
    }

    #[test]
    fn only_this_exact_program_counts_as_this_one() {
        let exe = r"C:\Users\Someone\AppData\Local\Programs\Limoniter\limoniter.exe";
        assert!(command_runs(&format!("\"{exe}\" \"%1\""), exe));
        assert!(command_runs(&format!("\"{}\" \"%1\"", exe.to_uppercase()), exe), "capitals don't matter on Windows");
        assert!(!command_runs(r#""D:\Tools\limoniter.exe" "%1""#, exe), "another copy");
        assert!(!command_runs(&format!("\"{exe}.old\" \"%1\""), exe), "a name that only starts the same");
        assert!(!command_runs("", exe));
    }

    /// The registry calls, for real, but well away from anything Windows reads: under a key of the test's own,
    /// which is deleted afterwards. They do write to this computer's registry, so they only run when asked for:
    /// cargo test real_registry -- --ignored --test-threads=1
    #[cfg(windows)]
    mod real_registry {
        use super::super::*;

        struct TestRoot(String);

        impl TestRoot {
            fn new(tag: &str) -> Self {
                let root = format!(r"Software\LimoniterTest-{}-{tag}\", std::process::id());
                registry::delete_tree(root.trim_end_matches('\\')).unwrap();
                Self(root)
            }
        }

        impl Drop for TestRoot {
            fn drop(&mut self) {
                let _ = registry::delete_tree(self.0.trim_end_matches('\\'));
            }
        }

        fn types(names: &[&str]) -> Vec<String> {
            names.iter().map(|name| name.to_string()).collect()
        }

        const EXE: &str = r"C:\Tools\limoniter.exe";

        #[test]
        #[ignore = "writes to this computer's registry"]
        fn registering_writes_what_was_planned_and_can_be_read_back() {
            let root = TestRoot::new("write");
            let root = root.0.as_str();
            register_under(root, EXE, "limoniter.exe", &types(&["txt", "log"])).unwrap();
            assert_eq!(registered_under(root).unwrap(), ["log", "txt"]);
            for item in plan(EXE, "limoniter.exe", &types(&["txt", "log"])) {
                assert_eq!(registry::value(&format!("{root}{}", item.key), item.name.as_deref()).as_deref(), Some(item.data.as_str()), "{item:?}");
            }
        }

        #[test]
        #[ignore = "writes to this computer's registry"]
        fn registering_fewer_takes_away_the_rest() {
            let root = TestRoot::new("fewer");
            let root = root.0.as_str();
            register_under(root, EXE, "limoniter.exe", &types(&["txt", "log"])).unwrap();
            register_under(root, EXE, "limoniter.exe", &types(&["txt"])).unwrap();
            assert_eq!(registered_under(root).unwrap(), ["txt"]);
            assert!(registry::open(&format!(r"{root}Software\Classes\.log")).unwrap().is_none(), "nothing was left behind");
            assert!(registry::open(&format!(r"{root}Software\Classes\.txt\OpenWithProgids")).unwrap().is_some());
        }

        #[test]
        #[ignore = "writes to this computer's registry"]
        fn other_programs_registrations_and_defaults_survive_everything() {
            let root = TestRoot::new("others");
            let root = root.0.as_str();
            let txt = format!(r"{root}Software\Classes\.txt");
            registry::create(&txt).unwrap().set(None, "SomeoneElse.txt").unwrap();
            registry::create(&format!(r"{txt}\OpenWithProgids")).unwrap().set(Some("SomeoneElse.txt"), "").unwrap();
            registry::create(&format!("{root}{REGISTERED_APPLICATIONS}")).unwrap().set(Some("SomeoneElse"), "x").unwrap();

            register_under(root, EXE, "limoniter.exe", &types(&["txt"])).unwrap();
            assert_eq!(registry::value(&txt, None).as_deref(), Some("SomeoneElse.txt"), "registering leaves the default as it was");
            remove_under(root, "limoniter.exe").unwrap();

            assert_eq!(registry::value(&txt, None).as_deref(), Some("SomeoneElse.txt"));
            assert_eq!(registry::value(&format!(r"{txt}\OpenWithProgids"), Some("SomeoneElse.txt")).as_deref(), Some(""));
            assert_eq!(registry::value(&format!(r"{txt}\OpenWithProgids"), Some(PROG_ID)), None);
            assert_eq!(registry::value(&format!("{root}{REGISTERED_APPLICATIONS}"), Some("SomeoneElse")).as_deref(), Some("x"));
            assert_eq!(registry::value(&format!("{root}{REGISTERED_APPLICATIONS}"), Some(APP_NAME)), None);
        }

        #[test]
        #[ignore = "writes to this computer's registry"]
        fn removing_leaves_nothing_of_ours_and_can_be_done_twice() {
            let root = TestRoot::new("remove");
            let root = root.0.as_str();
            register_under(root, EXE, "limoniter.exe", &types(&["txt"])).unwrap();
            remove_under(root, "limoniter.exe").unwrap();
            assert!(registered_under(root).unwrap().is_empty());
            for key in [format!(r"{root}Software\Limoniter"), format!(r"{root}Software\Classes\Limoniter.File"), format!(r"{root}Software\Classes\Applications\limoniter.exe"), format!(r"{root}Software\Classes\.txt")] {
                assert!(registry::open(&key).unwrap().is_none(), "{key}");
            }
            remove_under(root, "limoniter.exe").unwrap();
        }

        #[test]
        #[ignore = "writes to this computer's registry"]
        fn uninstalling_only_takes_back_what_this_copy_registered() {
            let root = TestRoot::new("uninstall");
            let root = root.0.as_str();
            register_under(root, r"D:\Other\limoniter.exe", "limoniter.exe", &types(&["txt"])).unwrap();
            assert!(!remove_if_mine_under(root, EXE, "limoniter.exe").unwrap(), "another copy's are left alone");
            assert_eq!(registered_under(root).unwrap(), ["txt"]);
            register_under(root, EXE, "limoniter.exe", &types(&["txt"])).unwrap();
            assert!(remove_if_mine_under(root, EXE, "limoniter.exe").unwrap());
            assert!(registered_under(root).unwrap().is_empty());
            assert!(!remove_if_mine_under(root, EXE, "limoniter.exe").unwrap(), "with nothing registered there's nothing to do");
        }


        #[test]
        #[ignore = "writes to this computer's registry"]
        fn reading_with_nothing_registered_is_just_empty() {
            let root = TestRoot::new("empty");
            assert!(registered_under(&root.0).unwrap().is_empty());
        }
    }
}
