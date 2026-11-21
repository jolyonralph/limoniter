//! A project from a ZIP or a web address, as the web IDEs import one: a ZIP unpacked into a folder of its own in
//! the folder chosen, or a Git repository cloned there (or, when Git isn't installed, a GitHub, GitLab or Codeberg
//! repository fetched as the ZIP those sites make of it). The folder is then opened as a project.

use std::path::{Path, PathBuf};

/// What a web address gives a project from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A ZIP to download and unpack.
    Zip(String),
    /// A Git repository to clone; with the ZIP of it its site makes, if there is one, for when Git isn't here.
    Git { url: String, zip: Option<String> },
}

/// The sites whose repositories are at site/owner/name, and that give a ZIP of the latest of each.
fn repository_site(host: &str) -> Option<&'static str> {
    match host.trim_start_matches("www.") {
        "github.com" => Some("github"),
        "gitlab.com" => Some("gitlab"),
        "codeberg.org" => Some("codeberg"),
        _ => None,
    }
}

/// What a web address is: a .zip (or a site's archive of a repository) is unpacked, and anything else is taken
/// for a Git repository. None if it isn't a web address at all.
pub fn source_of(address: &str) -> Option<Source> {
    let address = address.trim();
    let lower = address.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return None;
    }
    let path = lower.split(['?', '#']).next().unwrap_or(&lower);
    if path.ends_with(".zip") || path.contains("/archive/") || path.contains("/zipball/") || path.contains("/-/archive/") {
        return Some(Source::Zip(address.to_string()));
    }
    let (_, rest) = address.split_once("://")?;
    let mut parts = rest.split(['?', '#']).next().unwrap_or(rest).split('/').filter(|p| !p.is_empty());
    let host = parts.next()?.to_ascii_lowercase();
    let owner = parts.next();
    let name = parts.next().map(|n| n.trim_end_matches(".git"));
    let zip = match (repository_site(&host), owner, name) {
        (Some("github"), Some(owner), Some(name)) => Some(format!("https://github.com/{owner}/{name}/archive/HEAD.zip")),
        (Some("gitlab"), Some(owner), Some(name)) => Some(format!("https://gitlab.com/{owner}/{name}/-/archive/HEAD/{name}-HEAD.zip")),
        (Some("codeberg"), Some(owner), Some(name)) => Some(format!("https://codeberg.org/{owner}/{name}/archive/HEAD.zip")),
        _ => None,
    };
    // A page inside a repository (its tree, a file) is the repository itself.
    let url = match (repository_site(&host), owner, name) {
        (Some(_), Some(owner), Some(name)) => format!("https://{host}/{owner}/{name}"),
        _ => address.to_string(),
    };
    Some(Source::Git { url, zip })
}

/// The folder's name for a project from an address: the repository's or the ZIP's name.
pub fn folder_name(address: &str) -> String {
    let path = address.split(['?', '#']).next().unwrap_or(address);
    let path = path.split_once("://").map_or(path, |(_, rest)| rest);
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    // github.com/owner/name/archive/... is named for the repository, as a clone would be.
    let name = match parts.iter().position(|p| *p == "archive" || *p == "zipball" || *p == "-") {
        Some(at) if at >= 1 => parts[at - 1],
        _ if parts.len() > 3 && parts.first().is_some_and(|h| repository_site(&h.to_ascii_lowercase()).is_some()) => parts[2],
        _ => parts.last().copied().unwrap_or("project"),
    };
    let name = name.trim_end_matches(".git").trim_end_matches(".zip").trim_end_matches(".ZIP");
    let name = crate::media::tidy_name(name);
    if name.is_empty() || name == "_" || parts.len() < 2 { "project".into() } else { name }
}

/// A folder of that name in the folder chosen that isn't there yet: the name, or the name with a number after.
pub fn new_folder(parent: &Path, name: &str) -> PathBuf {
    let first = parent.join(name);
    if !first.exists() {
        return first;
    }
    (2..).map(|n| parent.join(format!("{name}-{n}"))).find(|p| !p.exists()).unwrap_or(first)
}

/// A ZIP's files written into a new folder in `parent` named for it (everything in one folder at the top of the
/// ZIP goes straight in). Gives back the folder.
pub fn unpack(zip: Vec<u8>, parent: &Path, name: &str) -> Result<PathBuf, String> {
    let target = new_folder(parent, name);
    crate::emulator_get::unzip(zip, &target).map_err(|e| crate::i18n::tf!("The ZIP couldn't be unpacked: {e}"))?;
    Ok(target)
}

