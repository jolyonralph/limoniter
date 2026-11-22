//! Packages that have newer versions, from each package manager a project uses: npm, pip (in the project's own
//! environment when it has one), Composer and R. Each is asked for what's out of date as JSON (R, as a table of
//! comma-separated values), and the answers are gathered into one list, with the version installed, the newest
//! version the project's own limits allow ("wanted") and the newest there is. Updating is done by a task.

use crate::test_runners::{project_program, quote};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manager {
    Npm,
    Pip,
    Composer,
    R,
}

impl Manager {
    pub fn label(self) -> &'static str {
        match self {
            Manager::Npm => "npm",
            Manager::Pip => "pip",
            Manager::Composer => "Composer",
            Manager::R => "R",
        }
    }
}

/// One package that has a newer version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outdated {
    pub package: String,
    pub manager: Manager,
    pub now: String,
    /// The newest version the project's limits allow, where the manager says (empty where it doesn't).
    pub wanted: String,
    pub newest: String,
}

/// The managers a folder's files say it uses.
pub fn detect(folder: &Path) -> Vec<Manager> {
    let has = |f: &str| folder.join(f).is_file();
    let mut found = Vec::new();
    if has("package.json") {
        found.push(Manager::Npm);
    }
    if has("requirements.txt") || has("pyproject.toml") || has("setup.py") || folder.join(".venv").is_dir() || folder.join("venv").is_dir() {
        found.push(Manager::Pip);
    }
    if has("composer.json") {
        found.push(Manager::Composer);
    }
    if has("DESCRIPTION") || has("renv.lock") {
        found.push(Manager::R);
    }
    found
}

fn python(folder: &Path) -> String {
    project_program(folder, "python").map_or_else(|| (if cfg!(windows) { "python" } else { "python3" }).to_string(), |p| quote(&p.display().to_string()))
}

const CRAN: &str = "https://cloud.r-project.org";

/// The command that lists a manager's outdated packages.
pub fn command(manager: Manager, folder: &Path) -> String {
    match manager {
        Manager::Npm => "npm outdated --json".to_string(),
        Manager::Pip => format!("{} -m pip list --outdated --format=json", python(folder)),
        Manager::Composer => "composer outdated --format=json".to_string(),
        Manager::R => format!("Rscript -e \"o <- old.packages(repos = '{CRAN}'); if (!is.null(o)) write.csv(o[, c('Package', 'Installed', 'ReposVer')], stdout(), row.names = FALSE)\""),
    }
}

