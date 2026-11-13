//! The window's side of sending to devices: File, Send to device; the box that sends; the box where devices are
//! set up; and following each send until it's done.

use super::script_host::After;
use super::*;
use crate::addons::Event;
use crate::deploy::{self, Device, Failure, Job, Outcome, Payload, Place, Sending, Stamp};
use std::collections::HashMap;
use std::time::Instant;

/// How long a word in the status bar about a finished send stays.
const NOTE_FOR: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(super) struct DriveScan {
    found: Option<Vec<deploy::Drive>>,
    when: Option<Instant>,
    looking: bool,
}

/// A send waiting for the add-ons' on_send functions to run first.
pub(super) struct PendingSend {
    pub doc_id: u64,
    pub device: Device,
    pub name: String,
}

/// Everything about sending, kept by the window.
pub(super) struct Sends {
    pub devices: Vec<Device>,
    pub jobs: Vec<Job>,
    /// Serving files to a Spectrum Next, once something has been sent to one.
    pub nextsync: Option<deploy::NextSync>,
    /// The port to listen on for the Next; 0 in a test, for any free one.
    pub next_port: u16,
    pub number: u64,
    /// For each file: the device it was last sent to, and the name it was sent as; latest first.
    pub last: Vec<(String, String, String)>,
    /// Each file on a card, folder or server as it was left when last sent there.
    pub stamps: Vec<(String, Stamp)>,
    /// Passwords typed this session, for FTP and the Ultimate.
    pub passwords: HashMap<String, String>,
    /// The drives plugged in, looked for in the background no more than every couple of seconds, as a drive
    /// can be slow to answer. None until the first look has finished.
    drives: Arc<Mutex<DriveScan>>,
    /// What the status bar says about the last send, and since when.
    pub note: Option<(String, String, Instant)>,
    told_next: bool,
}

impl Sends {
    pub(super) fn from_settings(settings: &str) -> Self {
        let scan = Arc::new(Mutex::new(DriveScan::default()));
        let devices = list_setting(settings, "send_device").iter().filter_map(|value| Device::from_setting(value)).collect();
        let last = list_setting(settings, "send_last")
            .iter()
            .filter_map(|value| {
                let mut parts = value.split('\u{1}');
                Some((parts.next()?.to_string(), parts.next()?.to_string(), parts.next()?.to_string()))
            })
            .collect();
        let stamps = list_setting(settings, "send_stamp")
            .iter()
            .filter_map(|value| {
                let mut parts = value.split('\u{1}');
                let key = parts.next()?.to_string();
                Some((key, Stamp { size: parts.next()?.parse().ok()?, modified: parts.next()?.parse().ok()? }))
            })
            .collect();
        let sends = Sends { devices, jobs: Vec::new(), nextsync: None, next_port: deploy::NEXTSYNC_PORT, number: 0, last, stamps, passwords: HashMap::new(), drives: scan, note: None, told_next: false };
        // A first look, so the menu knows which cards are in by the time it's opened.
        if sends.devices.iter().any(|device| matches!(device.place, Place::Card { .. })) {
            let _ = sends.drives();
        }
        sends
    }

    pub(super) fn to_settings(&self) -> String {
        let mut out: String = self.devices.iter().map(Device::to_setting).collect();
        // Untitled files are remembered only while they're open.
        out += &self.last.iter().filter(|(key, ..)| !key.starts_with("untitled:")).take(100).map(|(key, device, name)| format!("send_last={key}\u{1}{device}\u{1}{name}\n")).collect::<String>();
        out += &self.stamps.iter().take(200).map(|(key, stamp)| format!("send_stamp={key}\u{1}{}\u{1}{}\n", stamp.size, stamp.modified)).collect::<String>();
        out
    }

    /// The drives plugged in, as last found; a fresh look is started if that was a while ago. None before the
    /// first look has finished.
    pub(super) fn drives(&self) -> Option<Vec<deploy::Drive>> {
        let mut scan = self.drives.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let stale = scan.when.is_none_or(|when| when.elapsed() > Duration::from_secs(2));
        if stale && !scan.looking {
            scan.looking = true;
            let shared = self.drives.clone();
            std::thread::spawn(move || {
                let found = deploy::drives();
                let mut scan = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                scan.found = Some(found);
                scan.when = Some(Instant::now());
                scan.looking = false;
            });
        }
        scan.found.clone()
    }

    /// Whether a card is plugged in: None if that isn't known yet.
    pub(super) fn card_present(&self, label: &str) -> Option<bool> {
        Some(self.drives()?.iter().any(|drive| drive.label.eq_ignore_ascii_case(label.trim())))
    }

    fn remember(&mut self, file: &str, device: &str, name: &str) {
        self.last.retain(|(key, ..)| key != file);
        self.last.insert(0, (file.to_string(), device.to_string(), name.to_string()));
        self.last.truncate(150);
    }

    fn stamp_key(device: &Device, name: &str) -> String {
        format!("{}\u{2}{}", device.place.describe(), name).replace('\u{1}', " ")
    }

    fn set_stamp(&mut self, key: String, stamp: Stamp) {
        self.stamps.retain(|(known, _)| *known != key);
        self.stamps.insert(0, (key, stamp));
        self.stamps.truncate(250);
    }

