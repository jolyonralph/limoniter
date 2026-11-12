//! The window's side of remote files: the box that connects to a server and browses it, opening a file from it
//! or saving one to it, and sending a remote file back each time it's saved.

use super::*;
use crate::remote::{self, Entry, Job, KnownHosts, Outcome, Refused, Secret, Site, Stamp, Task};

/// Where a document that came from a server goes back to.
#[derive(Clone, Debug)]
pub(super) struct RemoteLink {
    pub site: Site,
    pub path: String,
    /// The copy on this computer it's edited in.
    pub local: PathBuf,
    /// The file on the server when it was fetched or last sent, to notice someone else changing it.
    pub stamp: Option<Stamp>,
}

/// A site kept by name in the settings. Its password, if kept, is in the system's credential store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SavedSite {
    pub name: String,
    pub site: Site,
}

pub(super) fn sites_to_settings(sites: &[SavedSite]) -> String {
    sites
        .iter()
        .map(|s| format!("sftp_site={}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\n", s.name, s.site.host, s.site.port, s.site.user, s.site.key_file.as_ref().map_or_else(String::new, |k| k.display().to_string())))
        .collect()
}

pub(super) fn sites_from_settings(values: &[String]) -> Vec<SavedSite> {
    values
        .iter()
        .filter_map(|value| {
            let parts: Vec<&str> = value.split('\u{1}').collect();
            let [name, host, port, user, key] = parts.as_slice() else {
                return None;
            };
            let key_file = (!key.is_empty()).then(|| PathBuf::from(key));
            Some(SavedSite { name: (*name).to_string(), site: Site { host: (*host).to_string(), port: port.parse().ok()?, user: (*user).to_string(), key_file } })
        })
        .collect()
}

/// What a background job is for.
#[derive(Clone, Debug)]
pub(super) enum Purpose {
    /// Listing for the box.
    Browse,
    /// Fetching a file to open.
    Open,
    /// Sending a document back after it was saved.
    Send(u64),
}

/// The Open remote file box, and Save to a server.
#[derive(Default)]
pub(super) struct RemoteBox {
    pub address: String,
    pub key_file: String,
    pub password: String,
    pub remember: bool,
    /// The folder listed, and what's in it.
    pub folder: Option<String>,
    pub entries: Vec<Entry>,
    pub status: String,
    /// A server key waiting to be trusted, or one that has changed.
    pub trust: Option<String>,
    pub changed_key: bool,
    /// Saving this document to the server, rather than opening a file from it.
    pub saving: Option<u64>,
    pub file_name: String,
}

impl RemoteBox {
    pub(super) fn site(&self) -> Result<(Site, String), String> {
        let (mut site, path) = remote::parse_url(&self.address)?;
        site.key_file = (!self.key_file.trim().is_empty()).then(|| PathBuf::from(self.key_file.trim()));
        Ok((site, path))
    }
}

impl Limoniter {
    fn remote_copies(&self) -> PathBuf {
        self.remote_root.clone().unwrap_or_else(|| std::env::temp_dir().join("limoniter-remote-copies"))
    }

    /// The way to sign in to a site: what was typed this session, a kept password, or its key file.
    pub(super) fn secret_for(&self, site: &Site) -> Secret {
        if let Some(secret) = self.remote_secrets.get(&site.key()) {
            return secret.clone();
        }
        if site.key_file.is_some() {
            return Secret::KeyPassphrase(None);
        }
        remote::stored_password(site).map_or(Secret::KeyPassphrase(None), Secret::Password)
    }

    fn start_remote(&mut self, purpose: Purpose, site: Site, task: Task) {
        let job = Job::start(site.clone(), self.secret_for(&site), self.known_hosts.clone(), task);
        self.remote_jobs.push((purpose, job));
    }

