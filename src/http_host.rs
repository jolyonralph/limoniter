//! The window's side of `.http` files: a bar above the file with a button for each request (and Ctrl+Alt+R for the
//! one the cursor is in), the request sent on a thread of its own, the response in a panel beside the file, and the
//! environment (`local`, `staging`, ...) shown in the status bar. What the files say is read in `http_file.rs`, and
//! how they're sent is in `http_send.rs`.

use super::*;
use crate::http_file::{self, BodyKind, CookieJar, Stored, Tone};
use crate::http_send::Reply;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};

/// What the status bar's environment menu leaves for the window to take in.
pub(super) const STATUS_PICKED: &str = "http-environment-picked";

/// The most of a body that is laid out for reading, in characters.
const MOST_SHOWN: usize = 100_000;

#[derive(Default)]
pub(super) struct State {
    /// The cookies each file's responses have set.
    jars: HashMap<PathBuf, CookieJar>,
    /// What each file's named requests' responses said, for the ones after them.
    named: HashMap<PathBuf, HashMap<String, Stored>>,
    /// The environment chosen for each project folder, by name. Kept in the settings file as
    /// `http_env=folder<TAB>name`.
    pub(super) kept: Vec<(PathBuf, String)>,
    pending: Option<Pending>,
    pub(super) shown: Option<Shown>,
    /// The file as last read, for the bar: the document, its text's version, and what it says.
    read: Option<(usize, u64, http_file::HttpFile)>,
}

struct Pending {
    receive: Receiver<(Result<Reply, String>, CookieJar)>,
    file: PathBuf,
    name: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Body,
    Headers,
}

/// A response, laid out for the panel.
pub(super) struct Shown {
    file: PathBuf,
    title: String,
    curl: String,
    /// None while it's being sent.
    pub(super) reply: Option<Result<Reply, String>>,
    pub(super) kind: BodyKind,
    /// The body as it's shown: laid out, and cut into stretches by what each is.
    pub(super) spans: Vec<(String, Tone)>,
    pub(super) cut: bool,
    picture: Option<crate::pictures::Picture>,
    texture: Option<egui::TextureHandle>,
    tab: Tab,
}

impl State {
    pub(super) fn from_settings(settings: &str) -> Self {
        let kept = settings.lines().filter_map(|l| l.strip_prefix("http_env=")).filter_map(|v| v.split_once('\t')).map(|(f, n)| (PathBuf::from(f), n.to_string())).collect();
        State { kept, ..State::default() }
    }

    pub(super) fn to_settings(&self) -> String {
        self.kept.iter().map(|(f, n)| format!("http_env={}\t{n}\n", f.display())).collect()
    }

    pub(super) fn chosen_for(&self, folder: &Path) -> Option<String> {
        self.kept.iter().find(|(f, _)| f == folder).map(|(_, n)| n.clone())
    }

    pub(super) fn choose(&mut self, folder: &Path, name: Option<String>) {
        self.kept.retain(|(f, _)| f != folder);
        if let Some(name) = name {
            self.kept.push((folder.to_path_buf(), name));
        }
    }
}

/// Whether a document is an HTTP file: by its name, or because it's coloured as one.
pub(super) fn is_http_doc(doc: &Document) -> bool {
    !doc.hex_mode && doc.browser.is_none() && (doc.path.as_deref().is_some_and(http_file::is_http_name) || doc.language_name() == highlight::HTTP_REQUESTS)
}

