//! Add-ons: scripts that add commands of their own to the menus and the command box, and can run when a file is
//! opened or saved. Each is one `.rhai` file, describing itself in lines that start `//!`:
//!
//! ```text
//! //! name: Tidy CSV
//! //! about: Trims the spaces round every field.
//! //! command: Tidy CSV fields = tidy
//! //! on_open: when_opened
//! //! on_save: before_saving
//! on_send: before_sending
//! on_run: before_running
//! ```
//!
//! They live in the add-ons folder beside the scripts, and in a shared folder if one is set, so a team can keep
//! theirs in one place. They can do no more than any script can: nothing outside the open files, no network.
//! Even so, a new add-on stays off until it's turned on, and one whose file has changed since is turned off again,
//! so nothing runs by itself that the person hasn't agreed to.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const FOLDER: &str = "addons";
const MOST_COMMANDS: usize = 40;
const LOOK_AGAIN: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddOnCommand {
    pub title: String,
    pub function: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddOn {
    pub path: PathBuf,
    pub name: String,
    pub about: String,
    pub commands: Vec<AddOnCommand>,
    pub on_open: Option<String>,
    pub on_save: Option<String>,
    /// Run before File, Send to device sends the file; send_device() and set_send_name() work there.
    pub on_send: Option<String>,
    /// Run just before File, Run on machine runs it.
    pub on_run: Option<String>,
    /// From the shared folder rather than the person's own.
    pub shared: bool,
    /// What's wrong with its description, if anything; it can't be turned on until that's put right.
    pub problem: Option<String>,
    /// A fingerprint of the file, so that a change to it is noticed.
    pub fingerprint: String,
}

/// Whether an add-on may run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    On,
    Off,
    /// It was on, but the file has changed since, so it's off until it's looked at again.
    Changed,
}

/// When an add-on's function runs by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Open,
    Save,
    Send,
    Run,
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn fingerprint(source: &str) -> String {
    Sha256::digest(source.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads an add-on's description from its `//!` lines.
pub fn describe(path: &Path, source: &str, shared: bool) -> AddOn {
    let stem = path.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut addon = AddOn { path: path.to_path_buf(), name: stem, about: String::new(), commands: Vec::new(), on_open: None, on_save: None, on_send: None, on_run: None, shared, problem: None, fingerprint: fingerprint(source) };
    let mut problems = Vec::new();
    let function = |value: &str, problems: &mut Vec<String>| -> Option<String> {
        let value = value.trim().trim_end_matches("()");
        if is_identifier(value) {
            Some(value.to_string())
        } else {
            problems.push(format!("\u{201C}{value}\u{201D} isn't a function name: use letters, digits and _."));
            None
        }
    };
    for line in source.lines() {
        let Some(rest) = line.trim_start().strip_prefix("//!") else {
            continue;
        };
        let Some((key, value)) = rest.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "name" if !value.is_empty() => addon.name = value.to_string(),
            "about" => {
                if !addon.about.is_empty() {
                    addon.about.push(' ');
                }
                addon.about += value;
            }
            "command" => match value.rsplit_once('=') {
                Some((title, name)) if !title.trim().is_empty() => {
                    if let Some(function) = function(name, &mut problems) {
                        addon.commands.push(AddOnCommand { title: title.trim().to_string(), function });
                    }
                }
                _ => problems.push(format!("A command is written as \u{201C}command: Its title = function\u{201D}, not \u{201C}{value}\u{201D}.")),
            },
            "on_open" => addon.on_open = function(value, &mut problems),
            "on_save" => addon.on_save = function(value, &mut problems),
            "on_send" => addon.on_send = function(value, &mut problems),
            "on_run" => addon.on_run = function(value, &mut problems),
            other => problems.push(format!("\u{201C}{other}\u{201D} isn't something an add-on can say. It can give name, about, command, on_open, on_save, on_send and on_run.")),
        }
    }
    if addon.commands.len() > MOST_COMMANDS {
        problems.push(format!("It has {} commands; an add-on can have up to {MOST_COMMANDS}.", addon.commands.len()));
    }
    if addon.commands.is_empty() && addon.on_open.is_none() && addon.on_save.is_none() && addon.on_send.is_none() && addon.on_run.is_none() && problems.is_empty() {
        problems.push("It doesn't give any commands or events, so it has nothing to add. See How things work for how to write one.".to_string());
    }
    if !problems.is_empty() {
        addon.problem = Some(problems.join("\n"));
    }
    addon
}

/// One command of an add-on that's on, as the menus and the command box list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub addon: String,
    pub title: String,
    pub path: PathBuf,
    pub function: String,
}

