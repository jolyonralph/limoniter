//! Python environments: finding a project's own (a virtual environment such as `.venv` or `venv`, uv's, Poetry's,
//! conda's), making one, and installing a project's packages into it. The one chosen is the Python that F9, the
//! debugger, pytest, the language server and the Jupyter kernel use, so they all see the same packages.
//!
//! Nothing here draws. Finding is quick (it reads folders), except for asking Poetry where its environment is,
//! which runs `poetry env info -p` and so is done away from the window.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What made an environment, as the person would name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `python -m venv`, or virtualenv.
    Venv,
    /// `uv venv`, or uv's project environment.
    Uv,
    Poetry,
    Conda,
    /// The one already switched on where Limoniter was started (`VIRTUAL_ENV`).
    Active,
    /// The computer's own Python, on the PATH.
    System,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Venv => "venv",
            Kind::Uv => "uv",
            Kind::Poetry => "Poetry",
            Kind::Conda => "conda",
            Kind::Active => "Already active",
            Kind::System => "The computer's own",
        }
    }
}

/// One Python to use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Env {
    pub kind: Kind,
    /// Short, for the status bar: the folder's name (".venv") or the conda environment's.
    pub name: String,
    /// The environment's own folder (its prefix); for the system's Python, empty.
    pub prefix: PathBuf,
    /// The program itself.
    pub python: PathBuf,
}

impl Env {
    /// The system's Python, found on the PATH as `python3` (or `py` on Windows).
    pub fn system() -> Env {
        let program = if cfg!(windows) { "py" } else { "python3" };
        Env { kind: Kind::System, name: program.to_string(), prefix: PathBuf::new(), python: PathBuf::from(program) }
    }

    /// What the status bar shows: "Python (.venv)", or "Python (python3)" for the computer's own.
    pub fn status_words(&self) -> String {
        crate::i18n::tf!("Python ({})", self.name)
    }

    /// The Python as a command line wants it: quoted when it's a path.
    pub fn command(&self) -> String {
        if self.kind == Kind::System { self.python.display().to_string() } else { quoted(&self.python.display().to_string()) }
    }
}

/// A path as the shell wants it, in quotes, as `php_host` quotes them.
pub fn quoted(text: &str) -> String {
    if cfg!(windows) { format!("\"{text}\"") } else { format!("'{}'", text.replace('\'', "'\\''")) }
}

/// The Python inside an environment's folder, if it's there.
pub fn python_in(prefix: &Path) -> Option<PathBuf> {
    let candidates: &[&str] = if cfg!(windows) { &["Scripts/python.exe", "python.exe"] } else { &["bin/python", "bin/python3"] };
    candidates.iter().map(|c| prefix.join(c)).find(|p| p.exists())
}

/// Whether a folder is a virtual environment: it has the `pyvenv.cfg` that venv, virtualenv and uv all write.
fn is_venv(folder: &Path) -> bool {
    folder.join("pyvenv.cfg").is_file()
}

/// The folder a file's environments are looked for in: the nearest above it with an environment or a project's
/// own Python file, or else the folder its tasks run in.
pub fn project_folder(file: &Path) -> PathBuf {
    let marks = ["pyproject.toml", "requirements.txt", "environment.yml", "Pipfile", "setup.py", "poetry.lock", "uv.lock"];
    let mut folder = file.parent();
    while let Some(here) = folder {
        if marks.iter().any(|m| here.join(m).is_file()) || [".venv", "venv"].iter().any(|v| is_venv(&here.join(v))) {
            return here.to_path_buf();
        }
        folder = here.parent();
    }
    crate::lsp::root_for(file)
}