    fn password(&self, device: &Device) -> Option<String> {
        let key = device.place.secret_key()?;
        self.passwords.get(&key).cloned().or_else(|| deploy::stored_password(&key))
    }
}

/// The box that sends a file: its name there, and a password if one is needed.
pub(super) struct SendBox {
    pub doc_id: u64,
    pub device: usize,
    pub name: String,
    pub password: String,
    pub remember: bool,
    /// Ask for a password, as the device refused without one.
    pub ask_password: bool,
    pub problem: String,
    pub focus: bool,
}

/// The box where devices are set up.
pub(super) struct DevicesBox {
    pub devices: Vec<Device>,
    pub selected: usize,
    /// A password typed for the device selected, and whether to keep it.
    pub password: String,
    pub keep: bool,
    /// How many emulators Find emulators added, once it's been used.
    pub found: Option<usize>,
    /// Something to say about the device selected, such as an emulator that wasn't found.
    pub told: String,
}

/// The key a file is remembered by: its path, or for one never saved, its tab.
fn file_key(doc: &Document) -> String {
    doc.path.as_ref().map_or_else(|| format!("untitled:{}", doc.id), |path| path.display().to_string())
}

/// Whether a device needs a password typed before sending, as none is kept.
fn wants_password(sends: &Sends, device: &Device) -> bool {
    matches!(&device.place, Place::Ftp { user, .. } if !user.is_empty() && !user.eq_ignore_ascii_case("anonymous")) && sends.password(device).is_none()
}

impl Limoniter {
    /// Opens the box to send the file in front to a device.
    pub(super) fn open_send_box(&mut self, device: usize) {
        let Some(chosen) = self.sends.devices.get(device) else {
            return;
        };
        let doc = &self.docs[self.active];
        let key = file_key(doc);
        // A listing goes as its program, under the program's name.
        let default_name = || match &doc.listing_of {
            Some((machine, stem)) => format!("{stem}.{}", machine.ending()),
            None => doc.name(),
        };
        let name = self.sends.last.iter().find(|(file, known, _)| *file == key && *known == chosen.name).map_or_else(default_name, |(_, _, name)| name.clone());
        let ask_password = wants_password(&self.sends, chosen);
        self.dialog = Dialog::Send(Box::new(SendBox { doc_id: doc.id, device, name, password: String::new(), remember: true, ask_password, problem: String::new(), focus: true }));
    }

    /// Sends the file in front where it was last sent, under the same name; or asks where, the first time.
    pub(super) fn send_again(&mut self) {
        let doc = &self.docs[self.active];
        let key = file_key(doc);
        let last = self.sends.last.iter().find(|(file, ..)| *file == key).cloned();
        let found = last.as_ref().and_then(|(_, device, name)| self.sends.devices.iter().position(|d| d.name == *device).map(|n| (n, name.clone())));
        match found {
            Some((n, name)) if !wants_password(&self.sends, &self.sends.devices[n]) => {
                let device = self.sends.devices[n].clone();
                self.begin_send(self.active, device, name);
            }
            Some((n, _)) => self.open_send_box(n),
            None if self.sends.devices.len() == 1 => self.open_send_box(0),
            None if self.sends.devices.is_empty() => self.open_devices_box(),
            None => self.message("Send again", "This file hasn't been sent anywhere yet. Choose a device under File, Send to device, and after that Send again (Ctrl+Alt+S) sends it there in one go.".into()),
        }
    }

    /// Starts a send, running the add-ons' on_send functions first if any are on.
    pub(super) fn begin_send(&mut self, index: usize, device: Device, name: String) {
        let doc = &self.docs[index];
        let id = doc.id;
        let key = file_key(doc);
        let text_view = !doc.hex_mode && doc.browser.is_none();
        self.sends.remember(&key, &device.name, &name);
        self.save_settings_if_changed();
        self.send_pending = Some(PendingSend { doc_id: id, device, name });
        if self.script.is_none() && text_view && self.queue_hooks(Event::Send, id) {
            if let Some(last) = self.addon_queue.back_mut() {
                last.after = Some(After::Send(id));
            }
            return;
        }
        self.finish_send(id);
    }

    /// Works out what's sent and sends it, once any add-ons have had their turn.
    pub(super) fn finish_send(&mut self, doc_id: u64) {
        let Some(pending) = self.send_pending.take_if(|pending| pending.doc_id == doc_id) else {
            return;
        };
        let Some(index) = self.docs.iter().position(|doc| doc.id == doc_id) else {
            return;
        };
        // An add-on may have renamed it; it's remembered under the name it went as.
        let key = file_key(&self.docs[index]);
        self.sends.remember(&key, &pending.device.name, &pending.name);
        let payload = match self.payload_for(index, &pending.device) {
            Ok(payload) => payload,
            Err(Some(problem)) => {
                self.message("Send to device", problem);
                return;
            }
            Err(None) => return,
        };
        if let Place::NextSync { folder } = &pending.device.place {
            self.offer_to_next(folder, &pending.name, payload);
            return;
        }
        self.start_send(doc_id, pending.device, pending.name, payload, false);
    }

