//! The window's side of the `limoniter` command for WSL: looking in a distribution when a terminal for it starts,
//! the one-time offer over the terminal, and the window in Tools that lists each distribution.

use super::*;
use crate::wsl_command::{Done, Found, Status};
use std::sync::mpsc::{Receiver, Sender, channel};

/// What the person answered the offer with.
pub(super) enum Answer {
    Install,
    NotNow,
    DontAsk,
}

pub(super) struct WslCommand {
    tell: Sender<Done>,
    hear: Receiver<Done>,
    /// What each distribution looked at has.
    known: Vec<(String, Found)>,
    /// The distribution being offered the command, if one is.
    pub offer: Option<String>,
    /// Looked at already in this run, so a second terminal doesn't ask again.
    looked_at: Vec<String>,
    /// Told "don't ask again", kept in a file beside the settings.
    declined: Vec<String>,
    declined_read: bool,
    pub window_open: bool,
    problem: Option<String>,
}

impl Default for WslCommand {
    fn default() -> Self {
        let (tell, hear) = channel();
        Self { tell, hear, known: Vec::new(), offer: None, looked_at: Vec::new(), declined: Vec::new(), declined_read: false, window_open: false, problem: None }
    }
}

/// Where Limoniter is, as Windows writes it (and WSL turns it back into /mnt/c/…).
fn program_path() -> String {
    let path = std::env::current_exe().map(|exe| exe.display().to_string()).unwrap_or_default();
    path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
}

fn declined_file() -> Option<PathBuf> {
    settings_folder().map(|folder| folder.join("wsl-command-declined.txt"))
}

