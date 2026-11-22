//! Finding the kernels Jupyter knows of, in the folders jupyter_client looks in and in the same order, and the
//! command that installs one when there's none.

use super::link::KernelSpec;
use std::path::{Path, PathBuf};

/// What a Python on this computer says of itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Python {
    /// The program itself (`sys.executable`).
    pub program: PathBuf,
    /// `sys.prefix`, whose `share/jupyter` holds the kernels installed with it.
    pub prefix: PathBuf,
    pub has_ipykernel: bool,
    /// The system looks after its packages (PEP 668), so `pip install --user` is refused.
    pub externally_managed: bool,
}

/// What's asked of a Python to learn the above, one answer to a line.
const PROBE: &str = "import sys, os, sysconfig, importlib.util\nprint(sys.executable)\nprint(sys.prefix)\nprint(importlib.util.find_spec('ipykernel') is not None)\nprint(os.path.exists(os.path.join(sysconfig.get_path('stdlib'), 'EXTERNALLY-MANAGED')))";

/// Asks a Python program about itself; None if it isn't there or isn't Python.
pub fn probe_python(program: &Path) -> Option<Python> {
    let mut command = crate::platform::command(program);
    command.args(["-c", PROBE]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let output = command.output().ok().filter(|o| o.status.success())?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines().map(str::trim);
    Some(Python {
        program: PathBuf::from(lines.next()?),
        prefix: PathBuf::from(lines.next()?),
        has_ipykernel: lines.next()? == "True",
        externally_managed: lines.next()? == "True",
    })
}

/// A program on the PATH, by its full path.
fn find_on_path(program: &str) -> Option<PathBuf> {
    let endings: &[&str] = if cfg!(windows) { &[".exe", ""] } else { &[""] };
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).find_map(|dir| endings.iter().map(|e| dir.join(format!("{program}{e}"))).find(|p| p.is_file()))
}

/// The Python a kernel would most likely be installed with: an active virtual environment's or conda's, else
/// `python3` or `python` on the PATH. On Windows the stand-ins in WindowsApps, which only offer to install
/// Python from the Store, are passed over.
pub fn usual_python() -> Option<Python> {
    let in_prefix = |prefix: PathBuf| {
        let program = if cfg!(windows) { prefix.join("python.exe") } else { prefix.join("bin").join("python") };
        let program = if !program.is_file() && cfg!(windows) { prefix.join("Scripts").join("python.exe") } else { program };
        program.is_file().then_some(program)
    };
    let prefixes = ["VIRTUAL_ENV", "CONDA_PREFIX"].into_iter().filter_map(std::env::var_os).map(PathBuf::from);
    let on_path = ["python3", "python"].into_iter().filter_map(find_on_path).filter(|p| !p.to_string_lossy().contains("WindowsApps"));
    prefixes.filter_map(in_prefix).chain(on_path).find_map(|program| probe_python(&program))
}

/// The folders kernels are looked for in, first first, as jupyter_core's `jupyter_path("kernels")` lists them:
/// `JUPYTER_PATH`, the user's data folder, the `share/jupyter` of each Python prefix given, and the system's.
/// `env` reads an environment variable, so the tests can give their own.
pub fn kernel_folders(env: &dyn Fn(&str) -> Option<String>, prefixes: &[PathBuf]) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    if let Some(paths) = env("JUPYTER_PATH") {
        folders.extend(std::env::split_paths(&paths).filter(|p| !p.as_os_str().is_empty()));
    }
    let home = env("HOME").or_else(|| env("USERPROFILE")).map(PathBuf::from);
    let user = if let Some(data) = env("JUPYTER_DATA_DIR") {
        Some(PathBuf::from(data))
    } else if cfg!(windows) {
        env("APPDATA").map(|a| PathBuf::from(a).join("jupyter"))
    } else if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library").join("Jupyter"))
    } else {
        env("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| home.map(|h| h.join(".local").join("share"))).map(|d| d.join("jupyter"))
    };
    folders.extend(user);
    folders.extend(prefixes.iter().map(|p| p.join("share").join("jupyter")));
    if cfg!(windows) {
        folders.extend(env("PROGRAMDATA").map(|p| PathBuf::from(p).join("jupyter")));
    } else {
        folders.push(PathBuf::from("/usr/local/share/jupyter"));
        folders.push(PathBuf::from("/usr/share/jupyter"));
    }
    let mut seen = Vec::new();
    folders.retain(|f| {
        let new = !seen.contains(f);
        seen.push(f.clone());
        new
    });
    folders.into_iter().map(|f| f.join("kernels")).collect()
}