    pub(super) fn start_send(&mut self, doc_id: u64, device: Device, name: String, payload: Payload, force: bool) {
        let stamp_key = Sends::stamp_key(&device, &name);
        let checked = matches!(device.place, Place::Card { .. } | Place::Folder { .. } | Place::Sftp { .. });
        let expected = checked.then(|| self.sends.stamps.iter().find(|(key, _)| *key == stamp_key).map(|(_, stamp)| Some(*stamp))).flatten();
        let sftp_secret = match &device.place {
            Place::Sftp { address, key_file } => crate::remote::parse_url(address).ok().map(|(mut site, _)| {
                site.key_file = (!key_file.trim().is_empty()).then(|| PathBuf::from(key_file.trim()));
                self.secret_for(&site)
            }),
            _ => None,
        };
        let scratch = std::env::temp_dir().join(format!("limoniter-send-{}", std::process::id())).join(format!("{}", self.sends.number));
        self.sends.number += 1;
        let password = self.sends.password(&device).unwrap_or_default();
        let sending = Sending { device, name, payload, expected, force: force || !checked, password, sftp_secret, known_hosts: self.known_hosts.clone(), scratch };
        self.sends.jobs.retain(|job| !(job.doc_id == doc_id && job.sending.device.name == sending.device.name));
        self.sends.jobs.push(Job::start(doc_id, sending));
    }

    /// The bytes to send: as the file is saved, or written in the device's own encoding. A file too large to hold
    /// is sent from disk as it's saved. Err(None) means a box has been opened about it.
    fn payload_for(&mut self, index: usize, device: &Device) -> Result<Payload, Option<String>> {
        let doc = &self.docs[index];
        if doc.browser.is_some() {
            return Err(Some("An image or archive is only read here. Take a file out of it with Save a copy, open that, and send it.".into()));
        }
        let huge = if doc.hex_mode { doc.hex.is_on_disk() } else { doc.text.is_disk_backed() };
        if huge {
            if device.preset.is_some() {
                return Err(Some(format!("{} writes text in another encoding on the way, and this file is too large to convert in memory. Send it to a device that takes it as it is.", device.name)));
            }
            return match &doc.path {
                Some(path) if !doc.is_modified() => Ok(Payload::File(path.clone())),
                _ => Err(Some("This file is too large to send from memory. Save it first, and it's sent from disk as it's saved.".into())),
            };
        }
        // A BASIC listing goes as the program it lists, tokenised as Save as program would.
        if let Some((machine, stem)) = doc.listing_of.clone().filter(|_| !doc.hex_mode) {
            return super::file_tools::tokenised(&doc.text.all_lines(), machine, &stem, None)
                .map(|(bytes, ..)| Payload::Bytes(Arc::new(bytes)))
                .map_err(|problem| Some(format!("This listing couldn't be tokenised, so nothing was sent.\n\n{problem}")));
        }
        let Some(preset) = device.preset.as_deref().and_then(codec::preset_by_id) else {
            return self.bytes_for(index).map(|bytes| Payload::Bytes(Arc::new(bytes))).ok_or(None);
        };
        if doc.hex_mode {
            return Err(Some(format!("{} writes text as {}, and hex view holds bytes rather than text. Switch to text to send it.", device.name, preset.name)));
        }
        let target = preset.format();
        let lines = doc.text.all_lines();
        match codec::export(&lines, &|_| preset.eol, &doc.text.format, &target, Lossy::Refuse) {
            Ok(bytes) => Ok(Payload::Bytes(Arc::new(bytes))),
            Err(problems) => {
                let first: Vec<String> = problems.listed.iter().take(5).map(|problem| format!("Line {}", problem.line + 1)).collect();
                Err(Some(format!("{} writes text as {}, which has no way to write {} character(s) here.\n\n{}\n\nChange them, or send to a device that takes the file as it is.", device.name, preset.name, problems.total, first.join(", "))))
            }
        }
    }

    /// Puts a file in line for a Spectrum Next to fetch with `.sync`, listening for it if that hasn't started.
    pub(super) fn offer_to_next(&mut self, folder: &str, name: &str, payload: Payload) {
        let data = match &payload {
            Payload::Bytes(bytes) => bytes.clone(),
            Payload::File(path) => match std::fs::read(path) {
                Ok(bytes) => Arc::new(bytes),
                Err(e) => {
                    warn(&format!("Couldn't read {}.\n\n{e}", path.display()));
                    return;
                }
            },
        };
        if data.len() > u32::MAX as usize / 2 {
            self.message("Send to device", "That's far too large for a Spectrum Next.".into());
            return;
        }
        if self.sends.nextsync.is_none() {
            match deploy::NextSync::start(self.sends.next_port) {
                Ok(server) => self.sends.nextsync = Some(server),
                Err(problem) => {
                    warn(&problem);
                    return;
                }
            }
        }
        let path = deploy::next_path(folder, name);
        self.sends.number += 1;
        if let Some(server) = &self.sends.nextsync {
            server.offer(deploy::Waiting { path: path.clone(), data, number: self.sends.number });
        }
        let address = deploy::local_address().unwrap_or_else(|| "this computer's address".into());
        if !self.sends.told_next && self.screenshot.is_none() && !cfg!(test) {
            self.sends.told_next = true;
            self.message("Waiting for the Next", format!("{path} is ready for the Next to fetch.\n\nOn the Next, type\n\n    .sync {address}\n\nthe first time, and after that just .sync. Limoniter keeps the file ready until it's fetched, and anything else sent meanwhile goes too.\n\nThe Next needs NextSync's .sync command, and its Wi-Fi set up."));
        }
    }