fn size_in_words(bytes: usize) -> String {
    match bytes {
        0..=1023 => crate::i18n::tf!("{bytes} bytes"),
        1024..=1_048_575 => crate::i18n::tf!("{:.1} KB", bytes as f64 / 1024.0),
        _ => crate::i18n::tf!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

/// The body laid out, whatever it is.
pub(super) fn lay_out(reply: &Reply) -> (BodyKind, Vec<(String, Tone)>, bool, Option<crate::pictures::Picture>) {
    let content_type = reply.header("content-type").unwrap_or("");
    let mut kind = http_file::body_kind(content_type, &reply.body);
    let mut picture = None;
    if kind == BodyKind::Image {
        let svg = content_type.contains("svg") || reply.body.starts_with(b"<svg");
        picture = if svg {
            crate::preview::svg_picture(&reply.body, None).ok()
        } else {
            crate::pictures::detect("", &reply.body).filter(|f| matches!(f, crate::pictures::Format::Png | crate::pictures::Format::Jpeg | crate::pictures::Format::Bmp)).and_then(|f| crate::pictures::decode(f, &reply.body).ok())
        };
        // A picture that can't be drawn here (a GIF, a WebP) is shown as the bytes it is.
        if picture.is_none() {
            kind = BodyKind::Binary;
        }
    }
    let text = String::from_utf8_lossy(&reply.body).into_owned();
    let (text, tone): (String, fn(&str) -> Vec<(String, Tone)>) = match kind {
        BodyKind::Json => (http_file::pretty_json(&text).unwrap_or(text), http_file::colour_json),
        BodyKind::Xml | BodyKind::Html => (http_file::indent_markup(&text), http_file::colour_markup),
        BodyKind::Image => (String::new(), |s| vec![(s.to_string(), Tone::Plain)]),
        BodyKind::Binary => {
            let dump: Vec<String> = reply.body.chunks(16).take(16).enumerate().map(|(row, bytes)| format!("{:04x}  {:<48}{}", row * 16, bytes.iter().map(|b| format!("{b:02x} ")).collect::<String>(), bytes.iter().map(|&b| if (32..127).contains(&b) { b as char } else { '.' }).collect::<String>())).collect();
            (dump.join("\n"), |s| vec![(s.to_string(), Tone::Plain)])
        }
        BodyKind::Text => (text, |s| vec![(s.to_string(), Tone::Plain)]),
    };
    let cut = text.chars().count() > MOST_SHOWN;
    let text: String = if cut { text.chars().take(MOST_SHOWN).collect() } else { text };
    (kind, tone(&text), cut, picture)
}

fn tone_colour(tone: Tone) -> egui::Color32 {
    let rainbow = theme::rainbow();
    match tone {
        Tone::Plain => theme::text(),
        Tone::Key | Tone::Tag => theme::accent(),
        Tone::Text => rainbow[2],
        Tone::Number => rainbow[4],
        Tone::Keyword => rainbow[0],
        Tone::Attribute => rainbow[3],
        Tone::Comment => theme::text_faint(),
    }
}

enum Act {
    Save,
    CopyCurl,
    CopyBody,
    ClearCookies,
}

impl Limoniter {
    fn http_key(&self) -> PathBuf {
        self.docs[self.active].path.clone().unwrap_or_default()
    }

    /// The text of the file in front.
    fn http_text(&self) -> String {
        let text = &self.docs[self.active].text;
        (0..text.line_count()).map(|i| text.line(i).to_string()).collect::<Vec<_>>().join("\n")
    }

    /// What the file in front says, read again only when its text has changed.
    fn http_read(&mut self) -> http_file::HttpFile {
        let version = self.docs[self.active].text.version();
        if let Some((doc, was, file)) = &self.http_ui.read
            && *doc == self.active
            && *was == version
        {
            return file.clone();
        }
        let file = http_file::parse(&self.http_text());
        self.http_ui.read = Some((self.active, version, file.clone()));
        file
    }

    /// The folder an HTTP file's environment choice is kept for: its project's.
    fn http_folder(&self) -> PathBuf {
        self.docs[self.active].path.as_deref().map(crate::tasks::root_for).unwrap_or_default()
    }

    /// The environment chosen for the file in front, if the project still has it.
    fn http_environment(&self, names: &[String]) -> Option<String> {
        self.http_ui.chosen_for(&self.http_folder()).filter(|chosen| names.contains(chosen))
    }

    /// Where the links "Send request" go: the line each is drawn on, and the request it sends. A link goes on the
    /// line above the request when that is blank, a `###` or a comment, as the REST Client puts it, else on its own.
    fn http_link_lines(&mut self) -> Vec<(usize, usize)> {
        if self.no_file() || !is_http_doc(&self.docs[self.active]) {
            return Vec::new();
        }
        let file = self.http_read();
        let text = &self.docs[self.active].text;
        file.requests
            .iter()
            .enumerate()
            .map(|(number, request)| {
                let above = request.line.checked_sub(1).filter(|&l| {
                    let t = text.line(l);
                    let t = t.trim_start();
                    t.is_empty() || t.starts_with('#') || t.starts_with("//")
                });
                (above.unwrap_or(request.line), number)
            })
            .collect()
    }

    /// Sends the request in front: the one the cursor is in, or the one numbered.
    pub(super) fn http_send_request(&mut self, which: Option<usize>) {
        if self.no_file() || !is_http_doc(&self.docs[self.active]) {
            return;
        }
        if self.http_ui.pending.is_some() {
            return self.message("HTTP request", crate::i18n::tr("A request is still being sent. Wait for its answer first.").into());
        }
        let file = self.http_read();
        let line = self.docs[self.active].text.caret_char_position().0;
        let Some(number) = which.or_else(|| http_file::request_at(&file.requests, line)) else {
            return self.message("HTTP request", crate::i18n::tr("There's no request in this file. A request is a line such as GET https://example.com/, with its headers under it.").into());
        };
        let request = file.requests[number].clone();
        let path = self.docs[self.active].path.clone();
        let key = self.http_key();
        let environments = path.as_deref().map(http_file::environments_for).unwrap_or_default();
        let chosen = self.http_environment(&environments.names());
        let variables = environments.variables(chosen.as_deref());
        let root = path.as_deref().map(crate::tasks::root_for);
        let dotenv = [root.as_deref(), path.as_deref().and_then(Path::parent)].into_iter().flatten().find_map(|folder| std::fs::read_to_string(folder.join(".env")).ok()).map(|text| crate::env_file::parse(&text)).unwrap_or_default();
        let folder = http_file::folder_of(path.as_deref());
        let responses = self.http_ui.named.get(&key).cloned().unwrap_or_default();
        let scope = http_file::Scope { file: &file.variables, environment: &variables, dotenv: &dotenv, responses: &responses, folder: &folder };
        let resolved = match http_file::resolve(&request, &scope) {
            Ok(resolved) => resolved,
            Err(problem) => return self.message("HTTP request", problem),
        };
        let mut jar = self.http_ui.jars.get(&key).cloned().unwrap_or_default();
        let (send, receive) = std::sync::mpsc::channel();
        let sending = resolved.clone();
        std::thread::spawn(move || {
            let result = crate::http_send::send(&sending, &mut jar);
            let _ = send.send((result, jar));
        });
        self.http_ui.pending = Some(Pending { receive, file: key.clone(), name: request.name.clone() });
        self.http_ui.shown = Some(Shown { file: key, title: format!("{} {}", resolved.method, resolved.url), curl: http_file::to_curl(&resolved), reply: None, kind: BodyKind::Text, spans: Vec::new(), cut: false, picture: None, texture: None, tab: Tab::Body });
    }

    /// Takes in an answer when it comes, and the environment chosen from the status bar. True while a request is out.
    pub(super) fn http_tick(&mut self, ctx: &Context) -> bool {
        if let Some(picked) = ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::unique(STATUS_PICKED))) {
            let folder = self.http_folder();
            self.http_ui.choose(&folder, Some(picked).filter(|name| !name.is_empty()));
        }
        let links = self.http_link_lines();
        if let Some(doc) = self.docs.get_mut(self.active) {
            doc.text_state.http_links = links;
        }
        let Some(pending) = &self.http_ui.pending else { return false };
        let answer = match pending.receive.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => (Err(crate::i18n::tr("The request stopped before it was answered.").to_string()), CookieJar::default()),
        };
        let Some(pending) = self.http_ui.pending.take() else { return false };
        let (result, jar) = answer;
        if result.is_ok() {
            self.http_ui.jars.insert(pending.file.clone(), jar);
        }
        if let (Ok(reply), Some(name)) = (&result, &pending.name) {
            let stored = Stored { status: reply.status, headers: reply.headers.clone(), body: String::from_utf8_lossy(&reply.body).into_owned() };
            self.http_ui.named.entry(pending.file.clone()).or_default().insert(name.clone(), stored);
        }
        if let Some(shown) = &mut self.http_ui.shown
            && shown.file == pending.file
        {
            if let Ok(reply) = &result {
                (shown.kind, shown.spans, shown.cut, shown.picture) = lay_out(reply);
            }
            shown.reply = Some(result);
        }
        false
    }

    pub(super) fn http_clear_cookies(&mut self) {
        let key = self.http_key();
        let had = self.http_ui.jars.get(&key).map_or(0, CookieJar::len);
        self.http_ui.jars.remove(&key);
        let words = if had == 0 { crate::i18n::tr("This file has no cookies kept.").to_string() } else { crate::i18n::tf!("{had} cookies cleared.") };
        self.script_print(words, false);
    }

    /// The environment in the status bar, for an HTTP file; clicking it chooses another.
    pub(super) fn http_status(&self, ui: &mut Ui) {
        if self.no_file() || !is_http_doc(&self.docs[self.active]) {
            return;
        }
        let environments = self.docs[self.active].path.as_deref().map(http_file::environments_for).unwrap_or_default();
        let names = environments.names();
        let chosen = self.http_environment(&names);
        let label = match &chosen {
            Some(name) => crate::i18n::tf!("Environment: {name}"),
            None if names.is_empty() => crate::i18n::tr("No environments").to_string(),
            None => crate::i18n::tr("No environment chosen").to_string(),
        };
        let shown = ui.add(egui::Label::new(RichText::new(label.clone()).color(if chosen.is_some() { theme::text_faint() } else { theme::text_dim() })).sense(egui::Sense::click()));
        crate::access::button(&shown, &label);
        let shown = shown.on_hover_text(crate::i18n::tr("The environment whose variables {{name}} takes values from. Click to choose another. They're kept in http-client.env.json, or in .vscode/settings.json as rest-client.environmentVariables"));
        crate::fit::popup_menu(&shown, |ui| {
            ui.set_min_width(180.0);
            let pick = |ui: &mut Ui, name: &str| ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(STATUS_PICKED), name.to_string()));
            for name in &names {
                if ui.add(egui::Button::selectable(chosen.as_deref() == Some(name.as_str()), name)).clicked() {
                    pick(ui, name);
                    ui.close();
                }
            }
            if names.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("Write http-client.env.json beside the file, as {\"local\": {\"host\": \"localhost\"}}")).color(theme::text_faint()));
            } else if ui.add(egui::Button::selectable(chosen.is_none(), crate::i18n::tr("No environment"))).clicked() {
                pick(ui, "");
                ui.close();
            }
        });
    }

    /// The response panel, if there's a response to show.
    pub(super) fn http_panel(&mut self, ui: &mut Ui) {
        let waiting = self.http_ui.pending.is_some();
        let cookies = self.http_ui.shown.as_ref().map_or(0, |s| self.http_ui.jars.get(&s.file).map_or(0, CookieJar::len));
        let kept = self.http_ui.shown.as_ref().and_then(|s| self.http_ui.jars.get(&s.file)).map(|jar| jar.described().join("\n")).unwrap_or_default();
        let Some(shown) = &mut self.http_ui.shown else { return };
        let mut open = true;
        let mut act = None;
        panels::dock(ui, true, "http-response", "Response", 480.0, &mut open, |ui| {
            ui.label(RichText::new(&shown.title).monospace().size(12.0).color(theme::text_dim()));
            match &shown.reply {
                None => {
                    ui.label(RichText::new(if waiting { crate::i18n::tr("Sending\u{2026}") } else { crate::i18n::tr("No answer") }).color(theme::accent()));
                }
                Some(Err(problem)) => {
                    ui.add(egui::Label::new(RichText::new(problem).color(theme::problem_mark())).wrap());
                }
                Some(Ok(reply)) => {
                    let colour = match reply.status {
                        200..=299 => theme::accent(),
                        300..=399 => theme::text(),
                        _ => theme::problem_mark(),
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("{} {}", reply.status, reply.reason)).strong().color(colour));
                        ui.label(RichText::new(crate::i18n::tf!("{} ms", reply.millis)).color(theme::text_dim()));
                        ui.label(RichText::new(size_in_words(reply.body.len())).color(theme::text_dim()));
                    });
                    for hop in &reply.hops {
                        ui.add(egui::Label::new(RichText::new(crate::i18n::tf!("Redirected ({}): {} \u{2192} {}", hop.status, hop.from, hop.to)).size(11.5).color(theme::text_faint())).wrap());
                    }
                    if !reply.hops.is_empty() {
                        ui.add(egui::Label::new(RichText::new(crate::i18n::tf!("Answered by {}", reply.url)).size(11.5).color(theme::text_faint())).wrap());
                    }
                }
            }
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(matches!(shown.reply, Some(Ok(_))), egui::Button::new(crate::i18n::tr("Save body\u{2026}"))).on_hover_text(crate::i18n::tr("Writes the body, exactly as it came, to a file")).clicked() {
                    act = Some(Act::Save);
                }
                if ui.button(crate::i18n::tr("Copy as curl")).on_hover_text(crate::i18n::tr("Copies a curl command that sends this request")).clicked() {
                    act = Some(Act::CopyCurl);
                }
                if ui.add_enabled(matches!(shown.reply, Some(Ok(_))), egui::Button::new(crate::i18n::tr("Copy body"))).clicked() {
                    act = Some(Act::CopyBody);
                }
                if cookies > 0 && ui.button(crate::i18n::tf!("Clear cookies ({cookies})")).on_hover_text(kept).clicked() {
                    act = Some(Act::ClearCookies);
                }
            });
            if let Some(Ok(reply)) = &shown.reply {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut shown.tab, Tab::Body, crate::i18n::tr("Body"));
                    ui.selectable_value(&mut shown.tab, Tab::Headers, crate::i18n::tf!("Headers ({})", reply.headers.len()));
                });
                ui.separator();
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| match shown.tab {
                    Tab::Headers => {
                        egui::Grid::new("http-headers").num_columns(2).striped(true).show(ui, |ui| {
                            for (name, value) in &reply.headers {
                                ui.label(RichText::new(name).monospace().size(12.0).color(theme::accent()));
                                ui.add(egui::Label::new(RichText::new(value).monospace().size(12.0)).wrap());
                                ui.end_row();
                            }
                        });
                    }
                    Tab::Body => {
                        if let Some(picture) = &shown.picture {
                            let texture = shown.texture.get_or_insert_with(|| ui.ctx().load_texture("http-response-picture", egui::ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &picture.rgba), egui::TextureOptions::LINEAR));
                            let scale = (ui.available_width() / picture.width as f32).clamp(0.01, 1.0);
                            ui.add(egui::Image::new((texture.id(), egui::vec2(picture.width as f32, picture.height as f32) * scale)));
                            return;
                        }
                        if reply.body.is_empty() {
                            ui.label(RichText::new(crate::i18n::tr("The answer has no body.")).color(theme::text_faint()));
                            return;
                        }
                        if shown.kind == BodyKind::Binary {
                            ui.label(RichText::new(crate::i18n::tf!("{} that aren't text. The first of them: Save body keeps them all.", size_in_words(reply.body.len()))).color(theme::text_faint()));
                        }
                        let font = egui::FontId::monospace(12.5);
                        let mut job = egui::text::LayoutJob::default();
                        for (words, tone) in &shown.spans {
                            job.append(words, 0.0, egui::TextFormat::simple(font.clone(), tone_colour(*tone)));
                        }
                        ui.add(egui::Label::new(job).selectable(true));
                        if shown.cut {
                            ui.label(RichText::new(crate::i18n::tf!("Only the first {MOST_SHOWN} characters are shown. Save body keeps all of it.")).color(theme::text_faint()));
                        }
                    }
                });
            } else {
                ui.separator();
                ui.add(egui::Label::new(RichText::new(&shown.curl).monospace().size(11.5).color(theme::text_faint())).wrap());
            }
        });
        let curl = shown.curl.clone();
        let (reply, file) = (shown.reply.clone(), shown.file.clone());
        if !open {
            self.http_ui.shown = None;
        }
        match act {
            Some(Act::CopyCurl) => ui.ctx().copy_text(curl),
            Some(Act::CopyBody) => {
                if let Some(Ok(reply)) = &reply {
                    ui.ctx().copy_text(String::from_utf8_lossy(&reply.body).into_owned());
                }
            }
            Some(Act::ClearCookies) => {
                self.http_ui.jars.remove(&file);
            }
            Some(Act::Save) => {
                if let Some(Ok(reply)) = &reply {
                    self.http_save_body(reply);
                }
            }
            None => {}
        }
    }

    fn http_save_body(&mut self, reply: &Reply) {
        let name = web_host::name_from_address(&reply.url);
        let name = if name.contains('.') { name } else { format!("{name}.{}", body_extension(reply)) };
        let Some(path) = crate::sys_dialogs::file_dialog().set_file_name(name).save_file() else { return };
        match std::fs::write(&path, &reply.body) {
            Ok(()) => self.script_print(crate::i18n::tf!("The body was saved as {}.", path.display()), false),
            Err(e) => self.message("HTTP request", crate::i18n::tf!("{} couldn't be written: {e}", path.display())),
        }
    }
}