#[derive(Default)]
pub struct AddOns {
    own: Option<PathBuf>,
    pub shared: Option<PathBuf>,
    pub list: Vec<AddOn>,
    /// The add-ons that are on: each file, and its fingerprint when it was turned on.
    on: Vec<(PathBuf, String)>,
    looked: Option<Instant>,
}

impl AddOns {
    /// `root` is where the person's own add-ons folder goes; None keeps nothing, as in a test.
    pub fn new(root: Option<PathBuf>) -> Self {
        Self { own: root.map(|root| root.join(FOLDER)), ..Self::default() }
    }

    pub fn own_folder(&self) -> Option<&Path> {
        self.own.as_deref()
    }

    pub fn set_shared(&mut self, folder: Option<PathBuf>) {
        self.shared = folder.filter(|f| !f.as_os_str().is_empty());
        self.refresh();
    }

    pub fn refresh_if_stale(&mut self) {
        if self.looked.is_none_or(|looked| looked.elapsed() >= LOOK_AGAIN) {
            self.refresh();
        }
    }

    pub fn refresh(&mut self) {
        self.looked = Some(Instant::now());
        let mut list = Vec::new();
        for (folder, shared) in [(self.own.clone(), false), (self.shared.clone(), true)] {
            let Some(entries) = folder.and_then(|folder| std::fs::read_dir(folder).ok()) else {
                continue;
            };
            let mut found: Vec<AddOn> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|e| e.eq_ignore_ascii_case(crate::scripting::EXTENSION)))
                .filter_map(|path| std::fs::read_to_string(&path).ok().map(|source| describe(&path, &source, shared)))
                .collect();
            found.sort_by_key(|addon| addon.name.to_lowercase());
            list.extend(found);
        }
        self.list = list;
    }

    pub fn state(&self, addon: &AddOn) -> State {
        match self.on.iter().find(|(path, _)| path == &addon.path) {
            Some((_, print)) if *print == addon.fingerprint && addon.problem.is_none() => State::On,
            Some(_) => State::Changed,
            None => State::Off,
        }
    }

    /// Turns an add-on on or off. One with a problem in its description can't be turned on.
    pub fn set_on(&mut self, path: &Path, on: bool) {
        self.on.retain(|(known, _)| known != path);
        if on && let Some(addon) = self.list.iter().find(|addon| addon.path == path && addon.problem.is_none()) {
            self.on.push((addon.path.clone(), addon.fingerprint.clone()));
        }
    }

    fn running(&self) -> impl Iterator<Item = &AddOn> {
        self.list.iter().filter(|addon| self.state(addon) == State::On)
    }

    /// Every command of every add-on that's on, in order.
    pub fn commands(&self) -> Vec<Listed> {
        self.running()
            .flat_map(|addon| addon.commands.iter().map(|command| Listed { addon: addon.name.clone(), title: command.title.clone(), path: addon.path.clone(), function: command.function.clone() }))
            .collect()
    }

    /// The functions to run when something happens: each add-on's name, file, and function.
    pub fn hooks(&self, event: Event) -> Vec<(String, PathBuf, String)> {
        self.running()
            .filter_map(|addon| {
                let function = match event {
                    Event::Open => addon.on_open.clone(),
                    Event::Save => addon.on_save.clone(),
                    Event::Send => addon.on_send.clone(),
                    Event::Run => addon.on_run.clone(),
                }?;
                Some((addon.name.clone(), addon.path.clone(), function))
            })
            .collect()
    }

    pub fn to_settings(&self) -> String {
        let mut out: String = self.on.iter().map(|(path, print)| format!("addon_on={print}\u{1}{}\n", path.display())).collect();
        if let Some(shared) = &self.shared {
            out += &format!("addons_shared={}\n", shared.display());
        }
        out
    }

    pub fn read_settings(&mut self, on: &[String], shared: Option<&str>) {
        for value in on {
            if let Some((print, path)) = value.split_once('\u{1}') {
                self.on.push((PathBuf::from(path), print.to_string()));
            }
        }
        self.shared = shared.filter(|s| !s.is_empty()).map(PathBuf::from);
    }

    /// Writes a new add-on from the example, under a name, in the person's own folder.
    pub fn create(&mut self, name: &str) -> Result<PathBuf, String> {
        let name = crate::scripting::checked_name(name)?;
        let folder = self.own.clone().ok_or("There's nowhere to keep it, because the settings folder couldn't be found.")?;
        std::fs::create_dir_all(&folder).map_err(|e| format!("The folder couldn't be made: {e}"))?;
        let path = folder.join(format!("{name}.{}", crate::scripting::EXTENSION));
        if path.exists() {
            return Err(format!("There's already an add-on called {name}."));
        }
        std::fs::write(&path, NEW_ADDON.replace("NAME", &name)).map_err(|e| format!("It couldn't be saved: {e}"))?;
        self.refresh();
        Ok(path)
    }
}