    /// Follows sends under way, and the Next fetching. True while anything is going on.
    pub(super) fn check_sends(&mut self) -> bool {
        let mut index = 0;
        while index < self.sends.jobs.len() {
            let Some(outcome) = self.sends.jobs[index].take() else {
                index += 1;
                continue;
            };
            let job = self.sends.jobs.remove(index);
            self.send_finished(job, outcome);
        }
        let mut waiting = false;
        if let Some(server) = &self.sends.nextsync {
            let mut shared = server.shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            waiting = !shared.waiting.is_empty() || shared.connected;
            let fetched = std::mem::take(&mut shared.fetched);
            let problem = shared.problem.take();
            drop(shared);
            if !fetched.is_empty() {
                self.sends.note = Some((format!("The Next fetched {}", fetched.join(", ")), String::new(), Instant::now()));
            }
            if let Some(problem) = problem {
                self.sends.note = Some(("The Next's fetch stopped part way; run .sync again".into(), problem, Instant::now()));
            }
        }
        let noting = self.sends.note.as_ref().is_some_and(|(_, _, when)| when.elapsed() < NOTE_FOR);
        !self.sends.jobs.is_empty() || waiting || noting
    }

    fn send_finished(&mut self, job: Job, outcome: Outcome) {
        let Sending { device, name, payload, .. } = job.sending.clone();
        match outcome {
            Outcome::Sent { stamp, note, place } => {
                if let Some(stamp) = stamp {
                    self.sends.set_stamp(Sends::stamp_key(&device, &name), stamp);
                    self.save_settings_if_changed();
                }
                let words = if note.is_empty() || note.contains('\n') { format!("Sent {name} to {}", device.name) } else { format!("Sent {name} to {}. {note}", device.name) };
                self.sends.note = Some((words, if note.is_empty() { place } else { format!("{place}\n\n{note}") }, Instant::now()));
            }
            Outcome::Failed(Failure::Changed) => {
                if ask_yes_no("Send to device", &format!("{name} on {} has been changed since it was last sent from here.\n\nReplace it with this version?", device.name)) {
                    self.start_send(job.doc_id, device, name, payload, true);
                }
            }
            Outcome::Failed(Failure::AlreadyThere) => {
                if ask_yes_no("Send to device", &format!("There's already a {name} on {}, not one sent from here.\n\nReplace it?", device.name)) {
                    self.start_send(job.doc_id, device, name, payload, true);
                }
            }
            Outcome::Failed(Failure::Refused(refused)) => {
                let Place::Sftp { address, key_file } = &device.place else {
                    return;
                };
                let Ok((mut site, _)) = crate::remote::parse_url(address) else {
                    return;
                };
                site.key_file = (!key_file.trim().is_empty()).then(|| PathBuf::from(key_file.trim()));
                let text = refused.describe(&site);
                match refused {
                    crate::remote::Refused::UnknownHost(print) => {
                        if ask_yes_no("A server not met before", &format!("{text}\n\nTrust it, and send?")) {
                            self.known_hosts.trust(&site, &print);
                            self.save_settings_if_changed();
                            self.start_send(job.doc_id, device, name, payload, false);
                        }
                    }
                    crate::remote::Refused::SignIn(_) => {
                        self.remote_secrets.remove(&site.key());
                        if let Some(n) = self.sends.devices.iter().position(|d| d.name == device.name)
                            && let Some(index) = self.docs.iter().position(|doc| doc.id == job.doc_id)
                        {
                            self.active = index;
                            self.open_send_box(n);
                            if let Dialog::Send(state) = &mut self.dialog {
                                state.name = name;
                                state.ask_password = true;
                                state.problem = text;
                            }
                        } else {
                            warn(&text);
                        }
                    }
                    _ => warn(&text),
                }
            }
            Outcome::Failed(Failure::Failed(_)) if job.stopped() => {
                self.sends.note = Some((format!("Stopped sending {name}"), String::new(), Instant::now()));
            }
            Outcome::Failed(Failure::Failed(problem)) => {
                // A wrong FTP password is forgotten, so the next send asks again.
                if let Some(key) = device.place.secret_key()
                    && problem.contains("sign-in")
                {
                    self.sends.passwords.remove(&key);
                }
                warn(&format!("{name} wasn't sent to {}.\n\n{problem}", device.name));
            }
        }
    }