/// The outdated packages in what a manager printed. Anything that isn't the list (npm's warnings, say) is passed
/// over, so a manager that printed nothing useful gives an empty list.
pub fn parse(manager: Manager, output: &str) -> Vec<Outdated> {
    let make = |package: &str, now: &str, wanted: &str, newest: &str| Outdated { package: package.into(), manager, now: now.into(), wanted: wanted.into(), newest: newest.into() };
    // JSON starts at the first brace or bracket: managers may print notices first.
    let json_from = |open: char| output.find(open).and_then(|at| serde_json::from_str::<serde_json::Value>(&output[at..]).ok());
    let text = |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string();
    match manager {
        Manager::Npm => json_from('{')
            .and_then(|v| v.as_object().cloned())
            .map(|packages| packages.iter().map(|(name, v)| make(name, &text(v, "current"), &text(v, "wanted"), &text(v, "latest"))).collect())
            .unwrap_or_default(),
        Manager::Pip => json_from('[')
            .and_then(|v| v.as_array().cloned())
            .map(|packages| packages.iter().map(|v| make(&text(v, "name"), &text(v, "version"), "", &text(v, "latest_version"))).collect())
            .unwrap_or_default(),
        Manager::Composer => json_from('{')
            .and_then(|v| v.get("installed").and_then(|i| i.as_array()).cloned())
            .map(|packages| {
                packages
                    .iter()
                    .map(|v| {
                        let newest = text(v, "latest");
                        // "semver-safe-update" says the newest is within the limits composer.json sets.
                        let wanted = if text(v, "latest-status") == "semver-safe-update" { newest.clone() } else { String::new() };
                        make(&text(v, "name"), &text(v, "version"), &wanted, &newest)
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Manager::R => output
            .lines()
            .skip_while(|l| !l.starts_with("\"Package\""))
            .skip(1)
            .filter_map(|l| {
                let parts: Vec<&str> = l.split(',').map(|p| p.trim().trim_matches('"')).collect();
                (parts.len() == 3).then(|| make(parts[0], parts[1], "", parts[2]))
            })
            .collect(),
    }
}

/// The command that updates some of a manager's packages, or all of them when none are named.
pub fn update_command(manager: Manager, folder: &Path, packages: &[&Outdated]) -> String {
    let names: Vec<String> = packages.iter().map(|p| quote(&p.package)).collect();
    match manager {
        Manager::Npm if packages.is_empty() => "npm update".to_string(),
        // Within the limits where there's room to, and otherwise to the newest, as npm update would do nothing.
        Manager::Npm => {
            let (within, beyond): (Vec<&&Outdated>, Vec<&&Outdated>) = packages.iter().partition(|p| !p.wanted.is_empty() && p.wanted != p.now);
            let mut parts = Vec::new();
            if !within.is_empty() {
                parts.push(format!("npm update {}", within.iter().map(|p| quote(&p.package)).collect::<Vec<_>>().join(" ")));
            }
            if !beyond.is_empty() {
                parts.push(format!("npm install {}", beyond.iter().map(|p| quote(&format!("{}@latest", p.package))).collect::<Vec<_>>().join(" ")));
            }
            parts.join(" && ")
        }
        Manager::Pip => format!("{} -m pip install --upgrade {}", python(folder), names.join(" ")),
        Manager::Composer => format!("composer update {}", names.join(" ")).trim().to_string(),
        Manager::R => format!("Rscript -e \"install.packages(c({}), repos = '{CRAN}')\"", packages.iter().map(|p| format!("'{}'", p.package)).collect::<Vec<_>>().join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_says_current_wanted_and_latest() {
        let out = "npm warn something\n{\n  \"lodash\": {\"current\": \"4.17.20\", \"wanted\": \"4.17.21\", \"latest\": \"4.17.21\", \"dependent\": \"app\"},\n  \"react\": {\"current\": \"17.0.2\", \"wanted\": \"17.0.2\", \"latest\": \"18.3.1\"}\n}\n";
        let found = parse(Manager::Npm, out);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].package.as_str(), found[0].now.as_str(), found[0].wanted.as_str(), found[0].newest.as_str()), ("lodash", "4.17.20", "4.17.21", "4.17.21"));
        assert_eq!(found[1].wanted, "17.0.2");
        assert!(parse(Manager::Npm, "").is_empty());
    }

    #[test]
    fn pip_composer_and_r_are_read() {
        let pip = parse(Manager::Pip, "[{\"name\": \"requests\", \"version\": \"2.25.0\", \"latest_version\": \"2.32.0\", \"latest_filetype\": \"wheel\"}]");
        assert_eq!(pip, [Outdated { package: "requests".into(), manager: Manager::Pip, now: "2.25.0".into(), wanted: String::new(), newest: "2.32.0".into() }]);
        let composer = parse(Manager::Composer, "{\"installed\": [{\"name\": \"monolog/monolog\", \"version\": \"2.0.0\", \"latest\": \"3.5.0\", \"latest-status\": \"update-possible\"}, {\"name\": \"psr/log\", \"version\": \"1.0.0\", \"latest\": \"1.1.4\", \"latest-status\": \"semver-safe-update\"}]}");
        assert_eq!(composer[0].wanted, "", "the newest is beyond composer.json's limits");
        assert_eq!(composer[1].wanted, "1.1.4");
        let r = parse(Manager::R, "Loading\n\"Package\",\"Installed\",\"ReposVer\"\n\"jsonlite\",\"1.8.0\",\"1.8.9\"\n");
        assert_eq!((r[0].package.as_str(), r[0].now.as_str(), r[0].newest.as_str()), ("jsonlite", "1.8.0", "1.8.9"));
    }

    #[test]
    fn updates_are_commands_for_each_manager() {
        let folder = Path::new("/nowhere-at-all");
        let a = Outdated { package: "lodash".into(), manager: Manager::Npm, now: "1".into(), wanted: "1.1".into(), newest: "2".into() };
        let b = Outdated { package: "react".into(), manager: Manager::Npm, now: "17".into(), wanted: "17".into(), newest: "18".into() };
        assert_eq!(update_command(Manager::Npm, folder, &[]), "npm update");
        assert_eq!(update_command(Manager::Npm, folder, &[&a, &b]), "npm update lodash && npm install react@latest");
        let c = Outdated { package: "monolog/monolog".into(), manager: Manager::Composer, ..a.clone() };
        assert_eq!(update_command(Manager::Composer, folder, &[&c]), "composer update monolog/monolog");
        assert_eq!(update_command(Manager::Composer, folder, &[]), "composer update");
        let r = Outdated { package: "jsonlite".into(), manager: Manager::R, ..a };
        assert!(update_command(Manager::R, folder, &[&r]).contains("install.packages(c('jsonlite')"));
    }

    #[test]
    fn managers_are_found_from_a_folder_s_files() {
        let folder = std::env::temp_dir().join(format!("limoniter-outdated-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        assert!(detect(&folder).is_empty());
        std::fs::write(folder.join("package.json"), "{}").unwrap();
        std::fs::write(folder.join("composer.json"), "{}").unwrap();
        std::fs::write(folder.join("requirements.txt"), "").unwrap();
        assert_eq!(detect(&folder), [Manager::Npm, Manager::Pip, Manager::Composer]);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
