//! Node projects: which package manager a project uses (read from its lock files), how a `package.json` script is
//! run with it, which projects serve a page while being worked on (Vite, Next, Create React App and the like), and
//! the address such a server prints when it's listening.

use std::path::{Path, PathBuf};

/// The package managers a project can use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl Manager {
    pub fn name(self) -> &'static str {
        match self {
            Manager::Npm => "npm",
            Manager::Pnpm => "pnpm",
            Manager::Yarn => "yarn",
            Manager::Bun => "bun",
        }
    }

    /// The command that runs one of `package.json`'s scripts.
    pub fn run_script(self, script: &str) -> String {
        format!("{} run {script}", self.name())
    }

    /// The command that fetches a project's packages.
    pub fn install(self) -> String {
        format!("{} install", self.name())
    }
}

/// The lock files that say which manager made the project's packages, the first found winning.
const LOCKS: &[(&str, Manager)] = &[("pnpm-lock.yaml", Manager::Pnpm), ("yarn.lock", Manager::Yarn), ("bun.lockb", Manager::Bun), ("bun.lock", Manager::Bun), ("package-lock.json", Manager::Npm), ("npm-shrinkwrap.json", Manager::Npm)];

/// The manager a project uses: from its lock file (in its folder, or in a folder above it, as a workspace has
/// one lock file for all its packages), else from `package.json`'s `packageManager` field, else npm.
pub fn manager_for(folder: &Path) -> Manager {
    let mut here = Some(folder);
    for _ in 0..6 {
        let Some(dir) = here else { break };
        if let Some((_, manager)) = LOCKS.iter().find(|(file, _)| dir.join(file).is_file()) {
            return *manager;
        }
        here = dir.parent();
    }
    package_json(folder)
        .and_then(|json| json.get("packageManager")?.as_str().map(str::to_string))
        .and_then(|field| [Manager::Pnpm, Manager::Yarn, Manager::Bun, Manager::Npm].into_iter().find(|m| field.starts_with(m.name())))
        .unwrap_or(Manager::Npm)
}

fn package_json(folder: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(folder.join("package.json")).ok()?).ok()
}

/// The folder of the nearest `package.json` at or above a file.
pub fn project_for(file: &Path) -> Option<PathBuf> {
    let mut folder = file.parent();
    while let Some(here) = folder {
        if here.join("package.json").is_file() {
            return Some(here.to_path_buf());
        }
        folder = here.parent();
    }
    None
}

/// A project's scripts, in the order `package.json` has them.
pub fn scripts(folder: &Path) -> Vec<String> {
    package_json(folder).and_then(|json| Some(json.get("scripts")?.as_object()?.keys().cloned().collect())).unwrap_or_default()
}

/// The script of `package.json` that a task's command runs (`npm run dev` is `dev`), if it is one.
pub fn script_of(folder: &Path, command: &str) -> Option<String> {
    let manager = manager_for(folder);
    scripts(folder).into_iter().find(|name| manager.run_script(name) == command)
}

/// The lines of a `package.json` text on which its scripts are named, with their names: the keys inside its
/// "scripts" object. Read as a person writes it, one script a line, with strings and nesting understood.
pub fn script_lines(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let (mut depth, mut line) = (0usize, 0usize);
    // The depth of the "scripts" object's braces once it's been entered.
    let mut inside: Option<usize> = None;
    let mut want_scripts = false;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\n' => line += 1,
            '{' => {
                depth += 1;
                if want_scripts && depth == 2 {
                    inside = Some(depth);
                }
                want_scripts = false;
            }
            '}' => {
                if inside == Some(depth) {
                    inside = None;
                }
                depth = depth.saturating_sub(1);
            }
            '"' => {
                let start_line = line;
                let mut word = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    word.push(chars[i]);
                    i += 1;
                }
                // A key is a string with a colon after it.
                let mut after = i + 1;
                while after < chars.len() && chars[after].is_whitespace() {
                    after += 1;
                }
                if chars.get(after) == Some(&':') {
                    if depth == 1 && word == "scripts" {
                        want_scripts = true;
                    } else if inside == Some(depth) && depth == 2 {
                        found.push((start_line, word));
                    }
                } else if depth != 1 {
                    want_scripts = false;
                }
            }
            c if !c.is_whitespace() && c != ':' => want_scripts = false,
            _ => {}
        }
        i += 1;
    }
    found
}

/// The script that starts the project's server while it's worked on: `dev`, else `start`, else `serve`.
pub fn server_script(folder: &Path) -> Option<String> {
    let all = scripts(folder);
    ["dev", "start", "serve"].into_iter().find(|name| all.iter().any(|s| s == name)).map(str::to_string)
}