    /// Words for the status bar about sending, and more to show on hover.
    pub(super) fn send_status(&self) -> Option<(String, String)> {
        if let Some(job) = self.sends.jobs.first() {
            return Some((format!("Sending {} to {}: {:.0}%", job.sending.name, job.sending.device.name, (job.share() * 100.0).min(100.0)), job.sending.device.place.describe()));
        }
        if let Some(server) = &self.sends.nextsync {
            let shared = server.shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if shared.connected {
                return Some((shared.status.clone(), String::new()));
            }
            if !shared.waiting.is_empty() {
                let names: Vec<&str> = shared.waiting.iter().map(|w| w.path.as_str()).collect();
                let address = deploy::local_address().unwrap_or_default();
                return Some((format!("{} file(s) waiting for .sync on the Next", names.len()), format!("{}\n\nOn the Next: .sync {address}", names.join("\n"))));
            }
        }
        self.sends.note.as_ref().filter(|(_, _, when)| when.elapsed() < NOTE_FOR).map(|(words, more, _)| (words.clone(), more.clone()))
    }

    pub(super) fn open_devices_box(&mut self) {
        self.dialog = Dialog::Devices(Box::new(DevicesBox { devices: self.sends.devices.clone(), selected: 0, password: String::new(), keep: true, found: None, told: String::new() }));
    }

