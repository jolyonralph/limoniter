//! The window's side of terminals on a server: which of the saved servers to offer, starting a session with
//! the logins and trusted keys the SFTP box keeps, and dealing with a server that wasn't trusted yet.

use super::*;
use crate::remote::{self, Refused, Site};
use crate::terminal::Terminal;
use remote_host::SavedSite;

/// The saved servers a terminal can open on: those reached over SSH (SFTP's own), by the names they were saved
/// under. An FTP server has no shell.
pub(super) fn ssh_servers(sites: &[SavedSite]) -> Vec<(String, Site)> {
    sites.iter().filter(|saved| !saved.site.scheme.is_ftp()).map(|saved| (saved.name.clone(), saved.site.clone())).collect()
}

/// Where on a server a folder of a remote project's mirror is: the project's own folder, with the way down to it.
pub(super) fn server_path(project_folder: &str, inside: Option<&str>) -> String {
    match inside {
        Some(inside) if !inside.is_empty() => remote::join(project_folder, inside),
        _ => project_folder.to_string(),
    }
}

impl Limoniter {
    /// A session on a server, as the SFTP box would sign in to it: the password or passphrase typed this session
    /// or kept by the system, the key file, or the agent; and the keys already trusted.
    fn ssh_terminal(&self, site: &Site, folder: Option<String>) -> Terminal {
        Terminal::start_ssh(site.clone(), self.secret_for(site), self.known_hosts.clone(), folder, 12, 80)
    }

    /// The box for choosing a server (and a folder on it) to open a terminal on: the SFTP box, which signs in,
    /// checks the server's key and keeps the password in the same way.
    pub(super) fn open_ssh_terminal_box(&mut self) {
        self.open_remote_box(None);
        if let Dialog::Remote(state) = &mut self.dialog {
            state.terminal = true;
            if !state.address.starts_with("sftp://") {
                state.address = "sftp://".to_string();
            }
        }
    }

    /// Opens a terminal on a server, in a folder there if one is given.
    pub(super) fn open_ssh_terminal(&mut self, site: &Site, folder: Option<String>) {
        let terminal = self.ssh_terminal(site, folder);
        self.show_terminal(terminal);
    }

    /// Opens a terminal on the server of the open remote project, in the folder given (a folder of its mirror
    /// here), or in the project's own folder.
    pub(super) fn open_server_terminal_here(&mut self, local: Option<&Path>) {
        let Some(project) = self.remote_project.as_ref().filter(|p| !p.origin.site.scheme.is_ftp()) else { return };
        let inside = local.filter(|local| !same_path(local, &project.places.files)).and_then(|local| project.relative(local));
        let (site, folder) = (project.origin.site.clone(), server_path(&project.origin.folder, inside.as_deref()));
        self.open_ssh_terminal(&site, Some(folder));
    }

    /// Looks at the terminals on servers for what needs the window: a password to remember for this session, or a
    /// server not met before, whose key is asked about; if it's trusted, the session starts again.
    pub(super) fn check_ssh_terminals(&mut self) {
        let mut asked: Vec<(u32, Site, String)> = Vec::new();
        let front = self.terminal_number;
        let all = self.terminal.iter().map(|t| (front, &**t)).chain(self.terminals_behind.iter().map(|(n, t)| (*n, &**t)));
        let mut learned = Vec::new();
        for (number, terminal) in all {
            let Some((site, _)) = &terminal.ssh else { continue };
            if let Some(secret) = terminal.take_learned() {
                learned.push((site.key(), secret));
            }
            if let Some(Refused::UnknownHost(print)) = terminal.take_refusal() {
                asked.push((number, site.clone(), print));
            }
        }
        self.remote_secrets.extend(learned);
        for (number, site, print) in asked {
            let question = format!("{}\n\n{}", Refused::UnknownHost(print.clone()).describe(&site), crate::i18n::tr("Trust this key and connect?"));
            if !ask_yes_no("Connect to a new server", &question) {
                continue;
            }
            self.known_hosts.trust(&site, &print);
            self.save_settings_if_changed();
            let folder = self.terminal_by_number(number).and_then(|t| t.ssh.as_ref().and_then(|(_, folder)| folder.clone()));
            let fresh = self.ssh_terminal(&site, folder);
            if let Some(slot) = self.terminal_slot(number) {
                **slot = fresh;
            }
        }
    }

    fn terminal_by_number(&self, number: u32) -> Option<&Terminal> {
        if number == self.terminal_number { self.terminal.as_deref() } else { self.terminals_behind.iter().find(|(n, _)| *n == number).map(|(_, t)| &**t) }
    }

    fn terminal_slot(&mut self, number: u32) -> Option<&mut Box<Terminal>> {
        if number == self.terminal_number { self.terminal.as_mut() } else { self.terminals_behind.iter_mut().find(|(n, _)| *n == number).map(|(_, t)| t) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::Scheme;

    fn saved(name: &str, scheme: Scheme) -> SavedSite {
        SavedSite { name: name.into(), site: Site { scheme, host: format!("{name}.example.org"), port: scheme.default_port(), user: "ann".into(), key_file: None } }
    }

    #[test]
    fn only_servers_with_a_shell_are_offered_in_the_order_saved() {
        let sites = [saved("nas", Scheme::Ftp), saved("web", Scheme::Sftp), saved("secure", Scheme::Ftps), saved("home", Scheme::Sftp)];
        let names: Vec<String> = ssh_servers(&sites).into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["web", "home"]);
        assert!(ssh_servers(&[]).is_empty());
    }

    #[test]
    fn a_project_folder_maps_to_its_place_on_the_server() {
        assert_eq!(server_path("/srv/site", None), "/srv/site");
        assert_eq!(server_path("/srv/site", Some("")), "/srv/site");
        assert_eq!(server_path("/srv/site", Some("css/old")), "/srv/site/css/old");
        assert_eq!(server_path("/", Some("etc")), "/etc");
    }
}