/// What a new add-on starts with, explaining itself.
pub const NEW_ADDON: &str = r#"//! name: NAME
//! about: What this add-on is for, in a sentence.
//! command: Count the words in the selection = count_selected
//! command: Show lines with TODO = list_todos
//! on_save: before_saving
//
// An add-on is a script whose //! lines above add commands to Tools > Add-ons and the command box (F1), and can
// name functions to run by themselves: on_open when a file is opened, on_save just before Save writes it, and
// on_send just before File > Send to device sends it (send_device() says where; set_send_name("NAME") renames it),
// and on_run just before File > Run on machine runs it.
// Each command or event runs the function it names. It can use everything a script can; see New script for the list.
// It's off until you turn it on in Tools > Add-ons > Manage add-ons, and turned off again if this file changes.

fn count_selected() {
    let words = regex_find_all(selected_text(), "\\S+");
    message("The selection has " + words.len() + " words.");
}

fn list_todos() {
    let found = "";
    for n in 1..=line_count() {
        let line = line_text(n);
        if line.contains("TODO") {
            found += n + ": " + line + "\n";
        }
    }
    show_panel("TODOs in " + file_name(), if found == "" { "None." } else { found });
}

fn before_saving() {
    // Runs before the file is written. This one leaves it alone; trim_trailing would take spaces off line ends:
    // command("trim_trailing");
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_add_on_says_what_it_adds() {
        let addon = describe(Path::new("x/tidy.rhai"), "//! name: Tidy\n//! about: Tidies.\n//! about: Twice.\n//! command: Tidy it = tidy\n//! command: Count = count()\n//! on_save: before\nfn tidy() {}\n", false);
        assert_eq!(addon.name, "Tidy");
        assert_eq!(addon.about, "Tidies. Twice.");
        assert_eq!(addon.commands, [AddOnCommand { title: "Tidy it".into(), function: "tidy".into() }, AddOnCommand { title: "Count".into(), function: "count".into() }]);
        assert_eq!(addon.on_save.as_deref(), Some("before"));
        assert!(addon.problem.is_none());
        let bad = describe(Path::new("bad.rhai"), "//! command: Oops\n//! on_open: not a name\n//! colour: red\n", false);
        assert_eq!(bad.problem.unwrap().lines().count(), 3);
        assert!(describe(Path::new("plain.rhai"), "message(\"hi\");", false).problem.is_some(), "a script with nothing to add isn't an add-on");
        assert!(describe(Path::new("new.rhai"), NEW_ADDON, false).problem.is_none());
    }

    #[test]
    fn add_ons_are_off_until_turned_on_and_off_again_once_changed() {
        let root = std::env::temp_dir().join(format!("limoniter-addons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut addons = AddOns::new(Some(root.clone()));
        let path = addons.create("Mine").unwrap();
        assert_eq!(addons.list.len(), 1);
        assert_eq!(addons.state(&addons.list[0]), State::Off);
        assert!(addons.commands().is_empty() && addons.hooks(Event::Save).is_empty(), "nothing runs while it's off");
        addons.set_on(&path, true);
        assert_eq!(addons.state(&addons.list[0]), State::On);
        assert_eq!(addons.commands().len(), 2);
        assert_eq!(addons.hooks(Event::Save), [("Mine".to_string(), path.clone(), "before_saving".to_string())]);
        assert!(addons.hooks(Event::Open).is_empty());
        // Kept in the settings, and still on when read back.
        let saved = addons.to_settings();
        let on: Vec<String> = saved.lines().filter_map(|l| l.strip_prefix("addon_on=")).map(str::to_string).collect();
        let mut again = AddOns::new(Some(root.clone()));
        again.read_settings(&on, None);
        again.refresh();
        assert_eq!(again.state(&again.list[0]), State::On);
        // Someone changes the file: it's off until looked at again.
        std::fs::write(&path, NEW_ADDON.replace("NAME", "Changed")).unwrap();
        again.refresh();
        assert_eq!(again.state(&again.list[0]), State::Changed);
        assert!(again.commands().is_empty());
        // A shared folder's add-ons are listed after the person's own.
        let shared = root.join("team");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("Team.rhai"), NEW_ADDON.replace("NAME", "Team")).unwrap();
        again.set_shared(Some(shared));
        assert_eq!(again.list.iter().map(|a| (a.name.as_str(), a.shared)).collect::<Vec<_>>(), [("Changed", false), ("Team", true)]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