    pub(super) fn open_remote_box(&mut self, saving: Option<u64>) {
        let mut state = RemoteBox { remember: true, ..RemoteBox::default() };
        let from = saving.and_then(|id| self.docs.iter().find(|d| d.id == id)).and_then(|doc| doc.remote.clone());
        if let Some(link) = from {
            state.address = link.site.url(&remote::parent(&link.path));
            state.key_file = link.site.key_file.as_ref().map_or_else(String::new, |k| k.display().to_string());
        } else if let Some(saved) = self.remote_sites.first() {
            state.address = saved.site.url("");
            state.key_file = saved.site.key_file.as_ref().map_or_else(String::new, |k| k.display().to_string());
        } else {
            state.address = "sftp://".to_string();
        }
        if let Some(id) = saving {
            state.file_name = self.docs.iter().find(|d| d.id == id).map_or_else(String::new, Document::name);
        }
        state.saving = saving;
        self.dialog = Dialog::Remote(Box::new(state));
    }

    /// Connects (again) and lists a folder: the one given, or the one in the address, or the home folder.
    pub(super) fn remote_connect(&mut self, folder: Option<String>) {
        let Dialog::Remote(state) = &mut self.dialog else {
            return;
        };
        let (site, path) = match state.site() {
            Ok(found) => found,
            Err(problem) => {
                state.status = problem;
                return;
            }
        };
        if !state.password.is_empty() {
            let secret = if site.key_file.is_some() { Secret::KeyPassphrase(Some(state.password.clone())) } else { Secret::Password(state.password.clone()) };
            self.remote_secrets.insert(site.key(), secret);
        }
        state.status = format!("Connecting to {}\u{2026}", site.host);
        state.trust = None;
        state.changed_key = false;
        let folder = folder.or_else(|| (path != "/").then_some(path));
        self.remote_jobs.retain(|(purpose, _)| !matches!(purpose, Purpose::Browse));
        self.start_remote(Purpose::Browse, site, Task::List(folder));
    }

    /// Fetches a remote file into its copy on this computer, to open when it arrives.
    pub(super) fn remote_fetch(&mut self, site: Site, path: String) {
        let local = remote::local_copy(&self.remote_copies(), &site, &path);
        self.start_remote(Purpose::Open, site, Task::Download { remote: path, local });
    }

    /// Saves a document to the server: into its copy here first, then sent.
    pub(super) fn remote_save(&mut self, index: usize, site: Site, path: String) {
        let local = remote::local_copy(&self.remote_copies(), &site, &path);
        if let Some(folder) = local.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        self.docs[index].remote = Some(RemoteLink { site, path, local: local.clone(), stamp: None });
        // Saving there marks it to be sent; nothing on the server is touched if the save here fails.
        if !self.write_to(index, &local) {
            self.docs[index].remote = None;
        }
    }

    /// Sends back every remote document that has just been saved.
    pub(super) fn send_saved_remotes(&mut self) {
        let mut wanted: Vec<(u64, RemoteLink)> = Vec::new();
        for doc in &mut self.docs {
            if std::mem::take(&mut doc.upload_wanted)
                && let Some(link) = doc.remote.clone()
            {
                wanted.push((doc.id, link));
            }
        }
        for (id, link) in wanted {
            self.remote_jobs.retain(|(purpose, _)| !matches!(purpose, Purpose::Send(other) if *other == id));
            let task = Task::Upload { local: link.local.clone(), remote: link.path.clone(), expected: link.stamp, force: false };
            self.start_remote(Purpose::Send(id), link.site, task);
        }
    }

    /// Looks at the background jobs, and acts on any that have finished. True while any are running.
    pub(super) fn check_remote_jobs(&mut self) -> bool {
        let mut index = 0;
        while index < self.remote_jobs.len() {
            let Some(outcome) = self.remote_jobs[index].1.take() else {
                index += 1;
                continue;
            };
            let (purpose, job) = self.remote_jobs.remove(index);
            self.remote_finished(purpose, &job, outcome);
        }
        !self.remote_jobs.is_empty()
    }