    /// The box that sends.
    pub(super) fn send_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Send(state) = &mut self.dialog else {
            return false;
        };
        let Some(device) = self.sends.devices.get(state.device).cloned() else {
            self.close_dialog();
            return true;
        };
        let listing_kind = self.docs.iter().find(|doc| doc.id == state.doc_id).and_then(|doc| doc.listing_of.as_ref().filter(|_| !doc.hex_mode)).map(|(machine, _)| machine.kind());
        let card_missing = match &device.place {
            Place::Card { label, .. } => self.sends.card_present(label) == Some(false),
            _ => false,
        };
        let Dialog::Send(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut send = false;
        let name_problem = deploy::name_problem(&state.name);
        egui::Window::new(format!("Send to {}", device.name)).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(RichText::new(device.place.describe()).color(theme::text_faint()));
            if let Some(kind) = listing_kind {
                ui.label(RichText::new(format!("Sent as a {kind}, tokenised from this listing")).color(theme::text_faint()));
            } else if let Some(preset) = device.preset.as_deref().and_then(codec::preset_by_id) {
                ui.label(RichText::new(format!("Written as {}", preset.name)).color(theme::text_faint()));
            }
            if card_missing {
                ui.colored_label(theme::rainbow_mistake(), "That card isn't plugged in.");
            }
            ui.add_space(4.0);
            egui::Grid::new("send-box").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("File name:");
                let field = ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(300.0));
                if std::mem::take(&mut state.focus) {
                    field.request_focus();
                }
                send |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.end_row();
                let sftp_key = matches!(&device.place, Place::Sftp { key_file, .. } if !key_file.trim().is_empty());
                if state.ask_password {
                    ui.label(if sftp_key { "Passphrase:" } else { "Password:" });
                    ui.horizontal(|ui| {
                        let field = ui.add(egui::TextEdit::singleline(&mut state.password).password(true).desired_width(200.0));
                        send |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if !sftp_key {
                            ui.checkbox(&mut state.remember, "Remember it").on_hover_text(format!("Kept in {}, not in Limoniter's settings", crate::platform::CREDENTIAL_STORE));
                        }
                    });
                    ui.end_row();
                }
            });
            if let Some(problem) = &name_problem {
                ui.colored_label(theme::rainbow_mistake(), problem);
            }
            if !state.problem.is_empty() {
                ui.colored_label(theme::rainbow_mistake(), &state.problem);
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(name_problem.is_none() && !card_missing, egui::Button::new("Send")).clicked() {
                    send = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if send && name_problem.is_none() && !card_missing {
            let (doc_id, name, password, remember, ask) = (state.doc_id, state.name.trim().to_string(), std::mem::take(&mut state.password), state.remember, state.ask_password);
            if ask && !password.is_empty() {
                self.keep_send_password(&device, &password, remember);
            }
            self.close_dialog();
            if let Some(index) = self.docs.iter().position(|doc| doc.id == doc_id) {
                self.begin_send(index, device, name);
            }
            return true;
        }
        if close {
            self.close_dialog();
        }
        true
    }

    fn keep_send_password(&mut self, device: &Device, password: &str, remember: bool) {
        match &device.place {
            Place::Sftp { address, key_file } => {
                if let Ok((mut site, _)) = crate::remote::parse_url(address) {
                    site.key_file = (!key_file.trim().is_empty()).then(|| PathBuf::from(key_file.trim()));
                    let secret = if site.key_file.is_some() { crate::remote::Secret::KeyPassphrase(Some(password.to_string())) } else { crate::remote::Secret::Password(password.to_string()) };
                    if remember && site.key_file.is_none() {
                        let _ = crate::remote::store_password(&site, password);
                    }
                    self.remote_secrets.insert(site.key(), secret);
                }
            }
            place => {
                if let Some(key) = place.secret_key() {
                    if remember {
                        let _ = deploy::store_password(&key, password);
                    }
                    self.sends.passwords.insert(key, password.to_string());
                }
            }
        }
    }

    /// The box where devices are added, changed and removed.
    pub(super) fn devices_dialog(&mut self, ctx: &Context) -> bool {
        if !matches!(self.dialog, Dialog::Devices(_)) {
            return false;
        }
        let drives: Vec<deploy::Drive> = self.sends.drives().unwrap_or_default();
        let Dialog::Devices(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut done = false;
        let mut forget = None;
        let names_clash = state.devices.iter().enumerate().any(|(i, a)| state.devices.iter().skip(i + 1).any(|b| b.name.trim().eq_ignore_ascii_case(a.name.trim())));
        let first_problem = state.devices.iter().find_map(|device| device.problem().map(|p| format!("{}: {p}", if device.name.trim().is_empty() { "A device" } else { device.name.as_str() })));
        egui::Window::new("Devices").collapsible(false).resizable(false).default_width(640.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("Places File, Send to device sends a copy of a file to, and the emulators and machines Run on machine (F9) runs programs on. The file goes on saving where it always does.");
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(190.0);
                    egui::ScrollArea::vertical().id_salt("device-list").max_height(330.0).auto_shrink([false, true]).show(ui, |ui| {
                        for (n, device) in state.devices.iter().enumerate() {
                            let label = if device.name.trim().is_empty() { "(no name)".to_string() } else { device.name.clone() };
                            if ui.selectable_label(n == state.selected, label).on_hover_text(device.place.describe()).clicked() && n != state.selected {
                                state.selected = n;
                                state.password.clear();
                                state.told.clear();
                            }
                        }
                    });
                    if ui.button("Find emulators").on_hover_text("Adds the emulators installed on this computer that aren't listed yet").clicked() {
                        let mut added = 0;
                        for (emulator, program) in crate::emulators::find_installed() {
                            let listed = state.devices.iter().any(|d| matches!(&d.place, Place::Emulator { emulator: id, .. } if id == emulator.id));
                            if !listed {
                                let mut name = emulator.name.to_string();
                                while state.devices.iter().any(|d| d.name.eq_ignore_ascii_case(&name)) {
                                    name.push_str(" (2)");
                                }
                                state.devices.push(Device { name, place: Place::Emulator { emulator: emulator.id.into(), program, options: String::new() }, preset: None, eject: false });
                                added += 1;
                            }
                        }
                        state.found = Some(added);
                        if added > 0 {
                            state.selected = state.devices.len() - 1;
                        }
                    }
                    if let Some(added) = state.found {
                        ui.label(RichText::new(match added {
                            0 => "No more were found.".to_string(),
                            1 => "1 added.".to_string(),
                            n => format!("{n} added."),
                        }).color(theme::text_faint()));
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Add").clicked() {
                            let n = state.devices.len() + 1;
                            state.devices.push(Device { name: format!("Device {n}"), place: Place::blank(0), preset: None, eject: false });
                            state.selected = state.devices.len() - 1;
                            state.password.clear();
                        }
                        if ui.add_enabled(!state.devices.is_empty(), egui::Button::new("Remove")).clicked() {
                            state.devices.remove(state.selected);
                            state.selected = state.selected.min(state.devices.len().saturating_sub(1));
                            state.password.clear();
                        }
                    });
                });
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    let Some(device) = state.devices.get_mut(state.selected) else {
                        ui.label("Add a device to send to: a card, a folder, a server, a Spectrum Next or an Ultimate 64.");
                        return;
                    };
                    device_editor(ui, device, &drives, &mut state.password, &mut state.keep, &mut forget, &mut state.told);
                });
            });
            ui.separator();
            if names_clash {
                ui.colored_label(theme::rainbow_mistake(), "Two devices have the same name.");
            } else if let Some(problem) = &first_problem {
                ui.colored_label(theme::rainbow_mistake(), problem);
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(!names_clash && first_problem.is_none(), egui::Button::new("Done")).clicked() {
                    done = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if let Some(key) = forget {
            deploy::forget_password(&key);
            self.sends.passwords.remove(&key);
        }
        if done && let Dialog::Devices(state) = &mut self.dialog {
            let devices = std::mem::take(&mut state.devices);
            let typed = std::mem::take(&mut state.password);
            let keep = state.keep;
            if !typed.is_empty()
                && let Some(device) = devices.get(state.selected).cloned()
            {
                self.keep_send_password(&device, &typed, keep);
            }
            // Renamed devices keep what was last sent to them.
            for (_, device_name, _) in &mut self.sends.last {
                if !devices.iter().any(|d| d.name == *device_name)
                    && let Some(old) = self.sends.devices.iter().find(|d| d.name == *device_name)
                    && let Some(renamed) = devices.iter().find(|d| d.place == old.place && !self.sends.devices.iter().any(|o| o.name == d.name))
                {
                    device_name.clone_from(&renamed.name);
                }
            }
            self.sends.devices = devices;
            self.close_dialog();
            self.save_settings_if_changed();
            return true;
        }
        if close {
            self.close_dialog();
        }
        true
    }
}