/// A project fetched from a web address into a new folder in `parent`: cloned with Git, or downloaded and
/// unpacked. Gives back the folder, and what was done, to say.
pub fn fetch(address: &str, parent: &Path) -> Result<(PathBuf, String), String> {
    let name = folder_name(address);
    match source_of(address).ok_or_else(|| crate::i18n::tr("Give a web address starting with http:// or https://.").to_string())? {
        Source::Zip(url) => {
            let folder = unpack(crate::emulator_get::get(&url)?, parent, &name)?;
            Ok((folder, crate::i18n::tf!("Unpacked from {url}.")))
        }
        Source::Git { url, zip } => {
            if crate::git::version().is_some() {
                let folder = new_folder(parent, &name);
                crate::git::clone(&url, &folder).map_err(|e| crate::i18n::tf!("Git couldn't clone {url}: {e}"))?;
                return Ok((folder, crate::i18n::tf!("Cloned from {url} with Git.")));
            }
            let Some(zip) = zip else {
                return Err(crate::i18n::tf!("{url} looks like a Git repository, and Git isn't installed to clone it. Install Git, or give the address of a ZIP."));
            };
            let folder = unpack(crate::emulator_get::get(&zip)?, parent, &name)?;
            Ok((folder, crate::i18n::tf!("Git isn't installed, so the repository was downloaded as a ZIP ({zip}) and unpacked; it isn't a clone.")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_says_whether_it_is_a_zip_or_a_repository() {
        assert_eq!(source_of("https://example.com/games/demo.zip?dl=1"), Some(Source::Zip("https://example.com/games/demo.zip?dl=1".into())));
        assert_eq!(source_of("https://github.com/omiq/rgc-basic/archive/refs/heads/main.zip"), Some(Source::Zip("https://github.com/omiq/rgc-basic/archive/refs/heads/main.zip".into())));
        assert_eq!(
            source_of("https://github.com/leuat/TRSE.git"),
            Some(Source::Git { url: "https://github.com/leuat/TRSE".into(), zip: Some("https://github.com/leuat/TRSE/archive/HEAD.zip".into()) })
        );
        assert_eq!(
            source_of("https://github.com/leuat/TRSE/tree/master/Publish"),
            Some(Source::Git { url: "https://github.com/leuat/TRSE".into(), zip: Some("https://github.com/leuat/TRSE/archive/HEAD.zip".into()) }),
            "a page in a repository is the repository"
        );
        assert_eq!(source_of("https://gitlab.com/a/b"), Some(Source::Git { url: "https://gitlab.com/a/b".into(), zip: Some("https://gitlab.com/a/b/-/archive/HEAD/b-HEAD.zip".into()) }));
        assert_eq!(source_of("https://git.example.org/scm/thing.git"), Some(Source::Git { url: "https://git.example.org/scm/thing.git".into(), zip: None }));
        assert_eq!(source_of("ftp://example.com/x.zip"), None);
        assert_eq!(source_of("not an address"), None);
    }

    #[test]
    fn the_folder_is_named_for_the_repository_or_the_zip() {
        assert_eq!(folder_name("https://github.com/leuat/TRSE.git"), "TRSE");
        assert_eq!(folder_name("https://github.com/omiq/rgc-basic/archive/refs/heads/main.zip"), "rgc-basic");
        assert_eq!(folder_name("https://github.com/leuat/TRSE/tree/master/Publish"), "TRSE");
        assert_eq!(folder_name("https://example.com/games/space%20demo.zip"), "space%20demo");
        assert_eq!(folder_name("https://example.com/"), "project");
    }

    fn crc32(bytes: &[u8]) -> u32 {
        !bytes.iter().fold(!0u32, |crc, b| (0..8).fold(crc ^ u32::from(*b), |c, _| if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 }))
    }

    /// A ZIP with nothing compressed.
    fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut out, mut central) = (Vec::new(), Vec::new());
        for (name, data) in files {
            let offset = out.len() as u32;
            let fields = |out: &mut Vec<u8>| {
                out.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
                out.extend(crc32(data).to_le_bytes());
                out.extend((data.len() as u32).to_le_bytes());
                out.extend((data.len() as u32).to_le_bytes());
                out.extend((name.len() as u16).to_le_bytes());
                out.extend([0, 0]);
            };
            out.extend(b"PK\x03\x04");
            fields(&mut out);
            out.extend(name.as_bytes());
            out.extend(*data);
            central.extend(b"PK\x01\x02\x14\x00");
            fields(&mut central);
            // Its comment's length, disk, and attributes.
            central.extend([0; 10]);
            central.extend(offset.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let start = out.len() as u32;
        out.extend(&central);
        out.extend(b"PK\x05\x06\x00\x00\x00\x00");
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend([0, 0]);
        out
    }

    #[test]
    fn a_zip_is_unpacked_into_a_new_folder_of_its_own() {
        let base = std::env::temp_dir().join(format!("limoniter-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let zip = stored_zip(&[("demo-main/main.ras", b"program Main;\nbegin\nend.\n"), ("demo-main/src/unit.tru", b"unit U;\nend.\n")]);
        let folder = unpack(zip.clone(), &base, "demo").unwrap();
        assert_eq!(folder, base.join("demo"));
        assert_eq!(std::fs::read(folder.join("main.ras")).unwrap(), b"program Main;\nbegin\nend.\n", "the ZIP's own top folder goes straight in");
        assert!(folder.join("src/unit.tru").is_file());
        let again = unpack(zip, &base, "demo").unwrap();
        assert_eq!(again, base.join("demo-2"), "a folder that's there already is left alone");
        assert!(unpack(b"not a zip".to_vec(), &base, "bad").is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