impl Limoniter {
    fn wsl_waker(&self) -> impl Fn() + Send + 'static + use<> {
        let ctx = self.waker.clone();
        move || {
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
        }
    }

    /// A terminal for a WSL distribution has started: look to see whether it has the command.
    pub(super) fn wsl_terminal_started(&mut self, distribution: &str) {
        if !cfg!(windows) || self.wsl_command.looked_at.iter().any(|name| name == distribution) {
            return;
        }
        self.wsl_command.looked_at.push(distribution.to_string());
        if !self.wsl_command.declined_read {
            self.wsl_command.declined_read = true;
            self.wsl_command.declined = declined_file().and_then(|file| std::fs::read_to_string(file).ok()).map(|text| text.lines().map(str::to_string).collect()).unwrap_or_default();
        }
        crate::wsl_command::check_in_background(distribution.to_string(), program_path(), self.wsl_command.tell.clone(), self.wsl_waker());
    }

    /// Takes in what the background work found, and draws the window in Tools if it's open.
    pub(super) fn wsl_command_tick(&mut self, ctx: &Context) {
        while let Ok(done) = self.wsl_command.hear.try_recv() {
            let state = &mut self.wsl_command;
            match done {
                Done::Checked(name, found) => {
                    if found.status == Status::None && !state.declined.contains(&name) && state.offer.is_none() && !state.window_open {
                        state.offer = Some(name.clone());
                    }
                    state.known.retain(|(known, _)| *known != name);
                    state.known.push((name, found));
                }
                Done::Installed(name, result) => {
                    match result {
                        Ok(()) => {
                            let on_path = state.known.iter().find(|(known, _)| *known == name).is_some_and(|(_, found)| found.on_path);
                            state.known.retain(|(known, _)| *known != name);
                            state.known.push((name.clone(), Found { status: Status::Mine(program_path()), on_path }));
                        }
                        Err(problem) => state.problem = Some(problem),
                    }
                    if state.offer.as_deref() == Some(name.as_str()) {
                        state.offer = None;
                    }
                }
                Done::Removed(name, result) => match result {
                    Ok(()) => {
                        let on_path = state.known.iter().find(|(known, _)| *known == name).is_some_and(|(_, found)| found.on_path);
                        state.known.retain(|(known, _)| *known != name);
                        state.known.push((name, Found { status: Status::None, on_path }));
                    }
                    Err(problem) => state.problem = Some(problem),
                },
            }
        }
        if self.wsl_command.window_open {
            self.wsl_command_window(ctx);
        }
    }

    /// What was answered to the offer over the terminal.
    pub(super) fn wsl_answered(&mut self, answer: Answer) {
        let Some(name) = self.wsl_command.offer.take() else {
            return;
        };
        match answer {
            Answer::Install => crate::wsl_command::install_in_background(name, program_path(), self.wsl_command.tell.clone(), self.wsl_waker()),
            Answer::NotNow => {}
            Answer::DontAsk => {
                self.wsl_command.declined.push(name);
                if let Some(file) = declined_file() {
                    let _ = crate::codec::write_file(&file, (self.wsl_command.declined.join("\n") + "\n").as_bytes());
                }
            }
        }
    }

    /// The offer, in a bar over the terminal, if there is one.
    pub(super) fn wsl_offer_bar(ui: &mut Ui, name: &str, on_path: Option<bool>) -> Option<Answer> {
        let mut answer = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::tf!("{name} has no limoniter command, to open files in this window from its shell."));
            if ui.button(crate::i18n::tf!("Install the limoniter command in {name}")).clicked() {
                answer = Some(Answer::Install);
            }
            if ui.button(crate::i18n::tr("Not now")).clicked() {
                answer = Some(Answer::NotNow);
            }
            if ui.button(crate::i18n::tr("Don't ask again")).clicked() {
                answer = Some(Answer::DontAsk);
            }
            if on_path == Some(false) {
                ui.label(RichText::new(crate::i18n::tr("It goes in ~/.local/bin, which isn't on that shell's PATH yet; add it there, or open a new shell after installing if your profile does so.")).size(12.0).color(theme::text_dim()));
            }
        });
        ui.separator();
        answer
    }

    /// Whether `~/.local/bin` is known to be on a distribution's PATH.
    pub(super) fn wsl_on_path(&self, name: &str) -> Option<bool> {
        self.wsl_command.known.iter().find(|(known, _)| known == name).map(|(_, found)| found.on_path)
    }

    fn wsl_command_window(&mut self, ctx: &Context) {
        let mut open = true;
        let mut install = None;
        let mut remove = None;
        let names: Vec<String> = crate::terminal::shells().into_iter().filter_map(|shell| if let crate::terminal::Shell::Wsl(name) = shell { Some(name) } else { None }).collect();
        // Each distribution is looked at as the window opens.
        let unknown: Vec<String> = names.iter().filter(|name| !self.wsl_command.known.iter().any(|(known, _)| known == *name)).cloned().collect();
        for name in unknown {
            crate::wsl_command::check_in_background(name, program_path(), self.wsl_command.tell.clone(), self.wsl_waker());
        }
        let state = &self.wsl_command;
        crate::fit::Dialog::new(crate::i18n::tr("Limoniter command in WSL")).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(crate::i18n::tr("The limoniter command opens files in this window from a WSL shell, as it does from any other terminal. It's a small script in the distribution's ~/.local/bin."));
            ui.add_space(6.0);
            if names.is_empty() {
                ui.label(crate::i18n::tr("There are no WSL distributions on this computer."));
            }
            for name in &names {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).strong());
                    match state.known.iter().find(|(known, _)| known == name).map(|(_, found)| &found.status) {
                        None => {
                            ui.label(crate::i18n::tr("Looking\u{2026}"));
                        }
                        Some(Status::Mine(_)) => {
                            ui.label(crate::i18n::tr("Installed"));
                            if ui.button(crate::i18n::tr("Remove")).clicked() {
                                remove = Some(name.clone());
                            }
                        }
                        Some(Status::Other) => {
                            ui.label(crate::i18n::tr("It has a limoniter command of its own, which is left alone."));
                        }
                        Some(Status::None) => {
                            if ui.button(crate::i18n::tr("Install")).clicked() {
                                install = Some(name.clone());
                            }
                        }
                    }
                });
            }
            if let Some(problem) = &state.problem {
                ui.colored_label(theme::problem_mark(), problem.as_str());
            }
        });
        if let Some(name) = install {
            self.wsl_command.problem = None;
            crate::wsl_command::install_in_background(name, program_path(), self.wsl_command.tell.clone(), self.wsl_waker());
        }
        if let Some(name) = remove {
            self.wsl_command.problem = None;
            crate::wsl_command::remove_in_background(name, self.wsl_command.tell.clone(), self.wsl_waker());
        }
        self.wsl_command.window_open = open;
    }
}
