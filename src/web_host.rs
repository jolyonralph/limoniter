//! Opening a file from a web address (http or https): fetched in the background, kept in a temporary folder, and
//! opened as any file is (an image in the browser, text with its encoding found). A document opened this way has
//! no file of its own, so Save asks where to put it, rather than writing into the temporary copy.

use super::*;
use std::sync::mpsc::{Receiver, channel};

/// The most a download may be.
const MOST_BYTES: u64 = 512 * 1024 * 1024;

pub(super) struct WebBox {
    pub(super) address: String,
    pub(super) status: String,
    fetching: Option<Receiver<Result<(String, Vec<u8>), String>>>,
}

/// A file's bytes and the name it goes by, from a web address.
fn fetch(address: &str) -> Result<(String, Vec<u8>), String> {
    let response = ureq::get(address).header("User-Agent", "Limoniter").call().map_err(|e| format!("It couldn't be fetched: {e}"))?;
    // The name the server gives it, or the last part of the address.
    let given = response
        .headers()
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').find_map(|part| part.trim().strip_prefix("filename=")))
        .map(|n| n.trim_matches('"').to_string());
    let name = given.unwrap_or_else(|| name_from_address(address));
    let bytes = response.into_body().with_config().limit(MOST_BYTES).read_to_vec().map_err(|e| format!("It couldn't be read: {e}"))?;
    Ok((name, bytes))
}

/// The file name in an address, or "download" if it ends in a folder.
pub(super) fn name_from_address(address: &str) -> String {
    let path = address.split(['?', '#']).next().unwrap_or("");
    let path = path.split_once("://").map_or(path, |(_, rest)| rest);
    let last = path.rsplit('/').next().filter(|_| path.contains('/')).unwrap_or("");
    let decoded = percent_decode(last);
    let clean = crate::media::tidy_name(&decoded);
    if last.is_empty() || clean == "_" { "download".into() } else { clean }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(value) = text.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(value);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Limoniter {
    pub(super) fn open_web_box(&mut self) {
        self.dialog = Dialog::OpenUrl(Box::new(WebBox { address: String::new(), status: String::new(), fetching: None }));
    }

    pub(super) fn web_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::OpenUrl(state) = &mut self.dialog else {
            return false;
        };
        // A download that's finished opens.
        if let Some(rx) = &state.fetching {
            match rx.try_recv() {
                Ok(Ok((name, bytes))) => {
                    self.dialog = Dialog::None;
                    self.open_downloaded(&name, &bytes);
                    return true;
                }
                Ok(Err(problem)) => {
                    state.status = problem;
                    state.fetching = None;
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            }
        }
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut go = false;
        egui::Window::new("Open a file from the web").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("The file's web address (http or https):");
            let field = ui.add(egui::TextEdit::singleline(&mut state.address).desired_width(460.0).hint_text("https://example.com/file.txt"));
            if state.status.is_empty() && state.fetching.is_none() {
                field.request_focus();
            }
            go = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if state.fetching.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Fetching\u{2026}");
                });
            } else if !state.status.is_empty() {
                ui.colored_label(theme::change_unsaved(), &state.status);
            }
            ui.horizontal(|ui| {
                go |= ui.add_enabled(state.fetching.is_none(), Button::new("Open")).clicked();
                close |= ui.button("Cancel").clicked();
            });
        });
        if go && state.fetching.is_none() {
            let address = state.address.trim().to_string();
            let lower = address.to_ascii_lowercase();
            if !(lower.starts_with("http://") || lower.starts_with("https://")) {
                state.status = "Give a web address starting with http:// or https://.".into();
            } else {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let _ = tx.send(fetch(&address));
                });
                state.status.clear();
                state.fetching = Some(rx);
            }
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }

    /// A downloaded file opened from a temporary copy; a document that isn't an image is given no file of its
    /// own, so it's saved where the person chooses.
    pub(super) fn open_downloaded(&mut self, name: &str, bytes: &[u8]) {
        let folder = std::env::temp_dir().join("limoniter-web").join(format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())));
        let path = folder.join(name);
        if let Err(e) = std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(&path, bytes)) {
            return self.message("Open a file from the web", format!("It couldn't be kept to open: {e}"));
        }
        let before = self.docs.len();
        if !self.open_path(&path) || self.docs.len() == before && self.docs[self.active].path.as_deref() != Some(path.as_path()) {
            return;
        }
        self.panels.recent.retain(|p| p != &path);
        let doc = &mut self.docs[self.active];
        if doc.browser.is_none() && !doc.text.is_disk_backed() && !doc.hex_mode {
            doc.untitled = name.to_string();
            doc.path = None;
            doc.disk_stamp = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_comes_from_the_address() {
        assert_eq!(name_from_address("https://example.com/files/read%20me.txt?x=1"), "read me.txt");
        assert_eq!(name_from_address("https://example.com/"), "download");
        assert_eq!(name_from_address("http://example.com"), "download");
    }

    #[test]
    fn a_downloaded_text_file_opens_with_no_file_of_its_own() {
        let (mut app, _ctx) = crate::app::tests::app_with("");
        app.open_downloaded("notes.txt", b"hello from the web\n");
        let doc = &app.docs[app.active];
        assert_eq!((doc.name(), doc.path.is_none()), ("notes.txt".to_string(), true));
        assert_eq!(&*doc.text.line(0), "hello from the web");
        assert!(app.panels.recent.iter().all(|p| !p.to_string_lossy().contains("limoniter-web")));
    }
}

#[cfg(test)]
mod recent_tests {
    use std::path::PathBuf;

    #[test]
    fn cleaning_up_takes_off_files_that_are_gone() {
        let here = std::env::temp_dir().join(format!("limoniter-recent-{}.txt", std::process::id()));
        std::fs::write(&here, "x").unwrap();
        let gone = PathBuf::from("/no/such/folder/gone.txt");
        let mut list = vec![gone, here.clone()];
        crate::panels::clean_recent(&mut list);
        assert_eq!(list, std::slice::from_ref(&here));
        std::fs::remove_file(here).ok();
    }
}
