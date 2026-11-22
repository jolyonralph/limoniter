//! More about a file for File properties: who owns it, the program the system opens it with, and how many of its
//! lines are code rather than comments or space.

use crate::tools::CommentMarks;
use std::path::Path;

/// The lines with code on them: not empty, and not only comment. Block comments are followed from line to line;
/// comment marks inside strings aren't told apart, which matters little to a count.
pub fn lines_of_code<S: AsRef<str>>(lines: &[S], marks: &CommentMarks) -> u64 {
    let mut in_block = false;
    let mut count = 0;
    for line in lines {
        let mut rest = line.as_ref().trim();
        let mut code = false;
        while !rest.is_empty() {
            if in_block {
                let close = marks.block.map_or("", |(_, close)| close);
                match rest.find(close) {
                    Some(at) => {
                        rest = rest[at + close.len()..].trim_start();
                        in_block = false;
                    }
                    None => rest = "",
                }
                continue;
            }
            if marks.line.is_some_and(|mark| starts_with_mark(rest, mark)) {
                break;
            }
            if let Some((open, _)) = marks.block.filter(|(open, _)| rest.starts_with(open)) {
                rest = &rest[open.len()..];
                in_block = true;
                continue;
            }
            code = true;
            // Past this piece of code to the next comment, if there is one.
            let next = [marks.line.and_then(|mark| rest.find(mark)), marks.block.and_then(|(open, _)| rest.find(open))].into_iter().flatten().min();
            match next {
                Some(at) if at > 0 => rest = &rest[at..],
                _ => rest = "",
            }
        }
        if code {
            count += 1;
        }
    }
    count
}

/// Whether a line starts with a comment mark: a word such as REM only as a word of its own.
fn starts_with_mark(text: &str, mark: &str) -> bool {
    if mark.chars().all(char::is_alphabetic) {
        let word: String = text.chars().take_while(|c| c.is_alphanumeric()).collect();
        word.eq_ignore_ascii_case(mark)
    } else {
        text.starts_with(mark)
    }
}

/// The owner's name, or None where it can't be told.
#[cfg(windows)]
pub fn owner(file: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{LookupAccountSidW, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SID_NAME_USE};
    use windows::core::{PCWSTR, PWSTR};
    let wide: Vec<u16> = file.as_os_str().encode_wide().chain([0]).collect();
    let mut sid = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the name ends in a nul; the owner's SID points into the descriptor, which is freed once it's named.
    unsafe {
        if GetNamedSecurityInfoW(PCWSTR(wide.as_ptr()), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION, Some(&mut sid), None, None, None, &mut descriptor).is_err() {
            return None;
        }
        let mut name = [0u16; 256];
        let mut domain = [0u16; 256];
        let (mut name_len, mut domain_len) = (name.len() as u32, domain.len() as u32);
        let mut kind = SID_NAME_USE::default();
        let found = LookupAccountSidW(PCWSTR::null(), sid, Some(PWSTR(name.as_mut_ptr())), &mut name_len, Some(PWSTR(domain.as_mut_ptr())), &mut domain_len, &mut kind);
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        found.ok()?;
        let name = String::from_utf16_lossy(&name[..name_len as usize]);
        let domain = String::from_utf16_lossy(&domain[..domain_len as usize]);
        Some(if domain.is_empty() { name } else { format!("{domain}\\{name}") })
    }
}

#[cfg(not(windows))]
pub fn owner(file: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata(file).ok()?.uid();
    // `id` asks the system's own user directory, which on a Mac or with LDAP isn't /etc/passwd.
    let output = crate::platform::command("id").args(["-nu", &uid.to_string()]).output().ok();
    match output.filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()) {
        Some(name) if !name.is_empty() => Some(name),
        _ => Some(format!("User {uid}")),
    }
}

/// The program the system opens this kind of file with, by its name, or None where it can't be told.
#[cfg(windows)]
pub fn opens_with(file: &Path) -> Option<String> {
    use windows::Win32::UI::Shell::{ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_FRIENDLYAPPNAME, AssocQueryStringW};
    use windows::core::{PCWSTR, PWSTR};
    let ending = format!(".{}", file.extension()?.to_string_lossy());
    let wide: Vec<u16> = ending.encode_utf16().chain([0]).collect();
    let mut name = [0u16; 512];
    let mut length = name.len() as u32;
    // SAFETY: the ending ends in a nul, and the answer's room is given with its size.
    let found = unsafe { AssocQueryStringW(ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_FRIENDLYAPPNAME, PCWSTR(wide.as_ptr()), PCWSTR::null(), Some(PWSTR(name.as_mut_ptr())), &mut length) };
    found.ok().ok()?;
    let name = String::from_utf16_lossy(&name[..(length as usize).saturating_sub(1)]);
    (!name.trim().is_empty()).then_some(name)
}

#[cfg(target_os = "macos")]
pub fn opens_with(_file: &Path) -> Option<String> {
    None
}

/// From the desktop's own list: the file's kind, the program set for that kind, and that program's name.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn opens_with(file: &Path) -> Option<String> {
    let ask = |args: &[&std::ffi::OsStr]| -> Option<String> {
        let output = crate::platform::command("xdg-mime").args(args).output().ok().filter(|o| o.status.success())?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string()).filter(|s| !s.is_empty())
    };
    let kind = ask(&["query".as_ref(), "filetype".as_ref(), file.as_os_str()])?;
    let desktop = ask(&["query".as_ref(), "default".as_ref(), kind.as_ref()])?;
    let home = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")));
    let others = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let folders = home.into_iter().chain(others.split(':').filter(|d| !d.is_empty()).map(std::path::PathBuf::from));
    for folder in folders {
        if let Ok(entry) = std::fs::read_to_string(folder.join("applications").join(&desktop))
            && let Some(name) = desktop_name(&entry)
        {
            return Some(name);
        }
    }
    Some(desktop.trim_end_matches(".desktop").to_string())
}

/// The plain Name= of a .desktop file's main section.
#[cfg(not(any(windows, target_os = "macos")))]
fn desktop_name(entry: &str) -> Option<String> {
    let mut in_main = false;
    for line in entry.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
        } else if in_main && let Some(name) = line.strip_prefix("Name=") {
            return Some(name.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_space_are_not_code() {
        let c = crate::tools::comment_marks("C").unwrap();
        let source = ["// heading", "", "int a; // why", "/* one", "   two */", "/* x */ int b;", "int c; /* starts", "ends */", "   "];
        assert_eq!(lines_of_code(&source, &c), 3);
        let basic = crate::tools::comment_marks("Batch File").unwrap();
        assert_eq!(lines_of_code(&["REM a note", "rem another", "REMOVE x", "echo"], &basic), 2);
        let html = crate::tools::comment_marks("HTML").unwrap();
        assert_eq!(lines_of_code(&["<!-- a -->", "<p>", "<!--", "-->"], &html), 1);
    }

    #[test]
    fn this_file_has_an_owner() {
        assert!(owner(Path::new(file!())).is_some_and(|name| !name.is_empty()));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn a_desktop_files_name_is_its_main_sections() {
        assert_eq!(desktop_name("[Desktop Entry]\nName[fr]=Éditeur\nName=Text Editor\n[Desktop Action new]\nName=New"), Some("Text Editor".into()));
        assert_eq!(desktop_name("[Other]\nName=No"), None);
    }
}