/// The packages that put up a page to look at while the code is being written.
const SERVERS: &[&str] = &["vite", "next", "react-scripts", "@vue/cli-service", "nuxt", "@sveltejs/kit", "astro", "parcel", "webpack-dev-server", "@angular/cli", "@remix-run/dev", "gatsby", "@docusaurus/core", "snowpack", "svelte"];

/// Whether the project is one of those: a front-end whose script serves a page, not a program to run.
pub fn serves_a_page(folder: &Path) -> bool {
    let Some(json) = package_json(folder) else { return false };
    ["dependencies", "devDependencies"].iter().any(|kind| json.get(kind).and_then(|d| d.as_object()).is_some_and(|d| SERVERS.iter().any(|name| d.contains_key(*name))))
}

/// The address in a line a dev server printed, once it says it's listening ("Local: http://localhost:5173/"). Only
/// addresses on this computer count, not the network ones also printed.
pub fn listening_url(line: &str) -> Option<String> {
    let clean = without_escapes(line);
    let mut from = 0;
    while let Some(at) = clean[from..].find("http") {
        let start = from + at;
        from = start + 4;
        let rest = &clean[start..];
        let Some(after) = rest.strip_prefix("http://").or_else(|| rest.strip_prefix("https://")) else { continue };
        let scheme = &rest[..rest.len() - after.len()];
        let end = after.find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | '>' | ',' | '\u{1b}')).unwrap_or(after.len());
        let address = after[..end].trim_end_matches(['.', ';']);
        let host_end = address.find(['/', '?', '#']).unwrap_or(address.len());
        let (host_port, path) = address.split_at(host_end);
        let (host, port) = match host_port.rsplit_once(':') {
            Some((h, p)) if !h.ends_with(':') && p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => (h, Some(p)),
            _ => (host_port, None),
        };
        let host = match host {
            "localhost" | "127.0.0.1" | "[::1]" => host,
            "0.0.0.0" | "[::]" => "localhost",
            _ => continue,
        };
        return Some(match port {
            Some(port) => format!("{scheme}{host}:{port}{path}"),
            None => format!("{scheme}{host}{path}"),
        });
    }
    None
}

/// A line without the colour and style codes terminals understand, which dev servers put in their messages.
fn without_escapes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(n) = chars.next_if(|n| !n.is_ascii_alphabetic()) {
                    let _ = n;
                }
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// What to do with F9 on a file in a Node project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunPlan {
    /// Start the project's server with this script (in this folder), and open the browser when it's listening.
    Server(PathBuf, String),
    /// Run the file with Node.
    Script,
    /// Nothing here is Node's to run.
    Nothing,
}

