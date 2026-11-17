//! Moving a file to the Recycle Bin, or the Trash, where it can be got back from: File, This file, Delete.
//!
//! Windows does it with the shell's own file operation, a Mac with NSFileManager, and Linux by the freedesktop.org
//! trash specification, which every Linux desktop's file manager reads: the file moves into `~/.local/share/Trash/files`
//! beside a note in `info` of where it came from and when. A file on another drive than the home folder is handed to
//! `gio trash`, which knows each drive's own trash folder.

use std::path::Path;

/// What the bin is called here.
pub const BIN: &str = if cfg!(windows) { "the Recycle Bin" } else { "the Trash" };
/// The command's name in the menu.
pub const MOVE_TO_BIN: &str = if cfg!(windows) { "Move to the Recycle Bin\u{2026}" } else { "Move to the Trash\u{2026}" };

/// Moves a file to the bin.
pub fn move_to_bin(file: &Path) -> Result<(), String> {
    if !file.exists() {
        return Err(crate::i18n::tf!("{} isn't there any more.", file.display()));
    }
    let file = std::fs::canonicalize(file).map_err(|e| crate::i18n::tf!("Couldn't find {}: {e}", file.display()))?;
    platform_move(&file)
}

#[cfg(windows)]
fn platform_move(file: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::Shell::{FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW, SHFileOperationW};
    use windows::core::PCWSTR;
    // A list of names, each ending in a nul, and the list in another. A \\?\ path isn't understood here.
    let shown = file.to_string_lossy();
    let plain = shown.strip_prefix(r"\\?\UNC\").map(|rest| format!(r"\\{rest}")).or_else(|| shown.strip_prefix(r"\\?\").map(str::to_string)).unwrap_or_else(|| shown.to_string());
    let mut from: Vec<u16> = std::ffi::OsStr::new(&plain).encode_wide().collect();
    from.extend([0, 0]);
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT).0 as u16,
        ..Default::default()
    };
    // SAFETY: the operation names one file in a list that ends as the shell expects, and lives until it's done.
    let result = unsafe { SHFileOperationW(&mut operation) };
    if result != 0 {
        return Err(crate::i18n::tf!("Windows couldn't move it to the Recycle Bin (error {result})."));
    }
    if operation.fAnyOperationsAborted.as_bool() {
        return Err(crate::i18n::tr("Moving it to the Recycle Bin was stopped.").into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform_move(file: &Path) -> Result<(), String> {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::msg_send;
    let path = std::ffi::CString::new(file.to_string_lossy().as_bytes()).map_err(|_| "The file's name can't be handed to macOS.".to_string())?;
    let (Some(strings), Some(urls), Some(managers)) = (AnyClass::get(c"NSString"), AnyClass::get(c"NSURL"), AnyClass::get(c"NSFileManager")) else {
        return Err(crate::i18n::tr("macOS's file manager couldn't be reached.").into());
    };
    // SAFETY: each message is one these Foundation classes answer, with the arguments they take; the error pointer
    // is left out, as the documentation allows.
    let moved: bool = unsafe {
        let text: Retained<AnyObject> = msg_send![strings, stringWithUTF8String: path.as_ptr()];
        let url: Retained<AnyObject> = msg_send![urls, fileURLWithPath: &*text];
        let manager: Retained<AnyObject> = msg_send![managers, defaultManager];
        msg_send![&*manager, trashItemAtURL: &*url, resultingItemURL: std::ptr::null_mut::<*mut AnyObject>(), error: std::ptr::null_mut::<*mut AnyObject>()]
    };
    if moved { Ok(()) } else { Err(crate::i18n::tr("macOS couldn't move it to the Trash.").into()) }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn platform_move(file: &Path) -> Result<(), String> {
    let home_trash = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share")))
        .map(|data| data.join("Trash"));
    if let Some(trash) = home_trash {
        match freedesktop_move(file, &trash, &chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()) {
            Ok(()) => return Ok(()),
            // On another drive, the file can't simply be moved into the home folder's trash.
            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {}
            Err(e) => return Err(crate::i18n::tf!("Couldn't move it to the Trash: {e}")),
        }
    }
    let output = std::process::Command::new("gio").arg("trash").arg(file).output().map_err(|e| crate::i18n::tf!("Couldn't move it to the Trash: {e}"))?;
    if output.status.success() { Ok(()) } else { Err(crate::i18n::tf!("Couldn't move it to the Trash: {}", String::from_utf8_lossy(&output.stderr).trim())) }
}

/// The freedesktop.org trash: the file into `files`, under a name not already used there, and a `.trashinfo` of
/// the same name into `info` saying where it was and when. The note is made first, so the name is taken.
#[cfg(not(any(windows, target_os = "macos")))]
fn freedesktop_move(file: &Path, trash: &Path, when: &str) -> std::io::Result<()> {
    use std::io::Write;
    let (files, info) = (trash.join("files"), trash.join("info"));
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let name = file.file_name().ok_or_else(|| std::io::Error::other("it has no name"))?.to_string_lossy().into_owned();
    let (stem, ending) = match name.rfind('.').filter(|at| *at > 0) {
        Some(at) => (&name[..at], &name[at..]),
        None => (name.as_str(), ""),
    };
    for number in 1..10_000 {
        let candidate = if number == 1 { name.clone() } else { format!("{stem} {number}{ending}") };
        let note = info.join(format!("{candidate}.trashinfo"));
        let mut out = match std::fs::OpenOptions::new().write(true).create_new(true).open(&note) {
            Ok(out) => out,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        if files.join(&candidate).exists() {
            let _ = std::fs::remove_file(&note);
            continue;
        }
        out.write_all(format!("[Trash Info]\nPath={}\nDeletionDate={when}\n", escape_path(file)).as_bytes())?;
        drop(out);
        return std::fs::rename(file, files.join(&candidate)).inspect_err(|_| {
            let _ = std::fs::remove_file(&note);
        });
    }
    Err(std::io::Error::other("the Trash has too many files of that name"))
}

/// A path as the trash specification writes it: percent-encoded, as in a URL, but with its slashes.
#[cfg(not(any(windows, target_os = "macos")))]
fn escape_path(file: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for byte in file.as_os_str().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(*byte as char),
            other => out += &format!("%{other:02X}"),
        }
    }
    out
}

#[cfg(all(test, not(any(windows, target_os = "macos"))))]
mod tests {
    use super::*;

    #[test]
    fn files_go_into_the_trash_with_a_note_of_where_they_were() {
        let root = std::env::temp_dir().join(format!("limoniter-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (work, trash) = (root.join("my work"), root.join("Trash"));
        std::fs::create_dir_all(&work).unwrap();
        for _ in 0..2 {
            std::fs::write(work.join("notes.txt"), "kept").unwrap();
            freedesktop_move(&work.join("notes.txt"), &trash, "2026-11-15T10:20:30").unwrap();
        }
        assert!(!work.join("notes.txt").exists());
        assert_eq!(std::fs::read_to_string(trash.join("files/notes.txt")).unwrap(), "kept");
        assert!(trash.join("files/notes 2.txt").exists(), "a second file of the same name is numbered");
        let note = std::fs::read_to_string(trash.join("info/notes 2.txt.trashinfo")).unwrap();
        assert!(note.starts_with("[Trash Info]\nPath=/"));
        assert!(note.contains("/my%20work/notes.txt\nDeletionDate=2026-11-15T10:20:30\n"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