/// The environments a project's folder has, its own first, then any already active, then the system's. `poetry`
/// is where Poetry said its environment is, if it was asked (see `poetry_env`).
pub fn find(folder: &Path, poetry: Option<&Path>) -> Vec<Env> {
    let mut found: Vec<Env> = Vec::new();
    let add = |env: Env, found: &mut Vec<Env>| {
        if !found.iter().any(|e| same(&e.prefix, &env.prefix) && e.kind != Kind::System) {
            found.push(env);
        }
    };
    let uv_project = folder.join("uv.lock").is_file();
    // Every folder at the top of the project that's a virtual environment, .venv and venv first.
    let mut folders: Vec<PathBuf> = std::fs::read_dir(folder).map(|entries| entries.flatten().map(|e| e.path()).filter(|p| p.is_dir() && is_venv(p)).collect()).unwrap_or_default();
    folders.sort_by_key(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        (name != ".venv", name != "venv", name)
    });
    for prefix in folders {
        let Some(python) = python_in(&prefix) else { continue };
        let config = std::fs::read_to_string(prefix.join("pyvenv.cfg")).unwrap_or_default();
        // uv writes "uv = 0.4.0" into the environments it makes.
        let kind = if config.lines().any(|l| l.trim_start().starts_with("uv =")) || (uv_project && prefix.ends_with(".venv")) { Kind::Uv } else { Kind::Venv };
        let name = prefix.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        add(Env { kind, name, prefix, python }, &mut found);
    }
    if let Some(prefix) = poetry
        && let Some(python) = python_in(prefix)
    {
        let name = prefix.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        add(Env { kind: Kind::Poetry, name, prefix: prefix.to_path_buf(), python }, &mut found);
    }
    // Conda's: the one environment.yml names, wherever conda keeps its environments, and the one switched on.
    if let Some(name) = std::fs::read_to_string(folder.join("environment.yml")).ok().and_then(|t| conda_name(&t))
        && let Some(prefix) = conda_roots().into_iter().map(|root| root.join("envs").join(&name)).find(|p| p.is_dir())
        && let Some(python) = conda_python(&prefix)
    {
        add(Env { kind: Kind::Conda, name, prefix, python }, &mut found);
    }
    if let Some(prefix) = std::env::var_os("CONDA_PREFIX").map(PathBuf::from)
        && let Some(python) = conda_python(&prefix)
    {
        let name = std::env::var("CONDA_DEFAULT_ENV").ok().filter(|n| !n.is_empty()).unwrap_or_else(|| prefix.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
        add(Env { kind: Kind::Conda, name, prefix, python }, &mut found);
    }
    if let Some(prefix) = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from)
        && let Some(python) = python_in(&prefix)
    {
        let name = prefix.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        add(Env { kind: Kind::Active, name, prefix, python }, &mut found);
    }
    found.push(Env::system());
    found
}

fn same(a: &Path, b: &Path) -> bool {
    a == b || (a.canonicalize().ok().is_some() && a.canonicalize().ok() == b.canonicalize().ok())
}

/// A conda environment's Python: in `bin` on Linux and the Mac, at the top on Windows.
fn conda_python(prefix: &Path) -> Option<PathBuf> {
    let python = if cfg!(windows) { prefix.join("python.exe") } else { prefix.join("bin/python") };
    python.exists().then_some(python)
}

/// The environment's name in a conda environment.yml (`name: data-work`).
pub fn conda_name(text: &str) -> Option<String> {
    text.lines().find_map(|line| line.strip_prefix("name:")).map(|n| n.trim().trim_matches(['"', '\'']).to_string()).filter(|n| !n.is_empty())
}

/// Where conda keeps its environments: beside the conda that's on the PATH or named by CONDA_EXE, and the usual
/// places Miniconda, Anaconda and Miniforge are put.
fn conda_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(exe) = std::env::var_os("CONDA_EXE").map(PathBuf::from)
        && let Some(root) = exe.parent().and_then(Path::parent)
    {
        roots.push(root.to_path_buf());
    }
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from) {
        for name in ["miniconda3", "anaconda3", "miniforge3", "mambaforge", ".conda"] {
            roots.push(home.join(name));
        }
    }
    roots
}

/// Whether the project is Poetry's: a poetry.lock, or `[tool.poetry]` in pyproject.toml.
pub fn uses_poetry(folder: &Path) -> bool {
    folder.join("poetry.lock").is_file() || std::fs::read_to_string(folder.join("pyproject.toml")).is_ok_and(|t| t.contains("[tool.poetry"))
}