/// What F9 does for a file: a front-end project's server for its pages and components, or Node itself for a plain
/// script.
pub fn plan_for(file: &Path) -> RunPlan {
    let ext = file.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    let script_file = matches!(ext.as_str(), "js" | "mjs" | "cjs");
    let component = matches!(ext.as_str(), "jsx" | "tsx" | "vue" | "svelte" | "astro");
    let page_file = component || matches!(ext.as_str(), "ts" | "html" | "htm" | "css" | "scss");
    if !script_file && !page_file {
        return RunPlan::Nothing;
    }
    // A front-end's server shows any of its files; elsewhere, a component still needs one to be seen.
    if let Some(folder) = project_for(file)
        && let Some(script) = server_script(&folder)
        && (serves_a_page(&folder) || component)
    {
        return RunPlan::Server(folder, script);
    }
    if script_file { RunPlan::Script } else { RunPlan::Nothing }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("limoniter-node-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn the_manager_comes_from_the_lock_file() {
        for (lock, manager) in [("pnpm-lock.yaml", Manager::Pnpm), ("yarn.lock", Manager::Yarn), ("bun.lockb", Manager::Bun), ("bun.lock", Manager::Bun), ("package-lock.json", Manager::Npm)] {
            let folder = scratch("lock");
            std::fs::write(folder.join("package.json"), "{}").unwrap();
            std::fs::write(folder.join(lock), "").unwrap();
            assert_eq!(manager_for(&folder), manager, "{lock}");
            std::fs::remove_dir_all(&folder).unwrap();
        }
        let folder = scratch("none");
        std::fs::write(folder.join("package.json"), "{}").unwrap();
        assert_eq!(manager_for(&folder), Manager::Npm, "npm where nothing says");
        std::fs::write(folder.join("package.json"), r#"{"packageManager": "pnpm@9.1.0"}"#).unwrap();
        assert_eq!(manager_for(&folder), Manager::Pnpm, "the packageManager field");
        // A workspace's package uses the lock file of the folder above it.
        let inner = folder.join("packages").join("app");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("package.json"), "{}").unwrap();
        std::fs::write(folder.join("yarn.lock"), "").unwrap();
        assert_eq!(manager_for(&inner), Manager::Yarn);
        assert_eq!(Manager::Yarn.run_script("dev"), "yarn run dev");
        assert_eq!(Manager::Bun.install(), "bun install");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_server_script_and_the_kind_of_project() {
        let folder = scratch("vite");
        std::fs::write(folder.join("package.json"), r#"{"scripts": {"build": "vite build", "start": "node .", "dev": "vite"}, "devDependencies": {"vite": "^5"}}"#).unwrap();
        let mut all = scripts(&folder);
        all.sort();
        assert_eq!(all, ["build", "dev", "start"]);
        assert_eq!(server_script(&folder).as_deref(), Some("dev"), "dev before start");
        assert!(serves_a_page(&folder));
        std::fs::write(folder.join("package.json"), r#"{"scripts": {"start": "node server.js"}, "dependencies": {"express": "4"}}"#).unwrap();
        assert_eq!(server_script(&folder).as_deref(), Some("start"));
        assert!(!serves_a_page(&folder));
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_address_is_read_from_what_a_server_prints() {
        assert_eq!(listening_url("  \u{1b}[32m➜\u{1b}[39m  \u{1b}[1mLocal\u{1b}[22m:   \u{1b}[36mhttp://localhost:\u{1b}[1m5173\u{1b}[22m/\u{1b}[39m").as_deref(), Some("http://localhost:5173/"));
        assert_eq!(listening_url("  - Local:        http://localhost:3000").as_deref(), Some("http://localhost:3000"));
        assert_eq!(listening_url("Local:            http://localhost:3000").as_deref(), Some("http://localhost:3000"));
        assert_eq!(listening_url("Project is running at: http://0.0.0.0:8080/").as_deref(), Some("http://localhost:8080/"));
        assert_eq!(listening_url("Server listening on https://127.0.0.1:8443/app.").as_deref(), Some("https://127.0.0.1:8443/app"));
        assert_eq!(listening_url("  On Your Network:  http://192.168.1.20:3000"), None, "the network's address isn't the one wanted");
        assert_eq!(listening_url("Download from http://example.com/node.tar.gz"), None);
        assert_eq!(listening_url("ready in 312 ms"), None);
        assert_eq!(listening_url("  ➜  Network: http://192.168.1.5:5173/ and Local: http://localhost:5173/").as_deref(), Some("http://localhost:5173/"));
    }

    #[test]
    fn f9_serves_a_front_end_and_runs_a_script() {
        let folder = scratch("plan");
        std::fs::write(folder.join("package.json"), r#"{"scripts": {"dev": "vite"}, "devDependencies": {"vite": "5"}}"#).unwrap();
        assert_eq!(plan_for(&folder.join("src").join("App.tsx")), RunPlan::Server(folder.clone(), "dev".into()));
        assert_eq!(plan_for(&folder.join("main.js")), RunPlan::Server(folder.clone(), "dev".into()), "a front-end's script too");
        assert_eq!(plan_for(&folder.join("notes.txt")), RunPlan::Nothing);
        std::fs::write(folder.join("package.json"), r#"{"scripts": {"start": "node server.js"}, "dependencies": {"express": "4"}}"#).unwrap();
        assert_eq!(plan_for(&folder.join("server.js")), RunPlan::Script, "a plain program is run");
        assert_eq!(plan_for(&folder.join("App.jsx")), RunPlan::Server(folder.clone(), "start".into()));
        assert_eq!(plan_for(&folder.join("lib.ts")), RunPlan::Nothing);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_lines_scripts_are_named_on_are_found() {
        let text = "{\n  \"name\": \"x\",\n  \"scripts\": {\n    \"dev\": \"vite {a}\",\n    \"build\": \"tsc\"\n  },\n  \"dependencies\": {\n    \"dev\": \"1\"\n  }\n}\n";
        assert_eq!(script_lines(text), [(3, "dev".to_string()), (4, "build".to_string())]);
        assert!(script_lines("{\"scripts\": 3, \"other\": {\"a\": 1}}").is_empty());
        assert_eq!(script_lines("{\"scripts\": {\"a\": \"b\", \"c:d\": \"e\"}}"), [(0, "a".to_string()), (0, "c:d".to_string())]);
        assert!(script_lines("not json at all").is_empty());
    }

    #[test]
    fn a_task_command_is_traced_to_its_script() {
        let folder = std::env::temp_dir().join(format!("limoniter-script-of-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("package.json"), r#"{"scripts": {"dev": "vite", "test": "jest"}}"#).unwrap();
        std::fs::write(folder.join("yarn.lock"), "").unwrap();
        assert_eq!(script_of(&folder, "yarn run dev").as_deref(), Some("dev"));
        assert_eq!(script_of(&folder, "npm run dev"), None, "another manager's");
        assert_eq!(script_of(&folder, "yarn install"), None);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