/// The ending a saved body is given when its address has none.
fn body_extension(reply: &Reply) -> &'static str {
    match http_file::body_kind(reply.header("content-type").unwrap_or(""), &reply.body) {
        BodyKind::Json => "json",
        BodyKind::Xml => "xml",
        BodyKind::Html => "html",
        BodyKind::Image if reply.body.starts_with(b"\x89PNG") => "png",
        BodyKind::Image => "jpg",
        BodyKind::Text => "txt",
        BodyKind::Binary => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    /// A server that answers every request the same way, and says what it was asked.
    fn serve(reply: &'static str) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut data = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let n = stream.read(&mut buffer).unwrap_or(0);
                    data.extend_from_slice(&buffer[..n]);
                    let text = String::from_utf8_lossy(&data).into_owned();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end].to_ascii_lowercase().split("content-length:").nth(1).and_then(|l| l.lines().next()).and_then(|l| l.trim().parse::<usize>().ok()).unwrap_or(0);
                        if data.len() >= end + 4 + length {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                kept.lock().unwrap().push(String::from_utf8_lossy(&data).into_owned());
                let body = if reply.is_empty() { "{\"token\":\"abc\"}" } else { reply };
                let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: sid=1; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
            }
        });
        (address, seen)
    }

    fn open_http(app: &mut Limoniter, folder: &Path, text: &str) {
        let path = folder.join("calls.http");
        std::fs::write(&path, text).unwrap();
        app.open_path(&path);
    }

    fn wait_for_answer(app: &mut Limoniter, ctx: &Context) {
        let began = Instant::now();
        while app.http_tick(ctx) {
            assert!(began.elapsed() < Duration::from_secs(20), "no answer");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("limoniter-httphost-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn the_request_at_the_cursor_is_sent_chained_and_shown() {
        let (address, seen) = serve("");
        let folder = scratch("chain");
        std::fs::write(folder.join(".env"), "API_KEY=from-dotenv\n").unwrap();
        std::fs::write(folder.join("http-client.env.json"), format!("{{\"local\": {{\"host\": \"{address}\"}}}}")).unwrap();
        let text = "### first\n# @name login\nPOST http://{{host}}/login\nContent-Type: application/json\n\n{\"key\": \"{{$dotenv API_KEY}}\"}\n\n### second\nGET http://{{host}}/me\nAuthorization: Bearer {{login.response.body.$.token}}\n";
        let (mut app, ctx) = app_with("");
        open_http(&mut app, &folder, text);
        let folder = app.http_folder();
        app.http_ui.choose(&folder, Some("local".into()));
        // The cursor is on the first request.
        app.docs[app.active].text.move_caret_to_char(2, 0);
        app.http_send_request(None);
        wait_for_answer(&mut app, &ctx);
        let shown = app.http_ui.shown.as_ref().unwrap();
        let reply = shown.reply.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(reply.status, 200);
        assert_eq!(shown.kind, BodyKind::Json);
        assert!(shown.spans.iter().any(|(s, t)| s == "\"token\"" && *t == Tone::Key));
        assert!(seen.lock().unwrap()[0].ends_with("{\"key\": \"from-dotenv\"}"), "{:?}", seen.lock().unwrap()[0]);
        // The second takes the first's token, and the cookie the first set.
        app.docs[app.active].text.move_caret_to_char(9, 0);
        app.http_send_request(None);
        wait_for_answer(&mut app, &ctx);
        let asked = seen.lock().unwrap()[1].to_ascii_lowercase();
        assert!(asked.contains("authorization: bearer abc"), "{asked}");
        assert!(asked.contains("cookie: sid=1"), "{asked}");
        assert!(app.http_ui.shown.as_ref().unwrap().curl.contains("Authorization: Bearer abc"));
        // The cookies can be cleared.
        app.http_clear_cookies();
        assert!(!app.http_ui.jars.contains_key(&app.http_key()));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_request_that_cant_be_made_up_says_why_and_sends_nothing() {
        let folder = scratch("undefined");
        let (mut app, ctx) = app_with("");
        open_http(&mut app, &folder, "GET http://{{nowhere}}/x\n");
        app.http_send_request(None);
        assert!(app.http_ui.pending.is_none());
        let Dialog::Message { body, .. } = &app.dialog else { panic!("a message was expected") };
        assert!(body.contains("nowhere"), "{body}");
        assert!(!app.http_tick(&ctx));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn each_request_gets_a_link_on_the_line_above_it_or_its_own() {
        let folder = scratch("links");
        let (mut app, ctx) = app_with("");
        open_http(&mut app, &folder, "GET http://x/1\n\n### two\n# @name two\nGET http://x/2\n###\n\nGET http://x/3\n");
        app.http_tick(&ctx);
        // The first has nothing above it; the second a comment; the third a blank line.
        assert_eq!(app.docs[app.active].text_state.http_links, [(0, 0), (3, 1), (6, 2)]);
        let plain = folder.join("notes.txt");
        std::fs::write(&plain, "GET http://x/\n").unwrap();
        app.open_path(&plain);
        app.http_tick(&ctx);
        assert!(app.docs[app.active].text_state.http_links.is_empty(), "no links in other files");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_link_sends_its_own_request() {
        let (address, seen) = serve("");
        let folder = scratch("numbered");
        let (mut app, ctx) = app_with("");
        open_http(&mut app, &folder, &format!("GET http://{address}/first\n\n###\nGET http://{address}/second\n"));
        app.http_send_request(Some(1));
        wait_for_answer(&mut app, &ctx);
        assert!(seen.lock().unwrap()[0].starts_with("GET /second"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn only_http_files_have_the_commands_and_the_environment_choice_is_kept() {
        let folder = scratch("choice");
        let (mut app, ctx) = app_with("");
        let plain = folder.join("notes.txt");
        std::fs::write(&plain, "GET http://127.0.0.1:1/\n").unwrap();
        app.open_path(&plain);
        assert!(!is_http_doc(&app.docs[app.active]));
        app.http_send_request(None);
        assert!(app.http_ui.pending.is_none() && app.http_ui.shown.is_none(), "a text file is no request file");
        open_http(&mut app, &folder, "GET http://127.0.0.1:1/\n");
        assert!(is_http_doc(&app.docs[app.active]));
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(STATUS_PICKED), "staging".to_string()));
        app.http_tick(&ctx);
        assert_eq!(app.http_ui.chosen_for(&app.http_folder()).as_deref(), Some("staging"));
        let saved = app.http_ui.to_settings();
        assert_eq!(State::from_settings(&saved).chosen_for(&app.http_folder()).as_deref(), Some("staging"), "kept in the settings");
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(STATUS_PICKED), String::new()));
        app.http_tick(&ctx);
        assert_eq!(app.http_ui.chosen_for(&app.http_folder()), None);
        assert_eq!(app.http_ui.to_settings(), "");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_failed_send_is_shown_in_the_panel_and_bodies_are_laid_out_by_kind() {
        let folder = scratch("failed");
        let (mut app, ctx) = app_with("");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        open_http(&mut app, &folder, &format!("GET http://127.0.0.1:{port}/\n"));
        app.http_send_request(None);
        wait_for_answer(&mut app, &ctx);
        let shown = app.http_ui.shown.as_ref().unwrap();
        assert!(matches!(&shown.reply, Some(Err(problem)) if problem.contains(&port.to_string())));
        assert!(shown.curl.starts_with("curl"));
        let reply = |content_type: &str, body: &[u8]| Reply { status: 200, headers: vec![("Content-Type".to_string(), content_type.to_string())], body: body.to_vec(), ..Reply::default() };
        let (kind, spans, cut, _) = lay_out(&reply("application/json", b"{\"a\":1}"));
        assert_eq!((kind, cut), (BodyKind::Json, false));
        assert_eq!(spans.iter().map(|(s, _)| s.as_str()).collect::<String>(), "{\n  \"a\": 1\n}");
        let (kind, spans, _, _) = lay_out(&reply("text/html", b"<html><body><p>Hi</p></body></html>"));
        assert_eq!(kind, BodyKind::Html);
        assert!(spans.iter().map(|(s, _)| s.as_str()).collect::<String>().contains("\n    <p>Hi</p>"));
        let (kind, spans, _, _) = lay_out(&reply("text/plain", b"just words"));
        assert_eq!((kind, spans.len()), (BodyKind::Text, 1));
        let (kind, spans, _, _) = lay_out(&reply("application/octet-stream", &[0, 1, 2, 3]));
        assert_eq!(kind, BodyKind::Binary);
        assert!(spans[0].0.starts_with("0000  00 01 02 03"));
        let (_, _, cut, _) = lay_out(&reply("text/plain", "x".repeat(MOST_SHOWN + 5).as_bytes()));
        assert!(cut, "a long body is cut for showing");
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"10\"><rect width=\"20\" height=\"10\" fill=\"red\"/></svg>";
        let (kind, _, _, picture) = lay_out(&reply("image/svg+xml", svg));
        assert_eq!(kind, BodyKind::Image);
        assert!(picture.is_some_and(|p| p.width == 2 * p.height), "drawn at its shape");
        let (kind, _, _, picture) = lay_out(&reply("image/gif", b"GIF89a\0\0\0\0"));
        assert_eq!((kind, picture.is_none()), (BodyKind::Binary, true), "a picture it can't draw is shown as bytes");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
