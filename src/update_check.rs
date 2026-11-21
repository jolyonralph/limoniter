//! Help, Check for a newer Limoniter, and the same once a day by itself while the setting's on: asks GitHub for
//! Limoniter's latest release and says if it's newer than this one. What's then done about it (the download page, or
//! Limoniter updating itself) is in update_host.rs and self_update.rs.
//!
//! The Microsoft Store's Limoniter and the Flatpak are kept up to date by the Store and by Flathub, so they don't
//! ask by themselves, and when asked they say where the update comes from.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LATEST: &str = "https://api.github.com/repos/jolyonralph/limoniter/releases/latest";
/// The page a newer version is downloaded from.
pub const DOWNLOAD_PAGE: &str = "https://github.com/jolyonralph/limoniter/releases/latest";
/// How often to ask, at most, unless asked to.
const EVERY: Duration = Duration::from_secs(20 * 60 * 60);
/// The most of a release's notes shown.
const MOST_NOTES: usize = 900;

/// Where this copy of Limoniter is updated from, when that isn't the download page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdatedBy {
    DownloadPage,
    Store,
    Flathub,
}

impl UpdatedBy {
    pub fn this_copy() -> Self {
        if crate::platform::from_store() {
            UpdatedBy::Store
        } else if std::env::var_os("FLATPAK_ID").is_some() {
            UpdatedBy::Flathub
        } else {
            UpdatedBy::DownloadPage
        }
    }
}

/// What a check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Checked too lately to ask again, or this newer version has been told about already.
    NotDue,
    UpToDate,
    /// A newer version, and the start of what its release says.
    Newer { version: String, notes: String },
}

/// This copy's version.
pub fn running() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The numbers in a version, "v0.10.2" being [0, 10, 2].
fn numbers(version: &str) -> Vec<u32> {
    version.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map_while(|part| part.parse().ok()).collect()
}

/// Whether one version comes after another.
pub fn newer(version: &str, than: &str) -> bool {
    let (a, b) = (numbers(version), numbers(than));
    !a.is_empty() && a > b
}

fn folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("LIMONITER_UPDATES") {
        return Some(PathBuf::from(folder));
    }
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The version and notes in GitHub's description of a release. Drafts and prereleases aren't offered.
pub fn release_from(text: &str) -> Result<(String, String), String> {
    let json: serde_json::Value = serde_json::from_str(text).map_err(|_| crate::i18n::tr("GitHub's answer wasn't what was expected.").to_string())?;
    if json["draft"].as_bool() == Some(true) || json["prerelease"].as_bool() == Some(true) {
        return Ok((String::new(), String::new()));
    }
    let tag = json["tag_name"].as_str().ok_or(crate::i18n::tr("The release has no version."))?;
    let body = json["body"].as_str().unwrap_or("").replace("\r\n", "\n");
    // How to download and install it is on the page it's downloaded from.
    let body = body.find("## Downloads").map_or(body.as_str(), |at| &body[..at]);
    let mut notes = body.trim().to_string();
    if notes.chars().count() > MOST_NOTES {
        let cut: String = notes.chars().take(MOST_NOTES).collect();
        let at = cut.rfind('\n').filter(|at| *at > MOST_NOTES / 2).unwrap_or(cut.len());
        notes = format!("{}\n\u{2026}", cut[..at].trim_end());
    }
    Ok((tag.trim_start_matches(['v', 'V']).to_string(), notes))
}

fn latest() -> Result<(String, String), String> {
    let response = ureq::get(LATEST)
        .header("User-Agent", "Limoniter")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| crate::i18n::tf!("GitHub couldn't be asked: {e}"))?;
    let text = response.into_body().with_config().limit(1 << 20).read_to_string().map_err(|e| crate::i18n::tf!("GitHub's answer couldn't be read: {e}"))?;
    release_from(&text)
}

/// What to make of the latest release, given when the last check was and which version was last told about.
/// A check asked for always says; one by itself tells about each newer version once.
pub fn judge(latest: &str, notes: String, asked: bool, told: &str) -> Outcome {
    if !newer(latest, running()) {
        return Outcome::UpToDate;
    }
    if !asked && told == latest {
        return Outcome::NotDue;
    }
    Outcome::Newer { version: latest.to_string(), notes }
}

/// Asks GitHub for the latest Limoniter. Unless `asked`, it doesn't ask again within a day.
pub fn check(asked: bool) -> Result<Outcome, String> {
    let folder = folder();
    let stamp = folder.as_ref().map(|f| f.join("update-check.txt"));
    let kept = stamp.as_ref().and_then(|s| std::fs::read_to_string(s).ok()).unwrap_or_default();
    let mut kept = kept.lines();
    let last = kept.next().and_then(|t| t.trim().parse::<u64>().ok()).unwrap_or(0);
    let told = kept.next().unwrap_or("").trim().to_string();
    if !asked && now().saturating_sub(last) < EVERY.as_secs() {
        return Ok(Outcome::NotDue);
    }
    let (version, notes) = latest()?;
    let outcome = judge(&version, notes, asked, &told);
    let now_told = if let Outcome::Newer { version, .. } = &outcome { version.clone() } else { told };
    if let (Some(folder), Some(stamp)) = (folder, stamp) {
        let _ = std::fs::create_dir_all(folder);
        let _ = std::fs::write(stamp, format!("{}\n{now_told}\n", now()));
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_compared_by_their_numbers() {
        assert!(newer("0.10.0", "0.9.1"));
        assert!(newer("v1.0", "0.99.99"));
        assert!(!newer("0.9.1", "0.9.1"));
        assert!(!newer("0.9", "0.9.1"));
        assert!(!newer("", "0.9.1"));
    }

    #[test]
    fn a_release_gives_its_version_and_the_start_of_its_notes() {
        let release = r#"{"tag_name":"v0.10.0","draft":false,"prerelease":false,"body":"New:\r\n- Column markers\r\n\r\n## Downloads\r\n- Windows"}"#;
        assert_eq!(release_from(release).unwrap(), ("0.10.0".to_string(), "New:\n- Column markers".to_string()));
        let long = format!(r#"{{"tag_name":"v1","body":"{}"}}"#, "A line of notes.\\n".repeat(200));
        let (_, notes) = release_from(&long).unwrap();
        assert!(notes.chars().count() <= MOST_NOTES + 2 && notes.ends_with('\u{2026}'));
        assert_eq!(release_from(r#"{"tag_name":"v9","prerelease":true}"#).unwrap().0, "");
        assert!(release_from("{}").is_err());
    }

    #[test]
    fn each_newer_version_is_told_about_once_unless_asked() {
        assert_eq!(judge(running(), String::new(), true, ""), Outcome::UpToDate);
        assert_eq!(judge("", String::new(), false, ""), Outcome::UpToDate);
        let newer = judge("999.0", "notes".into(), false, "");
        assert_eq!(newer, Outcome::Newer { version: "999.0".into(), notes: "notes".into() });
        assert_eq!(judge("999.0", String::new(), false, "999.0"), Outcome::NotDue);
        assert!(matches!(judge("999.0", String::new(), true, "999.0"), Outcome::Newer { .. }));
    }

    /// Asks GitHub for real: cargo test -- --ignored update_check::tests::asks_github
    #[test]
    #[ignore]
    fn asks_github() {
        println!("{:?}", latest().unwrap());
    }
}
