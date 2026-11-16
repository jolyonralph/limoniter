//! Tools, Save settings to a file and Restore settings from a file: everything kept in `settings.txt`, and the
//! encodings chosen for particular files, in one text file that can be kept or taken to another computer.
//!
//! The file is the settings file as it is, with a line at the top saying what it is, and each remembered encoding
//! as an `encoding_choice=` line. Passwords aren't in it: they're in the system's keychain.

/// The first line, which tells a settings file from any other text.
const HEADER: &str = "# Limoniter settings";
const ENCODING: &str = "encoding_choice=";

/// The settings and the remembered encodings as one file.
pub fn bundle(settings: &str, encodings: &str, version: &str, date: &str) -> String {
    let mut out = format!("{HEADER}, saved by version {version} on {date}.\n# Restore them with Tools, Restore settings from a file.\n");
    for line in settings.lines().filter(|line| !line.trim().is_empty()) {
        out += line;
        out.push('\n');
    }
    for line in encodings.lines().filter(|line| line.contains('\t')) {
        out += ENCODING;
        out += line;
        out.push('\n');
    }
    out
}

/// A saved file back as the settings file's text and the encodings file's.
pub fn unbundle(text: &str) -> Result<(String, String), String> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    if !text.starts_with(HEADER) {
        return Err("This isn't a file of Limoniter's settings: they start with \u{201C}# Limoniter settings\u{201D}.".into());
    }
    let (mut settings, mut encodings) = (String::new(), String::new());
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(choice) = line.strip_prefix(ENCODING) {
            encodings += choice;
            encodings.push('\n');
        } else if line.contains('=') {
            settings += line;
            settings.push('\n');
        }
    }
    Ok((settings, encodings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_come_back_as_they_were_saved() {
        let settings = "ruler=true\nrecent_file=C:\\a b\\c.txt\nfind_history=x = y\n";
        let encodings = "d:\\notes.txt\tamiga-text\tstandard\tplain\t0\n";
        let saved = bundle(settings, encodings, "0.10.0", "2026-11-15");
        assert!(saved.starts_with("# Limoniter settings, saved by version 0.10.0 on 2026-11-15."));
        assert_eq!(unbundle(&saved).unwrap(), (settings.to_string(), encodings.to_string()));
        let windows = saved.replace('\n', "\r\n");
        assert_eq!(unbundle(&format!("\u{FEFF}{windows}")).unwrap(), (settings.to_string(), encodings.to_string()));
    }

    #[test]
    fn other_files_are_turned_away() {
        assert!(unbundle("ruler=true\n").is_err());
        assert!(unbundle("").is_err());
    }
}
