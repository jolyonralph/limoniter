//! The adapter that debugs PHP: VS Code's vscode-php-debug (`xdebug.php-debug`), a Node.js program that listens for
//! Xdebug, PHP's own debugging extension. Limoniter downloads it, when asked, from Open VSX, the open extension
//! registry, and keeps it in a folder beside its settings; nothing else on the computer is changed. Xdebug itself
//! is a PHP extension, which the person's PHP needs (`pecl install xdebug`, or their system's `php-xdebug`).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

/// Where the extension's description (and the address of its latest download) is.
pub const REGISTRY: &str = "https://open-vsx.org/api/xdebug/php-debug/latest";

/// The port Xdebug 3 connects to, which the adapter listens on.
pub const PORT: u16 = 9003;

/// Where the adapter is kept.
pub fn folder() -> Option<PathBuf> {
    crate::boriel_get::folder().map(|f| f.join("php-debug"))
}

/// The adapter's program, if it's been downloaded.
pub fn script() -> Option<PathBuf> {
    script_in(&folder()?)
}

fn script_in(home: &Path) -> Option<PathBuf> {
    Some(home.join("extension").join("out").join("phpDebug.js")).filter(|p| p.is_file())
}

/// The command that starts the adapter: Node.js running it. None while it's not downloaded.
pub fn adapter_command() -> Option<String> {
    let script = script()?;
    Some(format!("node {}", crate::test_runners::quote(&script.display().to_string())))
}

/// The address of the extension's file in the registry's description of the latest version.
pub fn download_address(registry: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(registry).ok()?;
    json.get("files")?.get("download")?.as_str().filter(|a| a.starts_with("https://")).map(str::to_string)
}

/// Downloads the adapter into a folder, replacing what was there.
pub fn install(home: &Path) -> Result<PathBuf, String> {
    let registry = crate::emulator_get::get(REGISTRY)?;
    let address = download_address(&String::from_utf8_lossy(&registry)).ok_or_else(|| crate::i18n::tr("The registry's answer wasn't what was expected.").to_string())?;
    let bytes = crate::emulator_get::get(&address)?;
    let fresh = home.with_extension("new");
    let _ = std::fs::remove_dir_all(&fresh);
    crate::emulator_get::unzip(bytes, &fresh)?;
    if script_in(&fresh).is_none() {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(crate::i18n::tr("The download was unpacked, but phpDebug.js wasn't in it.").into());
    }
    let _ = std::fs::remove_dir_all(home);
    std::fs::rename(&fresh, home).map_err(|e| crate::i18n::tf!("{} couldn't be renamed: {e}", fresh.display()))?;
    script_in(home).ok_or_else(|| crate::i18n::tr("The download was unpacked, but phpDebug.js wasn't in it.").into())
}

/// A download going on, and what's to be debugged once it's done.
pub type Download = (Receiver<Result<PathBuf, String>>, Option<crate::dap::Config>);

/// The download going on, if there is one.
pub static DOWNLOADING: Mutex<Option<Download>> = Mutex::new(None);

/// Starts the download on a thread of its own, unless one's going. What's to be debugged is kept, to be started
/// when it's done.
pub fn start_download(then: Option<crate::dap::Config>) -> Result<(), String> {
    let mut going = DOWNLOADING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if going.is_some() {
        return Ok(());
    }
    let home = folder().ok_or_else(|| crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found.").to_string())?;
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(install(&home));
    });
    *going = Some((receive, then));
    Ok(())
}

/// Whether the PHP on the PATH has Xdebug loaded; None if PHP can't be run.
pub fn php_has_xdebug() -> Option<bool> {
    let output = crate::platform::command("php").args(["-r", "echo extension_loaded('xdebug') ? 'yes' : 'no';"]).stdin(std::process::Stdio::null()).output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).trim() == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_names_the_download() {
        let said = "{\"namespace\": \"xdebug\", \"version\": \"1.36.0\", \"files\": {\"download\": \"https://open-vsx.org/api/xdebug/php-debug/1.36.0/file/xdebug.php-debug-1.36.0.vsix\"}}";
        assert_eq!(download_address(said).as_deref(), Some("https://open-vsx.org/api/xdebug/php-debug/1.36.0/file/xdebug.php-debug-1.36.0.vsix"));
        assert_eq!(download_address("{\"files\": {\"download\": \"http://plain.example/x.vsix\"}}"), None, "only over https");
        assert_eq!(download_address("not json"), None);
    }

    #[test]
    fn the_adapter_is_found_where_it_was_unpacked() {
        let home = std::env::temp_dir().join(format!("limoniter-xdebug-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(script_in(&home), None);
        std::fs::create_dir_all(home.join("extension").join("out")).unwrap();
        std::fs::write(home.join("extension").join("out").join("phpDebug.js"), "// adapter\n").unwrap();
        assert!(script_in(&home).is_some());
        let _ = std::fs::remove_dir_all(&home);
    }
}