/// Asks Poetry where the project's environment is (`poetry env info -p`). Slow: run it away from the window.
pub fn poetry_env(folder: &Path) -> Option<PathBuf> {
    if !uses_poetry(folder) || !crate::lsp::on_path("poetry") {
        return None;
    }
    let out = crate::platform::command("poetry").args(["env", "info", "-p"]).current_dir(folder).stdin(std::process::Stdio::null()).output().ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

/// The command that makes a new environment in the folder, as `.venv`: uv's when uv is there (it's much quicker),
/// otherwise Python's own venv.
pub fn make_command(uv: bool) -> String {
    if uv {
        "uv venv .venv".to_string()
    } else {
        format!("{} -m venv .venv", Env::system().python.display())
    }
}

/// The commands that install a project's packages into an environment, with what each is for: from
/// requirements.txt, from pyproject.toml (the project itself, with what it needs), and conda's environment.yml.
/// uv's environments have no pip of their own, so uv installs into them.
pub fn install_commands(folder: &Path, env: &Env, uv: bool) -> Vec<(String, String)> {
    let mut commands = Vec::new();
    let python = env.command();
    let pip = |what: &str| if env.kind == Kind::Uv && uv { format!("uv pip install --python {python} {what}") } else { format!("{python} -m pip install {what}") };
    if folder.join("requirements.txt").is_file() {
        commands.push((crate::i18n::tr("Install packages from requirements.txt").to_string(), pip("-r requirements.txt")));
    }
    if folder.join("requirements-dev.txt").is_file() {
        commands.push((crate::i18n::tr("Install packages from requirements-dev.txt").to_string(), pip("-r requirements-dev.txt")));
    }
    if folder.join("pyproject.toml").is_file() {
        let command = if env.kind == Kind::Poetry {
            "poetry install".to_string()
        } else if env.kind == Kind::Uv && uv && folder.join("uv.lock").is_file() {
            "uv sync".to_string()
        } else {
            pip("-e .")
        };
        commands.push((crate::i18n::tr("Install the project and its packages (pyproject.toml)").to_string(), command));
    }
    if folder.join("environment.yml").is_file() && env.kind == Kind::Conda {
        commands.push((crate::i18n::tr("Update the conda environment from environment.yml").to_string(), format!("conda env update --prefix {} -f environment.yml", quoted(&env.prefix.display().to_string()))));
    }
    commands
}

/// A task's command with its leading `python` (or `python3`) made the environment's own.
pub fn with_python(command: &str, env: &Env) -> String {
    for word in ["python3 ", "python "] {
        if let Some(rest) = command.strip_prefix(word) {
            return format!("{} {rest}", env.command());
        }
    }
    command.to_string()
}

/// What a Python language server is told of the environment, as `workspace/configuration` asks and
/// `workspace/didChangeConfiguration` sends: pyright and basedpyright read `python.pythonPath`, pylsp reads Jedi's
/// `environment`.
pub fn lsp_settings(env: &Env) -> serde_json::Value {
    let python = env.python.display().to_string();
    let mut settings = serde_json::json!({
        "python": { "pythonPath": python, "defaultInterpreterPath": python },
        "pylsp": { "plugins": { "jedi": { "environment": python } } }
    });
    if env.kind != Kind::System
        && let (Some(parent), Some(name)) = (env.prefix.parent(), env.prefix.file_name())
    {
        settings["python"]["venvPath"] = serde_json::Value::String(parent.display().to_string());
        settings["python"]["venv"] = serde_json::Value::String(name.to_string_lossy().to_string());
    }
    settings
}

/// Whether an environment's Python has a module, by importing it. Slow (it starts Python): not every frame.
pub fn has_module(python: &Path, module: &str) -> bool {
    crate::platform::command(python).args(["-c", &format!("import {module}")]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

/// The environment chosen for the files being worked on, for the parts of Limoniter that start Python without the
/// window's help (a notebook's kernel).
static CHOSEN: Mutex<Option<Env>> = Mutex::new(None);

pub fn set_chosen(env: Option<Env>) {
    if let Ok(mut chosen) = CHOSEN.lock() {
        *chosen = env;
    }
}

pub fn chosen() -> Option<Env> {
    CHOSEN.lock().ok().and_then(|c| c.clone())
}

/// A Python kernel made to run in an environment: ipykernel's launcher, started by the environment's Python, if it
/// has ipykernel. Otherwise the kernel as it was, and why, for the person. Other languages' kernels are left as they are.
pub fn kernel_in(spec: &crate::jupyter::link::KernelSpec, env: &Env, has_ipykernel: bool) -> (crate::jupyter::link::KernelSpec, Option<String>) {
    let mut spec = spec.clone();
    if !spec.language.eq_ignore_ascii_case("python") || env.kind == Kind::System || spec.argv.first().is_some_and(|p| Path::new(p) == env.python) {
        return (spec, None);
    }
    if !has_ipykernel {
        let install = format!("{} -m pip install ipykernel", env.command());
        return (spec, Some(crate::i18n::tf!("{} has no ipykernel, so the kernel runs on another Python. To use the environment's: {install}", env.name)));
    }
    spec.argv = vec![env.python.display().to_string(), "-m".into(), "ipykernel_launcher".into(), "-f".into(), "{connection_file}".into()];
    spec.display_name = crate::i18n::tf!("Python ({})", env.name);
    (spec, None)
}

/// The kernel a notebook is started on, in the chosen environment (see `kernel_in`).
pub fn kernel_for_chosen(spec: &crate::jupyter::link::KernelSpec) -> crate::jupyter::link::KernelSpec {
    match chosen() {
        Some(env) if spec.language.eq_ignore_ascii_case("python") && env.kind != Kind::System => {
            let has = has_module(&env.python, "ipykernel");
            kernel_in(spec, &env, has).0
        }
        _ => spec.clone(),
    }
}

/// The environments chosen for each project folder, kept in the settings file as `python_env=folder<TAB>python`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Chosen {
    pub by_folder: Vec<(PathBuf, PathBuf)>,
}

impl Chosen {
    pub fn from_settings(settings: &str) -> Self {
        let by_folder = settings.lines().filter_map(|l| l.strip_prefix("python_env=")).filter_map(|v| v.split_once('\t')).map(|(f, p)| (PathBuf::from(f), PathBuf::from(p))).collect();
        Chosen { by_folder }
    }

    pub fn to_settings(&self) -> String {
        self.by_folder.iter().map(|(f, p)| format!("python_env={}\t{}\n", f.display(), p.display())).collect()
    }

    pub fn get(&self, folder: &Path) -> Option<&Path> {
        self.by_folder.iter().find(|(f, _)| f == folder).map(|(_, p)| p.as_path())
    }

    /// Goes back to the environment found for the folder.
    pub fn clear(&mut self, folder: &Path) {
        self.by_folder.retain(|(f, _)| f != folder);
    }

    pub fn set(&mut self, folder: &Path, python: &Path) {
        self.by_folder.retain(|(f, _)| f != folder);
        self.by_folder.push((folder.to_path_buf(), python.to_path_buf()));
    }

    /// The environment to use from those found: the one chosen before, if it's still there, or else the project's
    /// own first one (the system's if it has none).
    #[cfg(test)]
    pub fn pick(&self, folder: &Path, found: &[Env]) -> Option<Env> {
        self.pick_for_project(folder, found, None)
    }

    /// As `pick`, with the Python the project names (its settings, or VS Code's `python.defaultInterpreterPath`),
    /// relative to the folder, coming after the person's own choice and before the folder's first environment.
    pub fn pick_for_project(&self, folder: &Path, found: &[Env], project_python: Option<&str>) -> Option<Env> {
        if let Some(python) = self.get(folder)
            && let Some(env) = found.iter().find(|e| e.python == python)
        {
            return Some(env.clone());
        }
        if let Some(named) = project_python.map(str::trim).filter(|p| !p.is_empty()) {
            let path = folder.join(named);
            if let Some(env) = found.iter().find(|e| e.python == path || e.python == Path::new(named)) {
                return Some(env.clone());
            }
            if path.is_file() {
                let prefix = path.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default();
                let name = prefix.file_name().map_or_else(|| named.to_string(), |n| n.to_string_lossy().into_owned());
                return Some(Env { kind: Kind::Venv, name, prefix, python: path });
            }
        }
        found.first().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_venv(prefix: &Path, config: &str) {
        let bin = prefix.join(if cfg!(windows) { "Scripts" } else { "bin" });
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(if cfg!(windows) { "python.exe" } else { "python" }), "").unwrap();
        std::fs::write(prefix.join("pyvenv.cfg"), config).unwrap();
    }

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("limoniter-pyenv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn a_projects_environments_are_found_venv_first() {
        let root = folder("find");
        fake_venv(&root.join("venv"), "home = /usr/bin\n");
        fake_venv(&root.join(".venv"), "home = /usr/bin\nuv = 0.4.18\n");
        fake_venv(&root.join("other-env"), "home = /usr/bin\n");
        std::fs::create_dir_all(root.join("not-an-env")).unwrap();
        let found = find(&root, None);
        let names: Vec<(&str, Kind)> = found.iter().map(|e| (e.name.as_str(), e.kind)).collect();
        assert_eq!(&names[..3], &[(".venv", Kind::Uv), ("venv", Kind::Venv), ("other-env", Kind::Venv)]);
        assert_eq!(found.last().unwrap().kind, Kind::System, "the computer's own comes last");
        assert!(found[0].python.starts_with(root.join(".venv")));
        assert_eq!(found[0].status_words(), "Python (.venv)");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn poetry_and_conda_environments_are_named() {
        let root = folder("poetry");
        let poetry = root.join("cache/myproj-AbC123-py3.12");
        fake_venv(&poetry, "home = /usr/bin\n");
        std::fs::write(root.join("pyproject.toml"), "[tool.poetry]\nname = \"x\"\n").unwrap();
        assert!(uses_poetry(&root));
        let found = find(&root, Some(&poetry));
        assert_eq!(found[0].kind, Kind::Poetry);
        assert_eq!(found[0].name, "myproj-AbC123-py3.12");
        assert_eq!(conda_name("name: data-work\nchannels:\n  - conda-forge\n"), Some("data-work".into()));
        assert_eq!(conda_name("channels: []\n"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_project_folder_is_the_one_with_the_environment() {
        let root = folder("project");
        fake_venv(&root.join(".venv"), "home = /usr/bin\n");
        std::fs::create_dir_all(root.join("src/pkg")).unwrap();
        assert_eq!(project_folder(&root.join("src/pkg/mod.py")), root);
    }

    #[test]
    fn commands_use_the_environments_python() {
        let root = folder("install");
        let env = Env { kind: Kind::Venv, name: ".venv".into(), prefix: root.join(".venv"), python: root.join(".venv/bin/python") };
        assert!(install_commands(&root, &env, false).is_empty(), "nothing to install from");
        std::fs::write(root.join("requirements.txt"), "requests\n").unwrap();
        std::fs::write(root.join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();
        let commands = install_commands(&root, &env, false);
        assert_eq!(commands.len(), 2);
        assert!(commands[0].1.ends_with("-m pip install -r requirements.txt") && commands[0].1.contains(".venv"));
        assert!(commands[1].1.ends_with("-m pip install -e ."));
        let uv = Env { kind: Kind::Uv, ..env.clone() };
        assert!(install_commands(&root, &uv, true)[0].1.starts_with("uv pip install --python "), "uv's environments have no pip");
        std::fs::write(root.join("uv.lock"), "").unwrap();
        assert_eq!(install_commands(&root, &uv, true)[1].1, "uv sync");
        assert!(with_python("python -m pytest -v", &env).ends_with("python' -m pytest -v") || cfg!(windows));
        assert_eq!(with_python("cargo test", &env), "cargo test");
        assert_eq!(make_command(true), "uv venv .venv");
        assert!(make_command(false).ends_with(" -m venv .venv"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn language_servers_are_told_the_python() {
        let env = Env { kind: Kind::Venv, name: ".venv".into(), prefix: PathBuf::from("/w/.venv"), python: PathBuf::from("/w/.venv/bin/python") };
        let settings = lsp_settings(&env);
        assert_eq!(settings.pointer("/python/pythonPath").and_then(|v| v.as_str()), Some("/w/.venv/bin/python"));
        assert_eq!(settings.pointer("/python/venv").and_then(|v| v.as_str()), Some(".venv"));
        assert_eq!(settings.pointer("/pylsp/plugins/jedi/environment").and_then(|v| v.as_str()), Some("/w/.venv/bin/python"));
    }

    #[test]
    fn a_python_kernel_runs_in_the_environment() {
        let spec = crate::jupyter::link::KernelSpec { name: "python3".into(), display_name: "Python 3".into(), language: "python".into(), argv: vec!["/usr/bin/python3".into(), "-m".into(), "ipykernel_launcher".into(), "-f".into(), "{connection_file}".into()], env: Vec::new(), dir: PathBuf::new() };
        let env = Env { kind: Kind::Venv, name: ".venv".into(), prefix: PathBuf::from("/w/.venv"), python: PathBuf::from("/w/.venv/bin/python") };
        let (made, note) = kernel_in(&spec, &env, true);
        assert_eq!(made.argv[0], "/w/.venv/bin/python");
        assert!(made.argv.contains(&"{connection_file}".to_string()) && note.is_none());
        let (kept, note) = kernel_in(&spec, &env, false);
        assert_eq!(kept.argv[0], "/usr/bin/python3", "without ipykernel, the kernel's own Python");
        assert!(note.is_some_and(|n| n.contains("pip install ipykernel")));
        let r = crate::jupyter::link::KernelSpec { language: "R".into(), ..spec.clone() };
        assert_eq!(kernel_in(&r, &env, true).0, r, "R's kernel is left alone");
    }

    #[test]
    fn the_choice_is_kept_for_each_folder() {
        let mut chosen = Chosen::default();
        chosen.set(Path::new("/w/a"), Path::new("/w/a/venv/bin/python"));
        chosen.set(Path::new("/w/b"), Path::new("/usr/bin/python3"));
        chosen.set(Path::new("/w/a"), Path::new("/w/a/.venv/bin/python"));
        let back = Chosen::from_settings(&chosen.to_settings());
        assert_eq!(back, chosen);
        assert_eq!(back.get(Path::new("/w/a")), Some(Path::new("/w/a/.venv/bin/python")));
        let found = vec![Env { kind: Kind::Venv, name: "venv".into(), prefix: "/w/a/venv".into(), python: "/w/a/venv/bin/python".into() }, Env { kind: Kind::Venv, name: ".venv".into(), prefix: "/w/a/.venv".into(), python: "/w/a/.venv/bin/python".into() }];
        assert_eq!(back.pick(Path::new("/w/a"), &found).unwrap().name, ".venv");
        assert_eq!(back.pick(Path::new("/w/c"), &found).unwrap().name, "venv", "the first, when none was chosen");
        // The project's own Python (or VS Code's) comes after the person's choice, before the first one found.
        let c_found: Vec<Env> = found.iter().map(|e| Env { python: Path::new("/w/c").join(e.python.strip_prefix("/w/a").unwrap()), ..e.clone() }).collect();
        assert_eq!(back.pick_for_project(Path::new("/w/c"), &c_found, Some(".venv/bin/python")).unwrap().name, ".venv");
        assert_eq!(back.pick_for_project(Path::new("/w/a"), &found, Some("venv/bin/python")).unwrap().name, ".venv", "the person's choice wins");
        assert_eq!(back.pick_for_project(Path::new("/w/c"), &c_found, Some("")).unwrap().name, "venv");
    }
}
