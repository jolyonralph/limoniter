//! The window's side of remote files: the box that connects to a server and browses it, opening a file from it
//! or saving one to it, and sending a remote file back each time it's saved.

use super::*;
use crate::remote::{self, Change, Entry, Job, KnownHosts, Outcome, Refused, Secret, Site, Stamp, Task};

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
        .map(|s| format!("sftp_site={}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\n", s.name, s.site.host, s.site.port, s.site.user, s.site.key_file.as_ref().map_or_else(String::new, |k| k.display().to_string()), s.site.scheme.prefix()))
        .collect()
}

pub(super) fn sites_from_settings(values: &[String]) -> Vec<SavedSite> {
    values
        .iter()
        .filter_map(|value| {
            let parts: Vec<&str> = value.split('\u{1}').collect();
            // Kept before FTP could be used, a site had no scheme: it's SFTP.
            let (name, host, port, user, key, scheme) = match parts.as_slice() {
                [name, host, port, user, key] => (name, host, port, user, key, remote::Scheme::Sftp),
                [name, host, port, user, key, scheme] => (name, host, port, user, key, remote::Scheme::from_prefix(scheme).unwrap_or_default()),
                _ => return None,
            };
            let key_file = (!key.is_empty()).then(|| PathBuf::from(key));
            Some(SavedSite { name: (*name).to_string(), site: Site { scheme, host: (*host).to_string(), port: port.parse().ok()?, user: (*user).to_string(), key_file } })
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
    /// Looking at a document's file on the server, to notice it changing there.
    Watch(u64),
    /// Fetching it again, into its copy here, once it has.
    Refresh(u64),
}

/// How often an open remote file is looked at on its server.
const WATCH_REMOTE_EVERY: Duration = Duration::from_secs(15);

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
    /// The hosts in ~/.ssh/config, offered beside the saved sites.
    pub ssh_hosts: Vec<(String, Site)>,
    /// Files chosen with Ctrl to open together.
    pub chosen: std::collections::BTreeSet<String>,
    /// A file or folder being renamed (its name, and the new one), or a new folder being named.
    pub renaming: Option<(String, String)>,
    pub new_folder: Option<String>,
    /// Choosing a folder to open as a project, rather than a file.
    pub project: bool,
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
        let mut state = RemoteBox { remember: true, ssh_hosts: remote::own_ssh_hosts(), ..RemoteBox::default() };
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
        state.status = crate::i18n::tf!("Connecting to {}\u{2026}", site.host);
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
        let mut in_projects: Vec<PathBuf> = Vec::new();
        for doc in &mut self.docs {
            if std::mem::take(&mut doc.upload_wanted) {
                match (doc.remote.clone(), &doc.path) {
                    (Some(link), _) => wanted.push((doc.id, link)),
                    (None, Some(path)) => in_projects.push(path.clone()),
                    (None, None) => {}
                }
            }
        }
        for path in in_projects {
            self.remote_project_saved(&path);
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
            // Looking at a file on its server in the background: said nothing of, it's tried again later.
            (Purpose::Watch(_) | Purpose::Refresh(_), Outcome::Refused(_) | Outcome::Failed(_)) => {}
            // An sftp:// address opened with nothing to sign in with yet: the box, to sign in.
            (Purpose::Open, Outcome::Refused(refused)) if !matches!(self.dialog, Dialog::Remote(_)) => {
                let text = refused.describe(&site);
                self.open_remote_box(None);
                if let Dialog::Remote(state) = &mut self.dialog {
                    state.address = site.url(&remote::parent(&self.remote_opening.take().unwrap_or_default()));
                    state.key_file = site.key_file.as_ref().map_or_else(String::new, |k| k.display().to_string());
                    state.status = text;
                    if let Refused::UnknownHost(print) = refused {
                        state.trust = Some(print);
                    }
                }
            }
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
                    warn(&crate::i18n::tf!("{text}\n\nYour changes are saved on this computer. File, Save to a server can send them once it can connect."));
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
                    doc.remote = Some(RemoteLink { site: site.clone(), path: path.clone(), local: local.clone(), stamp: Some(stamp) });
                    if !self.remote_recent.contains(&site.url(&path)) {
                        self.remote_recent.insert(0, site.url(&path));
                        self.remote_recent.truncate(15);
                    }
                    // In the ordinary recent files as its address, rather than its copy here, to open again from there.
                    self.panels.recent.retain(|p| !same_path(p, &local));
                    self.note_recent_file(&PathBuf::from(site.url(&path)));
                }
            }
            (Purpose::Watch(id), Outcome::Looked { stamp: Some(now) }) => {
                // Changed on the server: fetched into the copy here, where the change is noticed as any file's is.
                if let Some(link) = self.docs.iter().find(|doc| doc.id == id).and_then(|doc| doc.remote.clone())
                    && link.stamp.is_some_and(|was| was != now)
                {
                    let task = Task::Download { remote: link.path.clone(), local: link.local.clone() };
                    self.start_remote(Purpose::Refresh(id), link.site, task);
                }
            }
            (Purpose::Refresh(id), Outcome::Downloaded { stamp, .. }) => {
                if let Some(link) = self.docs.iter_mut().find(|doc| doc.id == id).and_then(|doc| doc.remote.as_mut()) {
                    link.stamp = Some(stamp);
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
                let question = crate::i18n::tf!("{} has been changed on the server since you opened it.\n\nReplace it with your version?", site.url(&link.path));
                if ask_yes_no("Changed on the server", &question) {
                    let task = Task::Upload { local: link.local.clone(), remote: link.path.clone(), expected: None, force: true };
                    self.start_remote(Purpose::Send(id), link.site, task);
                } else {
                    self.message("Save", crate::i18n::tf!("Your version is saved on this computer, at {}.", link.local.display()));
                }
            }
            (purpose, Outcome::Failed(problem)) => match (&mut self.dialog, purpose) {
                (Dialog::Remote(state), Purpose::Browse | Purpose::Open) => state.status = problem,
                (_, Purpose::Send(id)) => {
                    let local = self.docs.iter().find(|doc| doc.id == id).and_then(|doc| doc.remote.as_ref()).map(|link| link.local.display().to_string()).unwrap_or_default();
                    warn(&crate::i18n::tf!("Couldn't send the file to {}.\n\n{problem}\n\nYour changes are saved on this computer, at {local}.", site.host));
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
        let (mut change, mut change_cancelled, mut deleting, mut open_many): (Option<Change>, bool, Option<(String, bool)>, Vec<String>) = (None, false, None, Vec::new());
        let title = if state.saving.is_some() {
            "Save to a server"
        } else if state.project {
            "Open a remote folder as a project"
        } else {
            "Open a remote file"
        };
        // FTP signs in with a password only.
        let ftp = state.address.trim_start().get(..3).is_some_and(|start| start.eq_ignore_ascii_case("ftp"));
        let mut open_project = None;
        let sites = self.remote_sites.clone();
        let recent = self.remote_recent.clone();
        crate::fit::Dialog::new(title).collapsible(false).resizable(true).default_size([640.0, 520.0]).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
            crate::fit::Part::Body => {
                egui::Grid::new("remote-sign-in").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label(crate::i18n::tr("Address:"));
                    ui.horizontal(|ui| {
                        let field = ui.add(egui::TextEdit::singleline(&mut state.address).hint_text(crate::i18n::tr("sftp://user@host:port/folder, or ftp://, ftpes://, ftps://")).desired_width(380.0));
                        connect |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if !sites.is_empty() || !recent.is_empty() || !state.ssh_hosts.is_empty() {
                            egui::ComboBox::from_id_salt("remote-sites").selected_text(crate::i18n::tr("Recent")).width(90.0).show_ui(ui, |ui| {
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
                                if !state.ssh_hosts.is_empty() {
                                    ui.separator();
                                    ui.label(RichText::new(crate::i18n::tr("From ~/.ssh/config")).small().color(theme::text_dim()));
                                    for (name, site) in state.ssh_hosts.clone() {
                                        if ui.selectable_label(false, &name).on_hover_text(site.url("")).clicked() {
                                            state.address = site.url("");
                                            state.key_file = site.key_file.as_ref().map_or_else(String::new, |k| k.display().to_string());
                                        }
                                    }
                                }
                            });
                        }
                    });
                    ui.end_row();
                    if ftp {
                        state.key_file.clear();
                    } else {
                        ui.label(crate::i18n::tr("Key file:"));
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut state.key_file).hint_text(crate::i18n::tr("None: sign in with a password")).desired_width(380.0));
                            if ui.button(crate::i18n::tr("Browse\u{2026}")).clicked()
                                && let Some(file) = rfd::FileDialog::new().set_title(crate::i18n::tr("Private key")).pick_file()
                            {
                                state.key_file = file.display().to_string();
                            }
                        });
                        ui.end_row();
                    }
                    ui.label(if state.key_file.trim().is_empty() { crate::i18n::tr("Password:") } else { crate::i18n::tr("Passphrase:") });
                    ui.horizontal(|ui| {
                        let field = ui.add(egui::TextEdit::singleline(&mut state.password).password(true).hint_text(crate::i18n::tr("Asked for only if needed")).desired_width(240.0));
                        connect |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if state.key_file.trim().is_empty() {
                            ui.checkbox(&mut state.remember, crate::i18n::tr("Remember it")).on_hover_text(crate::i18n::tf!("Kept in {}, not in Limoniter's settings", crate::platform::CREDENTIAL_STORE));
                        }
                    });
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new(crate::i18n::tr("Connect"))).clicked() {
                        connect = true;
                    }
                    if ui.small_button(crate::i18n::tr("Forget the kept password")).clicked() {
                        forget_password = true;
                    }
                    if busy {
                        ui.spinner();
                    }
                    if let Some(share) = fetching {
                        ui.label(crate::i18n::tf!("Fetching: {:.0}%", share * 100.0));
                    }
                });
                if !state.status.is_empty() {
                    ui.label(&state.status);
                }
                if let Some(print) = &state.trust {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(print).monospace());
                        if ui.button(crate::i18n::tr("Trust it and connect")).clicked() {
                            trust = true;
                        }
                    });
                }
                if state.changed_key && ui.button(crate::i18n::tr("Forget the old key")).on_hover_text(crate::i18n::tr("Only once you're sure the new key is right")).clicked() {
                    forget_key = true;
                }
                ui.separator();
                if let Some(folder) = state.folder.clone() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&folder).strong());
                        if folder != "/" && ui.small_button(crate::i18n::tr("Up")).clicked() {
                            go_to = Some(remote::parent(&folder));
                        }
                        if ui.small_button(crate::i18n::tr("New folder")).clicked() {
                            state.new_folder = Some(String::new());
                        }
                    });
                    // A new folder or a new name being typed.
                    if let Some(name) = &mut state.new_folder {
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr("New folder:"));
                            let field = ui.add(egui::TextEdit::singleline(name).desired_width(240.0));
                            field.request_focus();
                            let ok = !name.trim().is_empty() && !name.contains('/');
                            if ui.add_enabled(ok, egui::Button::new(crate::i18n::tr("Make it"))).clicked() || (ok && field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                                change = Some(Change::MakeFolder(remote::join(&folder, name.trim())));
                            }
                            if ui.button(crate::i18n::tr("Cancel")).clicked() {
                                change_cancelled = true;
                            }
                        });
                    }
                    if let Some((old, name)) = &mut state.renaming {
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tf!("Rename {old} to:"));
                            let field = ui.add(egui::TextEdit::singleline(name).desired_width(240.0));
                            let ok = !name.trim().is_empty() && !name.contains('/') && name.trim() != old.as_str();
                            if ui.add_enabled(ok, egui::Button::new(crate::i18n::tr("Rename"))).clicked() || (ok && field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                                change = Some(Change::Rename { from: remote::join(&folder, old), to: remote::join(&folder, name.trim()) });
                            }
                            if ui.button(crate::i18n::tr("Cancel")).clicked() {
                                change_cancelled = true;
                            }
                        });
                    }
                    egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                        for entry in &state.entries {
                            let label = if entry.folder { format!("\u{1F4C1} {}", entry.name) } else { format!("    {}", entry.name) };
                            let size = if entry.folder { String::new() } else { short_size(entry.size) };
                            let response = ui.horizontal(|ui| {
                                let picked = !entry.folder && (entry.name == state.file_name || state.chosen.contains(&entry.name));
                                let response = ui.selectable_label(picked, crate::i18n::tr(&label));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(size).color(theme::text_faint())));
                                response
                            });
                            let response = response.inner;
                            let with_ctrl = ui.input(|i| i.modifiers.command);
                            if entry.folder && response.clicked() {
                                go_to = Some(remote::join(&folder, &entry.name));
                            } else if response.clicked() && with_ctrl && state.saving.is_none() {
                                // Ctrl chooses several to open together.
                                if !state.chosen.remove(&entry.name) {
                                    state.chosen.insert(entry.name.clone());
                                }
                            } else if response.clicked() {
                                state.chosen.clear();
                                state.file_name.clone_from(&entry.name);
                            }
                            crate::fit::context_menu(&response, |ui| {
                                if ui.button(crate::i18n::tr("Rename\u{2026}")).clicked() {
                                    state.renaming = Some((entry.name.clone(), entry.name.clone()));
                                    ui.close();
                                }
                                if ui.button(crate::i18n::tr("Delete")).clicked() {
                                    deleting = Some((remote::join(&folder, &entry.name), entry.folder));
                                    ui.close();
                                }
                            });
                            if !entry.folder && response.double_clicked() && state.saving.is_none() && !state.project {
                                open = Some(remote::join(&folder, &entry.name));
                            }
                        }
                    });
                    ui.separator();
                    if state.project {
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!busy, egui::Button::new(crate::i18n::tf!("Open {folder} as a project"))).on_hover_text(crate::i18n::tr("Its files are fetched into a folder on this computer, and each save is sent back")).clicked() {
                                open_project = Some(folder.clone());
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        if state.project {
                            return;
                        }
                        ui.label(crate::i18n::tr("File name:"));
                        ui.add(egui::TextEdit::singleline(&mut state.file_name).desired_width(300.0));
                        let name_ok = !state.file_name.trim().is_empty() && !state.file_name.contains('/');
                        if state.saving.is_some() {
                            if ui.add_enabled(name_ok && !busy, egui::Button::new(crate::i18n::tr("Save here"))).clicked() {
                                save_here = true;
                            }
                        } else if !state.chosen.is_empty() {
                            if ui.add_enabled(!busy, egui::Button::new(crate::i18n::tf!("Open the {} chosen", state.chosen.len()))).clicked() {
                                open_many = state.chosen.iter().map(|name| remote::join(&folder, name)).collect();
                            }
                        } else if ui.add_enabled(name_ok && !busy, egui::Button::new(crate::i18n::tr("Open"))).clicked() {
                            open = Some(remote::join(&folder, state.file_name.trim()));
                        }
                    });
                }
            }
            crate::fit::Part::Buttons => {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(crate::i18n::tr("Close")).clicked() {
                        close = true;
                    }
                });
            }
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
        if let (Some(folder), Some(site)) = (open_project, site.clone()) {
            self.close_dialog();
            self.open_remote_project(site, folder);
            return true;
        }
        if let Some(site) = site.clone() {
            for path in open_many {
                self.remote_fetch(site.clone(), path);
            }
        }
        if let Some((path, is_folder)) = deleting {
            let question = if is_folder { crate::i18n::tf!("Delete the folder {path} on the server? It has to be empty.") } else { crate::i18n::tf!("Delete {path} on the server? This can't be undone.") };
            if ask_yes_no("Delete", &question) {
                change = Some(Change::Remove { path, folder: is_folder });
            }
        }
        if (change.is_some() || change_cancelled)
            && let Dialog::Remote(state) = &mut self.dialog
        {
            state.renaming = None;
            state.new_folder = None;
            state.chosen.clear();
        }
        if let (Some(change), Some(site), Some(folder)) = (change, site.clone(), folder.clone()) {
            self.start_remote(Purpose::Browse, site, Task::Change { change, folder });
        }
        if save_here && let (Some(site), Some(folder), Some(id)) = (site, folder, saving) {
            let path = remote::join(&folder, &file_name);
            if !exists || ask_yes_no("Save to a server", &crate::i18n::tf!("{file_name} is already there. Replace it?")) {
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

    /// Every so often, each remote file open is looked at on its server, so a change made there is noticed while
    /// it's open, not only on saving.
    pub(super) fn watch_remote_files(&mut self, ctx: &Context) {
        let open: Vec<(u64, RemoteLink)> = self.docs.iter().filter_map(|doc| doc.remote.clone().filter(|l| l.stamp.is_some()).map(|link| (doc.id, link))).collect();
        if open.is_empty() {
            return;
        }
        ctx.request_repaint_after(WATCH_REMOTE_EVERY);
        if self.remote_watched.is_some_and(|at| at.elapsed() < WATCH_REMOTE_EVERY) {
            return;
        }
        self.remote_watched = Some(std::time::Instant::now());
        for (id, link) in open {
            let busy = self.remote_jobs.iter().any(|(purpose, _)| matches!(purpose, Purpose::Watch(i) | Purpose::Refresh(i) | Purpose::Send(i) if *i == id));
            // Only with a way to sign in already known, so nothing's asked for.
            let known = self.remote_secrets.contains_key(&link.site.key()) || link.site.key_file.is_some() || remote::stored_password(&link.site).is_some();
            if !busy && known {
                self.start_remote(Purpose::Watch(id), link.site.clone(), Task::Look(link.path.clone()));
            }
        }
    }

    /// An sftp:// address, from the command line, Open an address or the recent files: fetched and opened, with the
    /// remote box to sign in if it's needed.
    pub(super) fn open_sftp_address(&mut self, address: &str) -> bool {
        match remote::parse_url(address) {
            Ok((site, path)) if path != "/" && !path.ends_with('/') => {
                // Signing in as that site did before.
                let site = self.remote_sites.iter().find(|s| s.site.host == site.host && s.site.user == site.user && s.site.port == site.port).map_or(site, |s| s.site.clone());
                self.remote_opening = Some(path.clone());
                self.remote_fetch(site, path);
                true
            }
            Ok(_) => {
                self.open_remote_box(None);
                if let Dialog::Remote(state) = &mut self.dialog {
                    state.address = address.to_string();
                }
                self.remote_connect(None);
                true
            }
            Err(problem) => {
                self.message("Open", problem);
                false
            }
        }
    }

    /// Words for the status bar while files are being fetched or sent.
    pub(super) fn remote_status(&self) -> Option<String> {
        let sending = self.remote_jobs.iter().find(|(purpose, _)| matches!(purpose, Purpose::Send(_)))?;
        Some(crate::i18n::tf!("Sending to {}: {:.0}%", sending.1.site.host, (sending.1.share() * 100.0).min(100.0)))
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
        let sites = vec![
            SavedSite { name: "work".into(), site: Site { host: "h".into(), port: 2222, user: "u".into(), key_file: Some(PathBuf::from("C:/k")), ..Site::default() } },
            SavedSite { name: "home".into(), site: Site { host: "g".into(), port: 22, user: "v".into(), key_file: None, ..Site::default() } },
            SavedSite { name: "nas".into(), site: Site { scheme: remote::Scheme::Ftpes, host: "nas".into(), port: 21, user: "ann".into(), key_file: None } },
        ];
        let text = sites_to_settings(&sites);
        assert!(!text.to_lowercase().contains("password"));
        let back = sites_from_settings(&text.lines().filter_map(|l| l.strip_prefix("sftp_site=")).map(str::to_string).collect::<Vec<_>>());
        assert_eq!(back, sites);
        // One kept before FTP could be used is SFTP.
        assert_eq!(sites_from_settings(&["old\u{1}h\u{1}22\u{1}u\u{1}".to_string()])[0].site.scheme, remote::Scheme::Sftp);
    }
}
