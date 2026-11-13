//! The Git wizard: for someone who's never used Git, a step at a time from nothing to their first commit, and to
//! their work kept safe online if they'd like. Every step says what it does and why, in plain words, and each one
//! can be come back to.

use super::git_host::{Job, explain};
use super::*;
use crate::git;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Welcome,
    Install,
    Identity,
    Project,
    Online,
    Done,
}

/// How the person wants to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Start {
    ThisFolder,
    Copy,
}

pub(super) struct Wizard {
    pub step: Step,
    pub name: String,
    pub email: String,
    /// The folder to keep the history of, or to copy a project into.
    pub folder: Option<PathBuf>,
    pub start: Start,
    pub ignore: String,
    pub copy_from: String,
    /// Where the project goes online.
    pub online: String,
    pub job: Option<Job>,
    /// What happened at the last step, and whether it was a problem.
    pub said: Option<(String, bool)>,
}

impl Limoniter {
    /// Opens the wizard, at the first step that hasn't been done.
    pub(super) fn open_git_wizard(&mut self) {
        let folder = self.docs.get(self.active).and_then(|doc| doc.path.as_deref()).and_then(Path::parent).map(Path::to_path_buf);
        let (name, email) = if git::program().is_some() { git::identity(None) } else { (None, None) };
        let ignore = folder.as_deref().map(git::suggested_ignore).unwrap_or_default();
        self.git.installed = Some(git::program().is_some());
        self.git.wizard = Some(Box::new(super::git_wizard::Wizard { step: Step::Welcome, name: name.unwrap_or_default(), email: email.unwrap_or_default(), folder, start: Start::ThisFolder, ignore, copy_from: String::new(), online: String::new(), job: None, said: None }));
    }