/// The fields for one device.
#[allow(clippy::too_many_arguments, reason = "each is a different part of the box's state")]
fn device_editor(ui: &mut Ui, device: &mut Device, drives: &[deploy::Drive], password: &mut String, keep: &mut bool, forget: &mut Option<String>, told: &mut String) {
    egui::Grid::new("device-fields").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Name:");
        ui.add(egui::TextEdit::singleline(&mut device.name).desired_width(320.0));
        ui.end_row();
        ui.label("Kind:");
        let mut kind = device.place.kind();
        egui::ComboBox::from_id_salt("device-kind").width(320.0).selected_text(Place::KINDS[kind]).show_ui(ui, |ui| {
            for (n, title) in Place::KINDS.iter().enumerate() {
                ui.selectable_value(&mut kind, n, *title);
            }
        });
        if kind != device.place.kind() {
            device.place = Place::blank(kind);
            password.clear();
        }
        ui.end_row();
        let hint = |text: &str| RichText::new(text.to_string()).color(theme::text_faint());
        match &mut device.place {
            Place::Card { label, folder } => {
                ui.label("Volume label:");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(label).hint_text("GOTEK").desired_width(160.0));
                    egui::ComboBox::from_id_salt("device-drives").selected_text("Plugged in now").width(150.0).show_ui(ui, |ui| {
                        if drives.is_empty() {
                            ui.label("Nothing found");
                        }
                        for drive in drives {
                            let shown = if drive.label.is_empty() { format!("{} (no label)", drive.root.display()) } else { format!("{}  {}", drive.label, drive.root.display()) };
                            if ui.add_enabled(!drive.label.is_empty(), egui::Button::selectable(false, shown)).clicked() {
                                label.clone_from(&drive.label);
                            }
                        }
                    });
                });
                ui.end_row();
                ui.label("Folder on it:");
                ui.add(egui::TextEdit::singleline(folder).hint_text("Blank for the top, or GAMES/NEW").desired_width(320.0));
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut device.eject, "Eject the card once the file is on it");
                ui.end_row();
                ui.label("");
                note(ui, hint("Found by its label, whichever drive letter or mount point it has today."));
                ui.end_row();
            }
            Place::Folder { path } => {
                ui.label("Folder:");
                ui.horizontal(|ui| {
                    let mut text = path.display().to_string();
                    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(250.0)).changed() {
                        *path = PathBuf::from(text);
                    }
                    if ui.button("Browse\u{2026}").clicked()
                        && let Some(folder) = rfd::FileDialog::new().set_title("Send to this folder").pick_folder()
                    {
                        *path = folder;
                    }
                });
                ui.end_row();
                ui.label("");
                note(ui, hint("A shared network folder, an emulator's folder, or a TNFS server's folder for FujiNet."));
                ui.end_row();
            }
            Place::Sftp { address, key_file } => {
                ui.label("Address:");
                ui.add(egui::TextEdit::singleline(address).hint_text("sftp://root@mister/media/fat/games").desired_width(320.0));
                ui.end_row();
                ui.label("Key file:");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(key_file).hint_text("None: sign in with a password").desired_width(250.0));
                    if ui.button("Browse\u{2026}").clicked()
                        && let Some(file) = rfd::FileDialog::new().set_title("Private key").pick_file()
                    {
                        *key_file = file.display().to_string();
                    }
                });
                ui.end_row();
                ui.label("");
                note(ui, hint("A password is asked for when sending, the first time. The server's key is checked as for remote files."));
                ui.end_row();
            }
            Place::Ftp { host, port, user, folder } => {
                ui.label("Address:");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(host).hint_text("192.168.1.64").desired_width(200.0));
                    ui.label("Port:");
                    ui.add(egui::DragValue::new(port).range(1..=65535));
                });
                ui.end_row();
                ui.label("User:");
                ui.add(egui::TextEdit::singleline(user).hint_text("Blank for anonymous").desired_width(200.0));
                ui.end_row();
                ui.label("Folder:");
                ui.add(egui::TextEdit::singleline(folder).hint_text("/Usb0/games").desired_width(320.0));
                ui.end_row();
                password_row(ui, &device.place, password, keep, forget);
                ui.label("");
                note(ui, hint("Plain FTP sends the password unprotected, so use it only on your own network."));
                ui.end_row();
            }
            Place::NextSync { folder } => {
                ui.label("Folder on the Next:");
                ui.add(egui::TextEdit::singleline(folder).hint_text("/dot, or /games/mine").desired_width(320.0));
                ui.end_row();
                ui.label("");
                note(ui, hint(&format!(
                    "Sending makes the file ready, and the Next fetches it over Wi-Fi when .sync is run on it: .sync {} the first time, and then just .sync. The Next needs NextSync's .sync dot command. Port {} on this computer has to be reachable.",
                    deploy::local_address().unwrap_or_else(|| "this computer's address".into()),
                    deploy::NEXTSYNC_PORT
                )));
                ui.end_row();
            }
            Place::Ultimate { host } => {
                ui.label("Address:");
                ui.add(egui::TextEdit::singleline(host).hint_text("192.168.1.64").desired_width(200.0));
                ui.end_row();
                password_row(ui, &device.place, password, keep, forget);
                ui.label("");
                note(ui, hint("Runs .prg and .crt files, plays .sid and .mod tunes, and puts .d64, .g64, .d71, .g71 and .d81 disks in drive A, straight away. Needs firmware 3.11 or later, with the web remote control service on. A password is only needed if the Ultimate has a network password."));
                ui.end_row();
            }
            Place::NextCable { port } => {
                ui.label("Serial port:");
                ui.add(egui::TextEdit::singleline(port).hint_text(if cfg!(windows) { "COM8" } else { "/dev/ttyUSB0" }).desired_width(200.0));
                ui.end_row();
                ui.label("");
                note(ui, hint("Runs and debugs Z80 source on a real Next, as DeZog does: the Next needs dezogif as its NMI ROM (machines/next/enNextMf.rom, from DeZog's dezogif releases) and a USB serial cable that can do 921600 baud, on the Joy 2 port (GND 8, the cable's RX to 7, its TX to 9) or the UART pins. Press the Next's NMI button once before running. Or host:port for dezogif over Wi-Fi."));
                ui.end_row();
            }
            Place::Emulator { emulator, program, options } => {
                ui.label("Emulator:");
                let known = crate::emulators::by_id(emulator);
                egui::ComboBox::from_id_salt("device-emulator").width(320.0).selected_text(known.map_or("Choose one", |e| e.name)).show_ui(ui, |ui| {
                    for each in crate::emulators::KNOWN {
                        if ui.selectable_label(emulator == each.id, each.name).clicked() && emulator != each.id {
                            *emulator = each.id.to_string();
                            program.clear();
                        }
                    }
                });
                ui.end_row();
                ui.label("Program:");
                ui.horizontal(|ui| {
                    let mut text = program.display().to_string();
                    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(200.0)).changed() {
                        *program = PathBuf::from(text);
                    }
                    if ui.button("Choose\u{2026}").clicked()
                        && let Some(chosen) = rfd::FileDialog::new().set_title("The emulator's program").pick_file()
                    {
                        *program = chosen;
                    }
                    if let Some(known) = known
                        && ui.button("Find").on_hover_text("Looks where it's usually installed").clicked()
                    {
                        match crate::emulators::find_installed().into_iter().find(|(e, _)| e.id == known.id) {
                            Some((_, found)) => *program = found,
                            None => *told = format!("{} wasn't found. It's at {}", known.name, known.website),
                        }
                    }
                });
                ui.end_row();
                ui.label("Extra options:");
                ui.add(egui::TextEdit::singleline(options).hint_text("Added to its command line").desired_width(320.0));
                ui.end_row();
                if let Some(known) = known {
                    ui.label("");
                    let machines: Vec<&str> = known.machines.iter().map(|m| m.name()).collect();
                    let formats: Vec<&str> = known.formats.iter().map(|f| f.ending()).collect();
                    let reached = match known.link {
                        crate::emulators::Link::None => "",
                        _ => " While it runs, Limoniter can read and change its memory and screen.",
                    };
                    note(ui, hint(&format!("Runs programs for the {}, from .{} files.{reached}", machines.join(", "), formats.join(", ."))));
                    ui.end_row();
                    if !told.is_empty() {
                        ui.label("");
                        note(ui, RichText::new(told.clone()).color(theme::rainbow_mistake()));
                        ui.end_row();
                    }
                }
                return;
            }
            Place::Command { line } => {
                ui.label("Command line:");
                ui.add(egui::TextEdit::multiline(line).hint_text("x64sc -autostart {file}").desired_rows(2).desired_width(320.0));
                ui.end_row();
                ui.label("");
                note(ui, hint(&format!("Run by {} with a copy of the file. {{file}} is the copy's full path, {{name}} its name and {{folder}} its folder. A program still running after five seconds, such as an emulator, is left to run.", crate::platform::SHELL_NAME)));
                ui.end_row();
            }
        }
        ui.label("Write text as:");
        let chosen = device.preset.as_deref().and_then(codec::preset_by_id).map_or("The file as it's saved", |preset| preset.name);
        egui::ComboBox::from_id_salt("device-preset").width(320.0).selected_text(chosen).show_ui(ui, |ui| {
            ui.selectable_value(&mut device.preset, None, "The file as it's saved");
            for preset in &codec::PRESETS {
                ui.selectable_value(&mut device.preset, Some(preset.id.to_string()), preset.name);
            }
        });
        ui.end_row();
        ui.label("");
        note(ui, hint("Changes the character set and line endings of the copy sent, never the file itself."));
        ui.end_row();
    });
}

/// A line of help in the device's fields, wrapped to the width of the fields.
fn note(ui: &mut Ui, text: RichText) {
    ui.scope(|ui| {
        ui.set_max_width(330.0);
        ui.add(egui::Label::new(text).wrap());
    });
}

fn password_row(ui: &mut Ui, place: &Place, password: &mut String, keep: &mut bool, forget: &mut Option<String>) {
    let Some(key) = place.secret_key() else {
        return;
    };
    ui.label("Password:");
    ui.horizontal(|ui| {
        let kept = deploy::stored_password(&key).is_some();
        ui.add(egui::TextEdit::singleline(password).password(true).hint_text(if kept { "Kept; type to change it" } else { "If one is needed" }).desired_width(160.0));
        ui.checkbox(keep, "Remember it").on_hover_text(format!("Kept in {}, not in Limoniter's settings", crate::platform::CREDENTIAL_STORE));
        if kept && ui.small_button("Forget it").clicked() {
            *forget = Some(key);
        }
    });
    ui.end_row();
}

#[cfg(test)]
#[path = "deploy_host_tests.rs"]
mod tests;

