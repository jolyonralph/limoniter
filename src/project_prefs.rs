//! What a project says about how its files are worked on, over the person's own settings: how it indents, what
//! it leaves out of search, which formatter and which language servers it uses, and so on. Each value is
//! optional, and "not said" means the person's own setting stands. Only what's been said is kept, so the file
//! stays small and can go into Git. A `.limproj` keeps them under "settings"; a folder project keeps them in
//! `.limoniter/project.json` inside the folder, where the rest of the team can have them. An `.editorconfig`
//! still has the last word for the file it covers.

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where a folder project keeps these, inside the folder.
pub fn file_in(root: &Path) -> PathBuf {
    root.join(".limoniter").join("project.json")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    /// Patterns (as lines of .gitignore) for what's left out of search and Go to file but still shown in the Explorer.
    pub search_exclude: Vec<String>,
    /// Whether the index leaves out what .gitignore does; None is yes.
    pub follow_gitignore: Option<bool>,
    /// Spaces (true) or tab characters (false) for the Tab key.
    pub spaces: Option<bool>,
    pub tab_width: Option<usize>,
    /// The preset (line endings and encoding) new files start with, by its id.
    pub new_file_preset: Option<String>,
    pub trim_trailing: Option<bool>,
    pub final_newline: Option<bool>,
    pub format_on_save: Option<bool>,
    /// The formatter for each language, by name ("none" for no formatter), over the person's own choices.
    pub formatters: BTreeMap<String, String>,
    /// The `.env` file to give what runs in the project, written relative to its folder.
    pub env_file: Option<String>,
    /// The folder tasks run in, relative to the project's folder.
    pub work_folder: Option<String>,
    /// Languages whose language server isn't used in this project.
    pub servers_off: Vec<String>,
    /// The folder the project's file templates are in, relative to the project's folder.
    pub templates_folder: Option<String>,
    /// Whether the folder's `.vscode` settings and tasks are followed; None is yes. Import into Limoniter, which
    /// copies them in, turns it off, so the project's own values are the only ones from then on.
    pub follow_vscode: Option<bool>,
    /// The Python (its interpreter's path) the project uses, as VS Code's `python.defaultInterpreterPath` says.
    pub python: Option<String>,
    /// Whether ESLint is wanted, as VS Code's `eslint.enable` says.
    pub eslint: Option<bool>,
}

impl Prefs {
    pub fn is_empty(&self) -> bool {
        *self == Prefs::default()
    }

    /// Only what's been said.
    pub fn to_json(&self) -> Value {
        let mut value = Map::new();
        if !self.search_exclude.is_empty() {
            value.insert("search_exclude".into(), json!(self.search_exclude));
        }
        let mut put = |key: &str, what: Option<Value>| {
            if let Some(what) = what {
                value.insert(key.into(), what);
            }
        };
        put("follow_gitignore", self.follow_gitignore.map(Value::from));
        put("spaces", self.spaces.map(Value::from));
        put("tab_width", self.tab_width.map(Value::from));
        put("new_file_preset", self.new_file_preset.clone().map(Value::from));
        put("trim_trailing", self.trim_trailing.map(Value::from));
        put("final_newline", self.final_newline.map(Value::from));
        put("format_on_save", self.format_on_save.map(Value::from));
        put("env_file", self.env_file.clone().map(Value::from));
        put("work_folder", self.work_folder.clone().map(Value::from));
        put("templates_folder", self.templates_folder.clone().map(Value::from));
        put("follow_vscode", self.follow_vscode.map(Value::from));
        put("python", self.python.clone().map(Value::from));
        put("eslint", self.eslint.map(Value::from));
        if !self.formatters.is_empty() {
            value.insert("formatters".into(), json!(self.formatters));
        }
        if !self.servers_off.is_empty() {
            value.insert("servers_off".into(), json!(self.servers_off));
        }
        Value::Object(value)
    }