    /// Draws the wizard while it's open; true if it is.
    pub(super) fn git_wizard_dialog(&mut self, ctx: &Context) -> bool {
        let Some(wizard) = self.git.wizard.as_mut() else {
            return false;
        };
        // What the last thing done in the background came to.
        let mut finished = false;
        if let Some(job) = &wizard.job
            && let Some(outcome) = job.take()
        {
            wizard.job = None;
            finished = true;
            match outcome {
                Ok(_) => {
                    wizard.said = None;
                    wizard.step = match wizard.step {
                        Step::Project => Step::Online,
                        _ => Step::Done,
                    };
                }
                Err(problem) => wizard.said = Some((explain(&problem), true)),
            }
        }
        if finished {
            // The file in front may be in a repository now, or one may have been put online.
            self.git.repo = None;
            self.git.repo_of = None;
            self.git.refresh();
        }
        let Some(wizard) = self.git.wizard.as_mut() else {
            return false;
        };
        if wizard.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape)) && wizard.job.is_none();
        let mut go: Option<Do> = None;
        let step = wizard.step;
        egui::Window::new("Git, step by step").collapsible(false).resizable(false).default_width(560.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(560.0);
            let steps = ["Welcome", "Get Git", "Who you are", "Your project", "Online", "Done"];
            let at = [Step::Welcome, Step::Install, Step::Identity, Step::Project, Step::Online, Step::Done].iter().position(|s| *s == step).unwrap_or(0);
            ui.horizontal_wrapped(|ui| {
                for (n, name) in steps.iter().enumerate() {
                    let text = RichText::new(format!("{}. {name}", n + 1));
                    ui.label(if n == at { text.strong().color(theme::accent()) } else { text.color(theme::text_faint()) });
                }
            });
            ui.separator();
            let para = |ui: &mut Ui, text: &str| {
                ui.add(egui::Label::new(text).wrap());
                ui.add_space(4.0);
            };
            match step {
                Step::Welcome => {
                    para(ui, "Git keeps the history of a folder of files. Whenever you like, you make a commit: a snapshot of how everything is, with a line saying what you did. Later you can see what changed, compare any two versions, and go back to any of them.");
                    para(ui, "It's how almost all software is made, but it's just as good for a novel, a website, notes, or a BASIC program. It's free, and everything stays on your computer unless you choose to put it online, on GitHub for example, where it's backed up and can be shared.");
                    para(ui, "This takes a few minutes, a step at a time. Limoniter then shows what's changed beside each line, and lets you commit, compare and go back from the Source control panel (Ctrl+Shift+G).");
                    if ui.button("Let's start").clicked() {
                        go = Some(Do::To(Step::Install));
                    }
                }
                Step::Install => match git::version() {
                    Some(version) => {
                        para(ui, &format!("Git {version} is installed. \u{2714}"));
                        if ui.button("Next").clicked() {
                            go = Some(Do::To(Step::Identity));
                        }
                    }
                    None => {
                        para(ui, "First, Git itself. It's a free program from git-scm.com.");
                        if cfg!(windows) {
                            para(ui, "Download Git for Windows and run the installer. Its usual choices are all fine: just keep pressing Next. It brings Git Credential Manager, which signs in to GitHub for you later.");
                        } else if cfg!(target_os = "macos") {
                            para(ui, "On a Mac, open Terminal and type: xcode-select --install. That installs Apple's developer tools, Git among them. (Or install it from git-scm.com.)");
                        } else {
                            para(ui, "On Linux, install it with your package manager, such as: sudo apt install git (Debian, Ubuntu) or sudo dnf install git (Fedora).");
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Open the download page").clicked() {
                                go = Some(Do::Web("https://git-scm.com/downloads".into()));
                            }
                            if ui.button("I've installed it: look again").clicked() {
                                go = Some(Do::To(Step::Install));
                            }
                        });
                    }
                },
                Step::Identity => {
                    para(ui, "Each commit says who made it. Git needs a name and an email for that; they're saved in Git's own settings, for every project.");
                    para(ui, "If you'll put work on GitHub and would rather your email wasn't public, GitHub gives you a private one to use instead (in GitHub's Settings, Emails, it ends @users.noreply.github.com).");
                    egui::Grid::new("git-identity").num_columns(2).show(ui, |ui| {
                        ui.label("Your name:");
                        ui.add(egui::TextEdit::singleline(&mut wizard.name).desired_width(300.0));
                        ui.end_row();
                        ui.label("Your email:");
                        ui.add(egui::TextEdit::singleline(&mut wizard.email).desired_width(300.0));
                        ui.end_row();
                    });
                    let ready = !wizard.name.trim().is_empty() && wizard.email.contains('@');
                    if ui.add_enabled(ready, Button::new("Save, and next")).clicked() {
                        go = Some(Do::Identity);
                    }
                }
                Step::Project => {
                    let in_repo = wizard.folder.as_deref().and_then(git::repository_of);
                    if let Some(root) = in_repo.filter(|_| wizard.start == Start::ThisFolder) {
                        para(ui, &format!("{} is already keeping its history with Git. \u{2714}", root.display()));
                        if ui.button("Next").clicked() {
                            go = Some(Do::To(Step::Online));
                        }
                    } else {
                        ui.radio_value(&mut wizard.start, Start::ThisFolder, "Start keeping the history of a folder");
                        ui.radio_value(&mut wizard.start, Start::Copy, "Copy a project that's already online, from GitHub or elsewhere");
                        ui.add_space(4.0);
                        match wizard.start {
                            Start::ThisFolder => {
                                ui.horizontal(|ui| {
                                    ui.label(wizard.folder.as_ref().map_or_else(|| "No folder chosen".to_string(), |f| f.display().to_string()));
                                    if ui.button("Choose\u{2026}").clicked() {
                                        go = Some(Do::ChooseFolder);
                                    }
                                });
                                para(ui, "Git will start keeping this folder's history, and make a first commit of everything in it as it is now.");
                                para(ui, "These kinds of file are left out (a .gitignore file lists them, and you can change it at any time):");
                                ui.add(egui::TextEdit::multiline(&mut wizard.ignore).font(egui::TextStyle::Monospace).desired_rows(5).desired_width(f32::INFINITY));
                                if ui.add_enabled(wizard.folder.is_some() && wizard.job.is_none(), Button::new("Start, with a first commit")).clicked() {
                                    go = Some(Do::Start);
                                }
                            }
                            Start::Copy => {
                                para(ui, "On GitHub, the green Code button on a project's page gives its address.");
                                egui::Grid::new("git-copy").num_columns(2).show(ui, |ui| {
                                    ui.label("Its address:");
                                    ui.add(egui::TextEdit::singleline(&mut wizard.copy_from).hint_text("https://github.com/someone/project.git").desired_width(320.0));
                                    ui.end_row();
                                    ui.label("Into:");
                                    ui.horizontal(|ui| {
                                        ui.label(wizard.folder.as_ref().map_or_else(|| "No folder chosen".to_string(), |f| f.display().to_string()));
                                        if ui.button("Choose\u{2026}").clicked() {
                                            go = Some(Do::ChooseFolder);
                                        }
                                    });
                                    ui.end_row();
                                });
                                let ready = wizard.folder.is_some() && !wizard.copy_from.trim().is_empty() && wizard.job.is_none();
                                if ui.add_enabled(ready, Button::new("Copy it")).on_hover_text("A folder named after the project is made inside the one chosen").clicked() {
                                    go = Some(Do::Copy);
                                }
                            }
                        }
                    }
                }
                Step::Online => {
                    let root = wizard.folder.as_deref().and_then(git::repository_of);
                    let connected = root.as_deref().map(git::remotes).unwrap_or_default();
                    if let Some((name, url)) = connected.first() {
                        para(ui, &format!("It's connected to {url} (as {name}). \u{2714} Push, in the Source control panel, sends your commits there."));
                        if ui.button("Next").clicked() {
                            go = Some(Do::To(Step::Done));
                        }
                    } else {
                        para(ui, "This step is up to you. Putting your project online keeps a copy safe if your computer fails, and lets you share it or work on it from another computer. GitHub is the best known place, and free; projects there can be private.");
                        para(ui, "1. Make a free GitHub account, if you haven't one, and sign in.\n2. Make a new repository there. Give it a name, choose private or public, and leave the boxes to add files unticked.\n3. Copy its address (it ends .git) into the box below.");
                        if ui.button("Open GitHub to make a repository").clicked() {
                            let name = root.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().replace(' ', "-")).unwrap_or_default();
                            go = Some(Do::Web(format!("https://github.com/new?name={name}")));
                        }
                        ui.add(egui::TextEdit::singleline(&mut wizard.online).hint_text("https://github.com/you/project.git").desired_width(400.0));
                        para(ui, if cfg!(target_os = "linux") { "When it's sent, Git asks GitHub who you are. On Linux that's easiest with GitHub's own gh program (gh auth login), or an SSH key." } else { "When it's sent, a window from Git asks you to sign in to GitHub, the first time only." });
                        ui.horizontal(|ui| {
                            let ready = root.is_some() && !wizard.online.trim().is_empty() && wizard.job.is_none();
                            if ui.add_enabled(ready, Button::new("Connect, and send it")).clicked() {
                                go = Some(Do::Online);
                            }
                            if ui.button("Not now").clicked() {
                                go = Some(Do::To(Step::Done));
                            }
                        });
                    }
                }
                Step::Done => {
                    para(ui, "That's it. From now on:");
                    para(ui, "\u{2022} Coloured marks beside the line numbers show what's changed since your last commit: green for new lines, blue for changed ones, a red notch where lines were taken out.\n\u{2022} When you've done something worth keeping, open Source control (Ctrl+Shift+G), write a line saying what you did, and press Commit.\n\u{2022} Push sends your commits online; Pull brings in ones made elsewhere.\n\u{2022} History shows every version of a file, to compare with now or open as it was.");
                    para(ui, "Commit little and often: each commit is a point you can always come back to.");
                    if ui.button("Open Source control").clicked() {
                        go = Some(Do::Finish);
                    }
                }
            }
            if let Some(job) = &wizard.job {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("{}\u{2026}", job.title));
                });
            }
            if let Some((said, problem)) = &wizard.said {
                ui.add(egui::Label::new(RichText::new(said).color(if *problem { theme::rainbow_mistake() } else { theme::text_dim() })).wrap());
            }
            ui.separator();
            ui.horizontal(|ui| {
                if at > 0 && wizard.job.is_none() && ui.button("Back").clicked() {
                    go = Some(Do::To([Step::Welcome, Step::Install, Step::Identity, Step::Project, Step::Online, Step::Done][at - 1]));
                }
                close |= ui.add_enabled(wizard.job.is_none(), Button::new("Close")).clicked();
            });
        });
        let Some(wizard) = self.git.wizard.as_mut() else {
            return true;
        };
        match go {
            Some(Do::To(step)) => {
                wizard.step = step;
                wizard.said = None;
            }
            Some(Do::Web(address)) => {
                if let Err(problem) = crate::platform::open_web_page(&address) {
                    wizard.said = Some((problem, true));
                }
            }
            Some(Do::Identity) => match git::set_identity(wizard.name.trim(), wizard.email.trim()) {
                Ok(()) => {
                    wizard.step = Step::Project;
                    wizard.said = None;
                }
                Err(problem) => wizard.said = Some((problem, true)),
            },
            Some(Do::ChooseFolder) => {
                if let Some(folder) = rfd::FileDialog::new().set_title("Choose the folder").pick_folder() {
                    wizard.ignore = git::suggested_ignore(&folder);
                    wizard.folder = Some(folder);
                }
            }
            Some(Do::Start) => {
                let Some(folder) = wizard.folder.clone() else {
                    return true;
                };
                let ignore = wizard.ignore.clone();
                wizard.said = None;
                wizard.job = Some(Job::start("Starting", move || start_repository(&folder, &ignore)));
            }
            Some(Do::Copy) => {
                let Some(parent) = wizard.folder.clone() else {
                    return true;
                };
                let url = wizard.copy_from.trim().to_string();
                let name = url.trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or("project").to_string();
                let into = parent.join(&name);
                wizard.folder = Some(into.clone());
                wizard.said = None;
                wizard.job = Some(Job::start("Copy", move || git::clone(&url, &into).map(|()| String::new())));
            }
            Some(Do::Online) => {
                let Some(root) = wizard.folder.as_deref().and_then(git::repository_of) else {
                    return true;
                };
                let url = wizard.online.trim().to_string();
                wizard.said = None;
                wizard.job = Some(Job::start("Sending it online", move || {
                    git::add_remote(&root, "origin", &url)?;
                    let status = git::status(&root)?;
                    git::push(&root, &status)
                }));
            }
            Some(Do::Finish) => {
                self.git.wizard = None;
                self.git.open = true;
                self.git.refresh();
                return true;
            }
            None => {}
        }
        if close {
            self.git.wizard = None;
            self.git.refresh();
            // The file in front may be in a repository now.
            self.git.repo = None;
            self.git.repo_of = None;
            self.git.status = None;
        }
        true
    }
}

/// What a step asks for.
enum Do {
    To(Step),
    Web(String),
    Identity,
    ChooseFolder,
    Start,
    Copy,
    Online,
    Finish,
}

/// Starts keeping a folder's history: a repository, the files to leave out, and a first commit of everything.
pub(super) fn start_repository(folder: &Path, ignore: &str) -> Result<String, String> {
    git::init(folder)?;
    let ignore_file = folder.join(".gitignore");
    if !ignore.trim().is_empty() && !ignore_file.exists() {
        std::fs::write(&ignore_file, ignore).map_err(|e| format!("Couldn't write .gitignore: {e}"))?;
    }
    git::run(folder, &["add", "--all"])?;
    git::commit(folder, "First version\n\nThe files as they were when Git started keeping their history.", false)
}