    fn remote_finished(&mut self, purpose: Purpose, job: &Job, outcome: Outcome) {
        let site = job.site.clone();
        match (purpose, outcome) {
            (_, Outcome::Refused(refused)) => {
                let text = refused.describe(&site);
                if let Dialog::Remote(state) = &mut self.dialog {
                    state.status = text;
                    match refused {
                        Refused::UnknownHost(print) => state.trust = Some(print),
                        Refused::HostKeyChanged { .. } => state.changed_key = true,
                        Refused::SignIn(_) => {
                            self.remote_secrets.remove(&site.key());
                        }
                        Refused::Failed(_) => {}
                    }
                } else {
                    // Sending after a save, with no box open: the file is saved here, and says so.
                    self.remote_secrets.remove(&site.key());
                    warn(&format!("{text}\n\nYour changes are saved on this computer. File, Save to a server can send them once it can connect."));
                }
            }
            (Purpose::Browse, Outcome::Listed { folder, entries }) => {
                if let Dialog::Remote(state) = &mut self.dialog {
                    if state.remember && !state.password.is_empty() && site.key_file.is_none() {
                        let _ = remote::store_password(&site, &state.password);
                    }
                    state.password.clear();
                    state.address = site.url(&folder);
                    state.status = format!("{} item(s)", entries.len());
                    state.folder = Some(folder);
                    state.entries = entries;
                }
                if !self.remote_sites.iter().any(|saved| saved.site == site) {
                    self.remote_sites.insert(0, SavedSite { name: format!("{}@{}", site.user, site.host), site });
                    self.remote_sites.truncate(20);
                }
            }
            (Purpose::Open, Outcome::Downloaded { remote: path, local, stamp }) => {
                // The box has done its work once the file is here.
                if matches!(self.dialog, Dialog::Remote(_)) {
                    self.close_dialog();
                }
                if self.open_path(&local)
                    && let Some(doc) = self.docs.iter_mut().find(|doc| doc.path.as_deref().is_some_and(|p| same_path(p, &local)))
                {
                    doc.remote = Some(RemoteLink { site: site.clone(), path: path.clone(), local, stamp: Some(stamp) });
                    if !self.remote_recent.contains(&site.url(&path)) {
                        self.remote_recent.insert(0, site.url(&path));
                        self.remote_recent.truncate(15);
                    }
                }
            }
            (Purpose::Send(id), Outcome::Uploaded { stamp, .. }) => {
                if let Some(link) = self.docs.iter_mut().find(|doc| doc.id == id).and_then(|doc| doc.remote.as_mut()) {
                    link.stamp = Some(stamp);
                }
            }
            (Purpose::Send(id), Outcome::ChangedOnServer) => {
                let Some(link) = self.docs.iter().find(|doc| doc.id == id).and_then(|doc| doc.remote.clone()) else {
                    return;
                };
                let question = format!("{} has been changed on the server since you opened it.\n\nReplace it with your version?", site.url(&link.path));
                if ask_yes_no("Changed on the server", &question) {
                    let task = Task::Upload { local: link.local.clone(), remote: link.path.clone(), expected: None, force: true };
                    self.start_remote(Purpose::Send(id), link.site, task);
                } else {
                    self.message("Save", format!("Your version is saved on this computer, at {}.", link.local.display()));
                }
            }
            (purpose, Outcome::Failed(problem)) => match (&mut self.dialog, purpose) {
                (Dialog::Remote(state), Purpose::Browse | Purpose::Open) => state.status = problem,
                (_, Purpose::Send(id)) => {
                    let local = self.docs.iter().find(|doc| doc.id == id).and_then(|doc| doc.remote.as_ref()).map(|link| link.local.display().to_string()).unwrap_or_default();
                    warn(&format!("Couldn't send the file to {}.\n\n{problem}\n\nYour changes are saved on this computer, at {local}.", site.host));
                }
                _ => warn(&problem),
            },
            _ => {}
        }
    }