    pub fn from_json(value: Option<&Value>) -> Prefs {
        let Some(value) = value else { return Prefs::default() };
        let flag = |key: &str| value.get(key).and_then(Value::as_bool);
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
        let list = |key: &str| value.get(key).and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).map(str::to_string).filter(|t| !t.trim().is_empty()).collect());
        Prefs {
            search_exclude: list("search_exclude"),
            follow_gitignore: flag("follow_gitignore"),
            spaces: flag("spaces"),
            tab_width: value.get("tab_width").and_then(Value::as_u64).map(|n| n as usize).filter(|n| (crate::text_buffer::MIN_TAB_SIZE..=crate::text_buffer::MAX_TAB_SIZE).contains(n)),
            new_file_preset: text("new_file_preset").filter(|id| crate::codec::preset_by_id(id).is_some()),
            trim_trailing: flag("trim_trailing"),
            final_newline: flag("final_newline"),
            format_on_save: flag("format_on_save"),
            formatters: value.get("formatters").and_then(Value::as_object).map_or_else(BTreeMap::new, |map| map.iter().filter_map(|(language, name)| Some((language.clone(), name.as_str()?.to_string()))).collect()),
            env_file: text("env_file"),
            work_folder: text("work_folder"),
            servers_off: list("servers_off"),
            templates_folder: text("templates_folder"),
            follow_vscode: flag("follow_vscode"),
            python: text("python"),
            eslint: flag("eslint"),
        }
    }

    /// These over `under`: each value this project says stays, and what it leaves unsaid is taken from `under`
    /// (VS Code's, read from its files). A list or a language's formatter said here beats the other's.
    pub fn over(&self, under: &Prefs) -> Prefs {
        let mut formatters = under.formatters.clone();
        formatters.extend(self.formatters.iter().map(|(language, name)| (language.clone(), name.clone())));
        Prefs {
            search_exclude: if self.search_exclude.is_empty() { under.search_exclude.clone() } else { self.search_exclude.clone() },
            follow_gitignore: self.follow_gitignore.or(under.follow_gitignore),
            spaces: self.spaces.or(under.spaces),
            tab_width: self.tab_width.or(under.tab_width),
            new_file_preset: self.new_file_preset.clone().or_else(|| under.new_file_preset.clone()),
            trim_trailing: self.trim_trailing.or(under.trim_trailing),
            final_newline: self.final_newline.or(under.final_newline),
            format_on_save: self.format_on_save.or(under.format_on_save),
            formatters,
            env_file: self.env_file.clone().or_else(|| under.env_file.clone()),
            work_folder: self.work_folder.clone().or_else(|| under.work_folder.clone()),
            servers_off: if self.servers_off.is_empty() { under.servers_off.clone() } else { self.servers_off.clone() },
            templates_folder: self.templates_folder.clone().or_else(|| under.templates_folder.clone()),
            follow_vscode: self.follow_vscode.or(under.follow_vscode),
            python: self.python.clone().or_else(|| under.python.clone()),
            eslint: self.eslint.or(under.eslint),
        }
    }

    /// The options the project's index is made with.
    pub fn index_options(&self) -> crate::workspace::Options {
        crate::workspace::Options { exclude: self.search_exclude.clone(), keep_ignored: self.follow_gitignore == Some(false) }
    }

    /// A folder the project names, written relative to its own, or whole.
    pub fn place(base: &Path, written: &str) -> PathBuf {
        crate::project::resolved(base, written)
    }

    /// Whether the project turns off the language server of a language.
    pub fn server_off(&self, language: &str) -> bool {
        self.servers_off.iter().any(|l| l == language)
    }

    /// The `.env` file to use: the project's own choice, or else the `.env` in its folder.
    pub fn env_path(&self, folder: &Path) -> PathBuf {
        self.env_file.as_deref().map_or_else(|| folder.join(".env"), |written| Self::place(folder, written))
    }
}

/// Reads a text of lines, one pattern each, as the settings window shows them.
pub fn lines_of(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_never_said_is_never_written() {
        assert_eq!(Prefs::default().to_json(), json!({}));
        assert!(Prefs::default().is_empty());
        let mut some = Prefs { spaces: Some(false), ..Prefs::default() };
        assert_eq!(some.to_json(), json!({ "spaces": false }));
        some.search_exclude = vec!["*.min.js".into()];
        some.formatters.insert("Python".into(), "none".into());
        assert_eq!(some.to_json(), json!({ "spaces": false, "search_exclude": ["*.min.js"], "formatters": { "Python": "none" } }));
    }

    #[test]
    fn every_setting_is_read_back_as_it_was_written() {
        let all = Prefs {
            search_exclude: vec!["*.min.js".into(), "vendor/".into()],
            follow_gitignore: Some(false),
            spaces: Some(true),
            tab_width: Some(3),
            new_file_preset: Some("linux".into()),
            trim_trailing: Some(true),
            final_newline: Some(false),
            format_on_save: Some(true),
            formatters: BTreeMap::from([("Python".to_string(), "Black".to_string())]),
            env_file: Some("config/.env.dev".into()),
            work_folder: Some("build".into()),
            servers_off: vec!["Rust".into()],
            templates_folder: Some("tpl".into()),
            follow_vscode: Some(false),
            python: Some(".venv/bin/python".into()),
            eslint: Some(false),
        };
        assert_eq!(Prefs::from_json(Some(&all.to_json())), all);
        let text = serde_json::to_string(&all.to_json()).unwrap();
        assert_eq!(Prefs::from_json(serde_json::from_str::<Value>(&text).ok().as_ref()), all);
    }

    #[test]
    fn odd_values_in_a_file_are_ignored() {
        let value = json!({ "tab_width": 400, "new_file_preset": "no-such", "env_file": "  ", "search_exclude": ["", "a/"] });
        let read = Prefs::from_json(Some(&value));
        assert_eq!(read, Prefs { search_exclude: vec!["a/".into()], ..Prefs::default() });
    }

    #[test]
    fn what_a_project_says_beats_what_is_under_it_and_the_rest_shows_through() {
        let own = Prefs { tab_width: Some(3), search_exclude: vec!["a/".into()], formatters: BTreeMap::from([("Python".to_string(), "Black".to_string())]), ..Prefs::default() };
        let vscode = Prefs { tab_width: Some(8), spaces: Some(true), search_exclude: vec!["b/".into()], formatters: BTreeMap::from([("Python".to_string(), "Ruff".to_string()), ("CSS".to_string(), "Prettier".to_string())]), ..Prefs::default() };
        let both = own.over(&vscode);
        assert_eq!((both.tab_width, both.spaces), (Some(3), Some(true)));
        assert_eq!(both.search_exclude, vec!["a/".to_string()]);
        assert_eq!(both.formatters.get("Python").map(String::as_str), Some("Black"));
        assert_eq!(both.formatters.get("CSS").map(String::as_str), Some("Prettier"));
        assert_eq!(Prefs::default().over(&vscode), vscode);
    }

    #[test]
    fn the_env_file_is_the_one_named_or_the_folder_s_own() {
        let mut prefs = Prefs::default();
        assert_eq!(prefs.env_path(Path::new("/p")), Path::new("/p/.env"));
        prefs.env_file = Some("conf/dev.env".into());
        assert_eq!(prefs.env_path(Path::new("/p")), Path::new("/p/conf/dev.env"));
    }
}