/// One kernel's `kernel.json`, read. The folder's name, in lower case, is the kernel's name, as jupyter_client
/// has it.
pub fn read_spec(dir: &Path) -> Option<KernelSpec> {
    let text = std::fs::read_to_string(dir.join("kernel.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let argv: Vec<String> = json["argv"].as_array()?.iter().map(|a| a.as_str().map(str::to_string)).collect::<Option<_>>()?;
    if argv.is_empty() {
        return None;
    }
    let name = dir.file_name()?.to_string_lossy().to_lowercase();
    let env = json["env"].as_object().map(|e| e.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
    Some(KernelSpec {
        display_name: json["display_name"].as_str().filter(|d| !d.is_empty()).unwrap_or(&name).to_string(),
        language: json["language"].as_str().unwrap_or_default().to_string(),
        name,
        argv,
        env,
        dir: dir.to_path_buf(),
    })
}

/// How the kernel wants to be interrupted: "message" (an `interrupt_request`) or "signal", the default.
pub fn interrupt_mode(spec: &KernelSpec) -> String {
    std::fs::read_to_string(spec.dir.join("kernel.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json["interrupt_mode"].as_str().map(str::to_string))
        .unwrap_or_else(|| "signal".into())
}

/// Every kernel in these folders, the first of each name winning, sorted by what they're called.
pub fn specs_in(folders: &[PathBuf]) -> Vec<KernelSpec> {
    let mut specs: Vec<KernelSpec> = Vec::new();
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(folder) else { continue };
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for spec in dirs.iter().filter_map(|d| read_spec(d)) {
            if !specs.iter().any(|s| s.name == spec.name) {
                specs.push(spec);
            }
        }
    }
    specs.sort_by_key(|s| s.display_name.to_lowercase());
    specs
}

/// The kernel made up for a Python that has ipykernel but no kernelspec to say so.
pub fn python_fallback(python: &Python) -> KernelSpec {
    KernelSpec {
        name: "python3".into(),
        display_name: "Python 3 (ipykernel)".into(),
        language: "python".into(),
        argv: vec![python.program.to_string_lossy().into_owned(), "-m".into(), "ipykernel_launcher".into(), "-f".into(), "{connection_file}".into()],
        env: Vec::new(),
        dir: python.prefix.clone(),
    }
}

/// Every kernel on this computer. This asks Python about itself, which takes a moment: call it when the list is
/// wanted, not every frame.
pub fn kernel_specs() -> Vec<KernelSpec> {
    let python = usual_python();
    let mut prefixes: Vec<PathBuf> = ["VIRTUAL_ENV", "CONDA_PREFIX"].into_iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
    prefixes.extend(python.as_ref().map(|p| p.prefix.clone()));
    let mut specs = specs_in(&kernel_folders(&|name| std::env::var(name).ok(), &prefixes));
    if !specs.iter().any(|s| s.language.eq_ignore_ascii_case("python")) && let Some(python) = python.filter(|p| p.has_ipykernel) {
        specs.insert(0, python_fallback(&python));
    }
    specs
}

/// What each kernel is called in a list: its display name, with its folder's name added where two share one.
pub fn labels(specs: &[KernelSpec]) -> Vec<String> {
    specs
        .iter()
        .map(|spec| {
            if specs.iter().filter(|s| s.display_name == spec.display_name).count() > 1 {
                format!("{} ({})", spec.display_name, spec.name)
            } else {
                spec.display_name.clone()
            }
        })
        .collect()
}

/// How a kernel for a language is installed, for a terminal to run, and the program that needs to be there; as
/// `lsp::install_command`. Python uses the active virtual environment's own pip, else conda's, else pip's
/// `--user`; where the system won't let pip install (PEP 668) it makes a virtual environment for Jupyter with
/// uv, or with pipx, and registers its kernel for this user. R uses IRkernel from CRAN.
pub fn install_command(language: &str) -> Option<(String, &'static str)> {
    let quote = |path: &Path| format!("\"{}\"", path.display());
    if language.eq_ignore_ascii_case("r") {
        let r = if cfg!(windows) { "Rscript" } else { "R" };
        let args = if cfg!(windows) { "-e" } else { "-q -e" };
        let line = format!("{r} {args} \"install.packages('IRkernel', repos = 'https://cloud.r-project.org'); IRkernel::installspec()\"");
        return crate::lsp::on_path(r).then_some((line, if cfg!(windows) { "Rscript" } else { "R" }));
    }
    if !language.eq_ignore_ascii_case("python") {
        return None;
    }
    let python = usual_python();
    if let Some(venv) = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from) && let Some(python) = python.as_ref().filter(|p| p.prefix == venv) {
        return Some((format!("{} -m pip install ipykernel", quote(&python.program)), "python"));
    }
    if std::env::var_os("CONDA_PREFIX").is_some() && crate::lsp::on_path("conda") {
        return Some(("conda install -y ipykernel".into(), "conda"));
    }
    let managed = python.as_ref().is_none_or(|p| p.externally_managed);
    if managed {
        let folder = crate::settings::data_folder().unwrap_or_else(std::env::temp_dir).join("jupyter-python");
        let program = if cfg!(windows) { folder.join("Scripts").join("python.exe") } else { folder.join("bin").join("python") };
        let register = format!("{} -m ipykernel install --user", quote(&program));
        if crate::lsp::on_path("uv") {
            return Some((format!("uv venv {folder} && uv pip install --python {program} ipykernel && {register}", folder = quote(&folder), program = quote(&program)), "uv"));
        }
        if crate::lsp::on_path("pipx") {
            // pipx keeps ipykernel in an environment of its own; its Python then registers the kernel.
            let venvs = pipx_venvs().unwrap_or_else(|| PathBuf::from("~/.local/share/pipx/venvs"));
            let pipx_python = if cfg!(windows) { venvs.join("ipykernel").join("Scripts").join("python.exe") } else { venvs.join("ipykernel").join("bin").join("python") };
            return Some((format!("pipx install --include-deps ipykernel && {} -m ipykernel install --user", quote(&pipx_python)), "pipx"));
        }
        if python.is_some() {
            // Python's own venv makes the environment where uv and pipx aren't.
            let base = python.as_ref().map(|p| quote(&p.program)).unwrap_or_default();
            return Some((format!("{base} -m venv {} && {} -m pip install ipykernel && {register}", quote(&folder), quote(&program)), "python"));
        }
        return None;
    }
    let program = python.map(|p| quote(&p.program))?;
    Some((format!("{program} -m pip install --user ipykernel"), "python"))
}

/// Where pipx keeps its environments, as it says itself.
fn pipx_venvs() -> Option<PathBuf> {
    let mut command = crate::platform::command("pipx");
    command.args(["environment", "--value", "PIPX_LOCAL_VENVS"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let output = command.output().ok().filter(|o| o.status.success())?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-jupyter-specs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_spec(root: &Path, name: &str, json: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kernel.json"), json).unwrap();
    }

    #[test]
    fn folders_are_in_jupyters_order() {
        let env = |name: &str| match name {
            "JUPYTER_PATH" => Some(std::env::join_paths(["/first", "/second"]).unwrap().to_string_lossy().into_owned()),
            "HOME" => Some("/home/me".into()),
            "APPDATA" => Some("C:\\Users\\me\\AppData\\Roaming".into()),
            "PROGRAMDATA" => Some("C:\\ProgramData".into()),
            _ => None,
        };
        let folders = kernel_folders(&env, &[PathBuf::from("/venv"), PathBuf::from("/venv")]);
        assert_eq!(folders[0], PathBuf::from("/first/kernels"));
        assert_eq!(folders[1], PathBuf::from("/second/kernels"));
        let user = if cfg!(windows) {
            PathBuf::from("C:\\Users\\me\\AppData\\Roaming").join("jupyter")
        } else if cfg!(target_os = "macos") {
            PathBuf::from("/home/me/Library/Jupyter")
        } else {
            PathBuf::from("/home/me/.local/share/jupyter")
        };
        assert_eq!(folders[2], user.join("kernels"));
        // The prefix given twice is looked in once.
        assert_eq!(folders[3], PathBuf::from("/venv").join("share").join("jupyter").join("kernels"));
        if cfg!(windows) {
            assert_eq!(folders[4], PathBuf::from("C:\\ProgramData").join("jupyter").join("kernels"));
        } else {
            assert_eq!(&folders[4..], &[PathBuf::from("/usr/local/share/jupyter/kernels"), PathBuf::from("/usr/share/jupyter/kernels")]);
        }
        // JUPYTER_DATA_DIR stands in for the user's folder.
        let env = |name: &str| (name == "JUPYTER_DATA_DIR").then(|| "/data".to_string());
        assert_eq!(kernel_folders(&env, &[])[0], PathBuf::from("/data/kernels"));
    }

    #[test]
    fn specs_are_read_and_the_first_of_a_name_wins() {
        let first = folder("first");
        let second = folder("second");
        write_spec(&first, "Python3", r#"{"argv": ["python", "-m", "ipykernel_launcher", "-f", "{connection_file}"], "display_name": "Python 3 (ipykernel)", "language": "python", "env": {"A": "1"}, "interrupt_mode": "message"}"#);
        write_spec(&second, "python3", r#"{"argv": ["other"], "display_name": "Hidden", "language": "python"}"#);
        write_spec(&second, "ir", r#"{"argv": ["R", "--slave", "-e", "IRkernel::main()", "--args", "{connection_file}"], "display_name": "R", "language": "R"}"#);
        write_spec(&second, "broken", r#"{"argv": [], "display_name": "Broken"}"#);
        write_spec(&second, "notjson", "{");
        let specs = specs_in(&[first.clone(), second.clone(), PathBuf::from("/no/such/folder")]);
        assert_eq!(specs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["python3", "ir"]);
        let python = &specs[0];
        assert_eq!(python.display_name, "Python 3 (ipykernel)");
        assert_eq!(python.argv[0], "python");
        assert_eq!(python.env, vec![("A".to_string(), "1".to_string())]);
        assert_eq!(python.dir, first.join("Python3"));
        assert_eq!(interrupt_mode(python), "message");
        assert_eq!(interrupt_mode(&specs[1]), "signal");
        let _ = std::fs::remove_dir_all(first);
        let _ = std::fs::remove_dir_all(second);
    }

    #[test]
    fn labels_tell_alike_names_apart() {
        let spec = |name: &str, display: &str| KernelSpec { name: name.into(), display_name: display.into(), language: "python".into(), argv: vec!["x".into()], env: vec![], dir: PathBuf::new() };
        let specs = [spec("python3", "Python 3"), spec("py311", "Python 3"), spec("ir", "R")];
        assert_eq!(labels(&specs), vec!["Python 3 (python3)", "Python 3 (py311)", "R"]);
        let fallback = python_fallback(&Python { program: PathBuf::from("/usr/bin/python3"), prefix: PathBuf::from("/usr"), has_ipykernel: true, externally_managed: false });
        assert_eq!(fallback.display_name, "Python 3 (ipykernel)");
        assert_eq!(fallback.argv, vec!["/usr/bin/python3", "-m", "ipykernel_launcher", "-f", "{connection_file}"]);
    }

    #[test]
    fn install_commands_name_what_they_need() {
        assert!(install_command("julia").is_none());
        if let Some((line, needs)) = install_command("R") {
            assert!(line.contains("IRkernel::installspec()"));
            assert!(needs.starts_with('R'));
        }
        if let Some((line, needs)) = install_command("python") {
            assert!(line.contains("ipykernel"), "{line}");
            assert!(["python", "conda", "uv", "pipx"].contains(&needs));
        }
    }

    /// Run by hand: `cargo test --release --offline -- --ignored jupyter::specs::tests::shows_this_computers_kernels --nocapture`.
    #[test]
    #[ignore]
    fn shows_this_computers_kernels() {
        for (spec, label) in kernel_specs().iter().zip(labels(&kernel_specs())) {
            println!("{label}: {} {:?} in {}", spec.name, spec.argv, spec.dir.display());
        }
        println!("{:?}", usual_python());
        println!("{:?} / {:?}", install_command("python"), install_command("R"));
    }
}