    /// The box for opening from, or saving to, a server.
    pub(super) fn remote_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Remote(state) = &mut self.dialog else {
            return false;
        };
        let busy = self.remote_jobs.iter().any(|(purpose, _)| matches!(purpose, Purpose::Browse | Purpose::Open));
        let fetching = self.remote_jobs.iter().find(|(purpose, _)| matches!(purpose, Purpose::Open)).map(|(_, job)| job.share());
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut connect = false;
        let mut go_to = None;
        let mut open = None;
        let mut save_here = false;
        let mut trust = false;
        let mut forget_key = false;
        let mut forget_password = false;
        let title = if state.saving.is_some() { "Save to a server" } else { "Open a remote file" };
        let sites = self.remote_sites.clone();
        let recent = self.remote_recent.clone();
        egui::Window::new(title).collapsible(false).resizable(true).default_size([640.0, 520.0]).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            egui::Grid::new("remote-sign-in").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Address:");
                ui.horizontal(|ui| {
                    let field = ui.add(egui::TextEdit::singleline(&mut state.address).hint_text("sftp://user@host:port/folder").desired_width(380.0));
                    connect |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if !sites.is_empty() || !recent.is_empty() {
                        egui::ComboBox::from_id_salt("remote-sites").selected_text("Recent").width(90.0).show_ui(ui, |ui| {
                            for saved in &sites {
                                if ui.selectable_label(false, saved.site.url("")).clicked() {
                                    state.address = saved.site.url("");
                                    state.key_file = saved.site.key_file.as_ref().map_or_else(String::new, |k| k.display().to_string());
                                }
                            }
                            for url in &recent {
                                if ui.selectable_label(false, url).clicked() {
                                    state.address.clone_from(url);
                                }
                            }
                        });
                    }
                });
                ui.end_row();
                ui.label("Key file:");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut state.key_file).hint_text("None: sign in with a password").desired_width(380.0));
                    if ui.button("Browse\u{2026}").clicked()
                        && let Some(file) = rfd::FileDialog::new().set_title("Private key").pick_file()
                    {
                        state.key_file = file.display().to_string();
                    }
                });
                ui.end_row();
                ui.label(if state.key_file.trim().is_empty() { "Password:" } else { "Passphrase:" });
                ui.horizontal(|ui| {
                    let field = ui.add(egui::TextEdit::singleline(&mut state.password).password(true).hint_text("Asked for only if needed").desired_width(240.0));
                    connect |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if state.key_file.trim().is_empty() {
                        ui.checkbox(&mut state.remember, "Remember it").on_hover_text(format!("Kept in {}, not in Limoniter's settings", crate::platform::CREDENTIAL_STORE));
                    }
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("Connect")).clicked() {
                    connect = true;
                }
                if ui.small_button("Forget the kept password").clicked() {
                    forget_password = true;
                }
                if busy {
                    ui.spinner();
                }
                if let Some(share) = fetching {
                    ui.label(format!("Fetching: {:.0}%", share * 100.0));
                }
            });
            if !state.status.is_empty() {
                ui.label(&state.status);
            }
            if let Some(print) = &state.trust {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(print).monospace());
                    if ui.button("Trust it and connect").clicked() {
                        trust = true;
                    }
                });
            }
            if state.changed_key && ui.button("Forget the old key").on_hover_text("Only once you're sure the new key is right").clicked() {
                forget_key = true;
            }
            ui.separator();
            if let Some(folder) = state.folder.clone() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&folder).strong());
                    if folder != "/" && ui.small_button("Up").clicked() {
                        go_to = Some(remote::parent(&folder));
                    }
                });
                egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                    for entry in &state.entries {
                        let label = if entry.folder { format!("\u{1F4C1} {}", entry.name) } else { format!("    {}", entry.name) };
                        let size = if entry.folder { String::new() } else { short_size(entry.size) };
                        let response = ui.horizontal(|ui| {
                            let response = ui.selectable_label(!entry.folder && entry.name == state.file_name, label);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(size).color(theme::text_faint())));
                            response
                        });
                        let response = response.inner;
                        if entry.folder && response.clicked() {
                            go_to = Some(remote::join(&folder, &entry.name));
                        } else if response.clicked() {
                            state.file_name.clone_from(&entry.name);
                        }
                        if !entry.folder && response.double_clicked() && state.saving.is_none() {
                            open = Some(remote::join(&folder, &entry.name));
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("File name:");
                    ui.add(egui::TextEdit::singleline(&mut state.file_name).desired_width(300.0));
                    let name_ok = !state.file_name.trim().is_empty() && !state.file_name.contains('/');
                    if state.saving.is_some() {
                        if ui.add_enabled(name_ok && !busy, egui::Button::new("Save here")).clicked() {
                            save_here = true;
                        }
                    } else if ui.add_enabled(name_ok && !busy, egui::Button::new("Open")).clicked() {
                        open = Some(remote::join(&folder, state.file_name.trim()));
                    }
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });
        let site = state.site().ok().map(|(site, _)| site);
        let folder = state.folder.clone();
        let saving = state.saving;
        let file_name = state.file_name.trim().to_string();
        let exists = state.entries.iter().any(|entry| entry.name == file_name);
        if forget_password && let Some(site) = &site {
            remote::forget_password(site);
            self.remote_secrets.remove(&site.key());
        }
        if trust && let (Some(site), Some(Dialog::Remote(state))) = (&site, Some(&mut self.dialog)) && let Some(print) = state.trust.take() {
            self.known_hosts.trust(site, &print);
            self.save_settings_if_changed();
            connect = true;
        }
        if forget_key && let Some(site) = &site {
            self.known_hosts.forget(site);
            self.save_settings_if_changed();
            connect = true;
        }
        if connect || go_to.is_some() {
            self.remote_connect(go_to);
        }
        if let (Some(path), Some(site)) = (open, site.clone()) {
            self.remote_fetch(site, path);
            close = false;
        }
        if save_here && let (Some(site), Some(folder), Some(id)) = (site, folder, saving) {
            let path = remote::join(&folder, &file_name);
            if !exists || ask_yes_no("Save to a server", &format!("{file_name} is already there. Replace it?")) {
                if let Some(index) = self.docs.iter().position(|doc| doc.id == id) {
                    self.close_dialog();
                    self.remote_save(index, site, path);
                }
                return true;
            }
        }
        if close {
            self.remote_jobs.retain(|(purpose, job)| {
                let keep = !matches!(purpose, Purpose::Browse);
                if !keep {
                    job.stop();
                }
                keep
            });
            self.close_dialog();
        }
        true
    }

    /// Words for the status bar while files are being fetched or sent.
    pub(super) fn remote_status(&self) -> Option<String> {
        let sending = self.remote_jobs.iter().find(|(purpose, _)| matches!(purpose, Purpose::Send(_)))?;
        Some(format!("Sending to {}: {:.0}%", sending.1.site.host, (sending.1.share() * 100.0).min(100.0)))
    }

    /// Settings kept for remote files: sites, trusted server keys and recent files. Never passwords.
    pub(super) fn remote_settings(&self) -> String {
        let mut out = sites_to_settings(&self.remote_sites);
        out += &self.known_hosts.to_settings();
        out += &self.remote_recent.iter().map(|url| format!("sftp_recent={url}\n")).collect::<String>();
        out
    }
}

/// Reads the remote settings back.
pub(super) fn read_remote_settings(settings: &str) -> (Vec<SavedSite>, KnownHosts, Vec<String>) {
    (sites_from_settings(&list_setting(settings, "sftp_site")), KnownHosts::read_settings(&list_setting(settings, "sftp_host_key")), list_setting(settings, "sftp_recent"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_are_kept_without_passwords() {
        let sites = vec![SavedSite { name: "work".into(), site: Site { host: "h".into(), port: 2222, user: "u".into(), key_file: Some(PathBuf::from("C:/k")) } }, SavedSite { name: "home".into(), site: Site { host: "g".into(), port: 22, user: "v".into(), key_file: None } }];
        let text = sites_to_settings(&sites);
        assert!(!text.to_lowercase().contains("password"));
        let back = sites_from_settings(&text.lines().filter_map(|l| l.strip_prefix("sftp_site=")).map(str::to_string).collect::<Vec<_>>());
        assert_eq!(back, sites);
    }
}
