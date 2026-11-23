//! Node's debugger: VS Code's js-debug, in the standalone build its authors release for other editors
//! (`js-debug-dap-v….tar.gz`, from github.com/microsoft/vscode-js-debug). It's a debug adapter written in
//! JavaScript, so it runs on the Node already installed. When the person asks, the latest release is downloaded
//! into a folder of Limoniter's own beside its settings, where debugging a JavaScript file then finds it.
//! js-debug is free software (MIT licence), so fetching it for the person from its own page is allowed; Limoniter
//! doesn't come with it.

use std::path::{Path, PathBuf};

/// Where GitHub describes the latest release.
pub const LATEST: &str = "https://api.github.com/repos/microsoft/vscode-js-debug/releases/latest";

/// The adapter's script, which is what is run with Node.
const SERVER: &str = "dapDebugServer.js";

/// Where Limoniter keeps what it installs for debugging Node.
pub fn folder() -> Option<PathBuf> {
    crate::boriel_get::folder().map(|f| f.join("js-debug"))
}

/// The adapter's script, if it was installed here: the folder it's in is looked through a few levels down.
pub fn installed() -> Option<PathBuf> {
    installed_in(&folder()?)
}

fn installed_in(home: &Path) -> Option<PathBuf> {
    let mut level = vec![home.to_path_buf()];
    for _ in 0..4 {
        if let Some(found) = level.iter().map(|f| f.join(SERVER)).find(|p| p.is_file()) {
            return Some(found);
        }
        let mut next = Vec::new();
        for f in &level {
            if let Ok(entries) = std::fs::read_dir(f) {
                let mut folders: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
                folders.sort();
                next.extend(folders);
            }
        }
        level = next;
    }
    None
}

/// The command that starts the adapter (Node, with its script). It's told a port after it, and listens there.
pub fn command(script: &Path) -> String {
    format!("node \"{}\"", script.display())
}

/// The release's file to download, and its version, from GitHub's description of the latest release.
pub fn pick_asset(release: &str) -> Result<(String, String), String> {
    let json: serde_json::Value = serde_json::from_str(release).map_err(|_| crate::i18n::tr("GitHub's answer wasn't what was expected.").to_string())?;
    let tag = json["tag_name"].as_str().unwrap_or("").to_string();
    json["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.starts_with("js-debug-dap-") && n.ends_with(".tar.gz"))))
        .and_then(|a| a["browser_download_url"].as_str())
        .map(|url| (tag.clone(), url.to_string()))
        .ok_or_else(|| crate::i18n::tf!("The latest release ({tag}) has no js-debug download that Limoniter knows."))
}

/// Downloads the latest release and unpacks it in place of any before (which is kept until the new one's
/// unpacked). Gives the adapter's script and the version.
pub fn install(home: &Path, say: &dyn Fn(String)) -> Result<(PathBuf, String), String> {
    if !crate::lsp::on_path("node") {
        return Err(crate::i18n::tr("js-debug runs on Node.js, which isn't installed. It's at nodejs.org.").into());
    }
    say(crate::i18n::tr("Looking for the latest js-debug\u{2026}").into());
    let release = crate::emulator_get::get(LATEST)?;
    let (version, address) = pick_asset(&String::from_utf8_lossy(&release))?;
    say(crate::i18n::tf!("Downloading js-debug {version} from {address}\u{2026}"));
    let bytes = crate::emulator_get::get(&address)?;
    say(crate::i18n::tf!("Unpacking it ({} MB)\u{2026}", bytes.len().div_ceil(1 << 20)));
    let script = unpack_into(&bytes, home)?;
    Ok((script, version))
}

/// Unpacks a release beside the folder it goes in, then puts it in that folder's place.
fn unpack_into(bytes: &[u8], home: &Path) -> Result<PathBuf, String> {
    let parent = home.parent().ok_or_else(|| crate::i18n::tr("There's nowhere to keep it.").to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", parent.display()))?;
    let fresh = parent.join("js-debug-new");
    let _ = std::fs::remove_dir_all(&fresh);
    let done = crate::media::gunzip_bytes(bytes).and_then(|tar| crate::emulator_get::untar(&tar, &fresh));
    if let Err(problem) = done {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(problem);
    }
    if installed_in(&fresh).is_none() {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(crate::i18n::tf!("The download was unpacked, but {SERVER} wasn't in it."));
    }
    crate::emulator_get::put_in_place(&fresh, home)?;
    installed_in(home).ok_or_else(|| crate::i18n::tf!("The download was unpacked, but {SERVER} wasn't in it."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_file_is_picked() {
        let release = r#"{"tag_name": "v1.96.0", "assets": [
            {"name": "js-debug-dap-v1.96.0.tar.gz", "browser_download_url": "https://github.com/microsoft/vscode-js-debug/releases/download/v1.96.0/js-debug-dap-v1.96.0.tar.gz"},
            {"name": "js-debug-vsix-v1.96.0.vsix", "browser_download_url": "https://example.test/x.vsix"}]}"#;
        let (version, address) = pick_asset(release).unwrap();
        assert_eq!(version, "v1.96.0");
        assert!(address.ends_with("js-debug-dap-v1.96.0.tar.gz"));
        assert!(pick_asset(r#"{"tag_name": "v2", "assets": []}"#).is_err());
        assert!(pick_asset("not json").is_err());
    }

    #[test]
    fn the_adapter_is_found_where_it_was_unpacked() {
        let home = std::env::temp_dir().join(format!("limoniter-jsdebug-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(installed_in(&home), None);
        std::fs::create_dir_all(home.join("js-debug").join("src")).unwrap();
        std::fs::write(home.join("js-debug").join("src").join(SERVER), "").unwrap();
        let found = installed_in(&home).unwrap();
        assert!(found.ends_with("src/dapDebugServer.js") || found.ends_with("src\\dapDebugServer.js"));
        assert!(command(&found).starts_with("node \""));
        std::fs::remove_dir_all(&home).unwrap();
    }
}
