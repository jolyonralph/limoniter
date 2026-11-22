//! AmigaShell programs run on this computer, with no emulator or Kickstart: vamos, from Christian Vogelgsang's
//! amitools (github.com/cnvogelg/amitools, GPL 2), runs an Amiga program's 68000 code itself and does what it asks
//! of AmigaDOS and exec on the computer's own files, so a Shell program's output comes straight back. It's a
//! Python program, got with `pip install amitools`; Limoniter only finds it and starts it.
//!
//! Three things run through it: a program F9 builds or the one in front (as an emulator would run it), an
//! AmigaShell script (through the Execute command of the person's own Workbench, which vamos needs for anything
//! beyond a program), and SAS/C's own compiler, sc, or its smake, so a SAS/C project builds unchanged.
//!
//! The command lines follow vamos's own reference (docs/vamos-ref.rst in amitools): options before the program,
//! `--` before it, -V for a volume (a folder of this computer's), -a for an assign, -p for the command path,
//! -P when the program is named by its Amiga path, -C, -m and -s for the processor, memory and stack.

use crate::compilers::Problem;
use eframe::egui::{self, RichText, Ui};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::RwLock;
use std::sync::mpsc::{Receiver, channel};

/// Where to get vamos, said wherever it isn't found.
pub const HOW_TO_GET: &str = "vamos comes with amitools, a Python package: with Python 3 installed, run pip install amitools, then click Find (or Choose\u{2026} and point at vamos).";
pub const WEBSITE: &str = "https://github.com/cnvogelg/amitools";

/// The processors vamos can be: -C takes these.
pub const CPUS: [&str; 4] = ["68000", "68020", "68030", "68040"];

/// How vamos runs things, remembered between runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub cpu: String,
    /// Memory, in KiB (-m). vamos gives 1024 unless told.
    pub ram_kib: u32,
    /// The program's stack, in KiB (-s). vamos gives 8 unless told.
    pub stack_kib: u32,
    /// A copy of a Workbench's files (with C, Libs, Devs, S and L in it), for the commands a script uses.
    pub system: Option<PathBuf>,
    /// Where SAS/C is (the folder with c/sc, include and lib in it), to build SAS/C projects.
    pub sasc: Option<PathBuf>,
    /// What's given to a program after its name, as it would be typed in the Shell.
    pub arguments: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { cpu: "68020".into(), ram_kib: 8192, stack_kib: 16, system: None, sasc: None, arguments: String::new() }
    }
}

impl Settings {
    fn to_text(&self) -> String {
        let path = |p: &Option<PathBuf>| p.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        format!("cpu={}\nram={}\nstack={}\nsystem={}\nsasc={}\narguments={}\n", self.cpu, self.ram_kib, self.stack_kib, path(&self.system), path(&self.sasc), self.arguments)
    }

    fn from_text(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let path = || (!value.trim().is_empty()).then(|| PathBuf::from(value.trim()));
            match key.trim() {
                "cpu" if CPUS.contains(&value.trim()) => settings.cpu = value.trim().into(),
                "ram" => settings.ram_kib = value.trim().parse().unwrap_or(settings.ram_kib).clamp(256, 1024 * 1024),
                "stack" => settings.stack_kib = value.trim().parse().unwrap_or(settings.stack_kib).clamp(4, 1024),
                "system" => settings.system = path(),
                "sasc" => settings.sasc = path(),
                "arguments" => settings.arguments = value.to_string(),
                _ => {}
            }
        }
        settings
    }
}

fn settings_file() -> Option<PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("vamos.txt"))
}

static SETTINGS: RwLock<Option<Settings>> = RwLock::new(None);

/// The settings, read the first time they're asked for (never from the file in tests).
pub fn settings() -> Settings {
    if let Some(known) = SETTINGS.read().ok().and_then(|s| s.clone()) {
        return known;
    }
    let read = if cfg!(test) { None } else { settings_file().and_then(|f| std::fs::read_to_string(f).ok()) };
    let settings = read.map(|t| Settings::from_text(&t)).unwrap_or_default();
    if let Ok(mut kept) = SETTINGS.write() {
        *kept = Some(settings.clone());
    }
    settings
}

/// Keeps new settings, and saves them.
pub fn set_settings(settings: Settings) {
    if !cfg!(test)
        && let Some(file) = settings_file()
    {
        let _ = std::fs::create_dir_all(file.parent().expect("in a folder"));
        let _ = std::fs::write(&file, settings.to_text());
    }
    if let Ok(mut kept) = SETTINGS.write() {
        *kept = Some(settings);
    }
}

/// The folders pip puts a package's programs in, which a program started from the desktop may not have on its
/// path: ~/.local/bin on Linux and a Mac (pip install --user, and pipx), and each Python's Scripts folder on
/// Windows, for everyone and for one person.
pub fn python_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        folders.push(home.join(".local").join("bin"));
        if cfg!(target_os = "macos") {
            // pip install --user with the python.org Python puts programs under ~/Library/Python/3.x/bin.
            folders.extend(versions(&home.join("Library").join("Python")).into_iter().map(|v| v.join("bin")));
        }
    }
    if cfg!(windows) {
        for (variable, below) in [("APPDATA", "Python"), ("LOCALAPPDATA", "Programs\\Python")] {
            if let Some(root) = std::env::var_os(variable).map(PathBuf::from) {
                folders.extend(versions(&root.join(below)).into_iter().map(|v| v.join("Scripts")));
            }
        }
    }
    folders
}

/// The Python3x folders in a folder, newest first.
fn versions(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with("python3") || n.to_string_lossy().starts_with("3."))).collect();
    found.sort();
    found.reverse();
    found
}

/// Where vamos is: on the path, or where pip usually puts it.
pub fn find() -> Option<PathBuf> {
    crate::emulators::find_program(&["vamos"], &["amitools"])
}

/// A volume's host folder as vamos takes it after `-V name:`.
fn shown(path: &Path) -> String {
    path.display().to_string()
}

/// The options every run starts with: the processor, memory and stack, and the person's Workbench as SYS:, with
/// its commands, libraries and devices assigned as on an Amiga. vamos's own automatic assigns are switched off
/// then, so they can't stand in front of the real ones.
fn machine_options(settings: &Settings, least_ram: u32, least_stack: u32) -> Vec<String> {
    let mut args = vec!["-C".to_string(), settings.cpu.clone(), "-m".into(), settings.ram_kib.max(least_ram).to_string(), "-s".into(), settings.stack_kib.max(least_stack).to_string()];
    if let Some(system) = &settings.system {
        args.extend(["-A".into(), "off".into(), "-V".into(), format!("sys:{}", shown(system))]);
        for (assign, folder) in [("c", "c"), ("s", "s"), ("libs", "libs"), ("devs", "devs"), ("l", "l")] {
            args.extend(["-a".into(), format!("{assign}:sys:{folder}")]);
        }
        // T: for the files commands leave about, in vamos's own RAM:.
        args.extend(["-a".into(), "t:ram:".into(), "-p".into(), "c:".into()]);
    }
    args
}

/// Splits the person's arguments as the Shell would: by spaces, keeping what's in double quotes together.
pub fn split_arguments(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            ' ' | '\t' if !quoted => {
                if started {
                    parts.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            _ => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        parts.push(current);
    }
    parts
}

/// What to give vamos to run a program (a file on this computer), with the arguments chosen.
pub fn program_arguments(settings: &Settings, program: &Path) -> Vec<String> {
    let mut args = machine_options(settings, 0, 0);
    args.push("--".into());
    args.push(shown(program));
    args.extend(split_arguments(&settings.arguments));
    args
}

/// What to give vamos to run an AmigaShell script, by its name in the folder vamos is started in (which it makes
/// the current directory): the Workbench's Execute does it, so there must be a Workbench to take it from.
pub fn script_arguments(settings: &Settings, script: &str) -> Result<Vec<String>, String> {
    if settings.system.is_none() {
        return Err(crate::i18n::tr("A script needs the AmigaShell's own commands, which vamos doesn't have. Choose a copy of a Workbench's files (with its C, Libs and S folders) for vamos in File, Devices, then run it again.").into());
    }
    let mut args = machine_options(settings, 0, 0);
    // -P: c:Execute is an Amiga path, not a file here (on Windows it would read as drive C).
    args.extend(["-P".into(), "--".into(), "c:Execute".into(), script.into()]);
    args.extend(split_arguments(&settings.arguments));
    Ok(args)
}

/// Whether a file is an AmigaShell script, to be run with Execute: its ending says so, or it starts with one of
/// Execute's own dot commands (.key, .bra and the rest).
pub fn is_script(name: &str, first_line: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.ends_with(".script") || lower.ends_with(".amigados") {
        return true;
    }
    let line = first_line.trim_start().to_lowercase();
    [".key ", ".k ", ".bra ", ".ket ", ".def ", ".dot "].iter().any(|dot| line.starts_with(dot))
}

/// SAS/C, through vamos: SC: is the person's SAS/C, with INCLUDE: and LIB: assigned in it as SAS/C's own
/// installer does, and its commands found in SC:c. A project with an smakefile is built by smake, as on the
/// Amiga; otherwise sc compiles and links the files (reading SCOPTIONS itself), naming the program PROGRAMNAME.
/// Both run in the project's folder, which vamos makes the current directory, so its files are named as they are
/// there.
pub fn sasc_arguments(settings: &Settings, sasc: &Path, folder: &Path, sources: &[PathBuf], stem: &str) -> Vec<String> {
    // SAS/C's passes want more stack and memory than a small program: its manual asks for 20000 bytes of stack.
    let mut args = machine_options(settings, 8192, 64);
    args.extend(["-V".into(), format!("sc:{}", shown(sasc)), "-a".into(), "include:sc:include".into(), "-a".into(), "lib:sc:lib".into()]);
    args.extend(["-a".into(), "env:ram:".into(), "-a".into(), "envarc:ram:".into()]);
    args.extend(["-p".into(), if settings.system.is_some() { "sc:c,c:".into() } else { "sc:c".into() }, "-P".into(), "--".into()]);
    if folder.join("smakefile").is_file() || folder.join("SMakefile").is_file() {
        args.push("sc:c/smake".into());
        return args;
    }
    args.extend(["sc:c/sc".into(), "LINK".into()]);
    for source in sources {
        let name = source.strip_prefix(folder).unwrap_or(source);
        // AmigaDOS's paths have forward slashes, whichever computer this is.
        args.push(name.display().to_string().replace('\\', "/"));
    }
    args.push(format!("PROGRAMNAME={stem}"));
    args
}

/// Where SAS/C is: the folder chosen, else where it's usually copied to, if it holds c/sc.
pub fn find_sasc(settings: &Settings) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let mut places: Vec<PathBuf> = settings.sasc.iter().cloned().collect();
    places.extend(["SASC", "SC"].into_iter().filter_map(std::env::var_os).map(PathBuf::from));
    if let Some(home) = &home {
        places.extend(["sasc", "sc", "amiga/sasc", "amiga/sc"].map(|p| home.join(p)));
    }
    places.extend(["C:\\SASC", "C:\\sc", "/opt/sasc"].map(PathBuf::from));
    places.into_iter().find(|p| has_sasc(p))
}

/// Whether a folder is SAS/C's: its compiler in c.
pub fn has_sasc(folder: &Path) -> bool {
    ["c/sc", "C/sc", "c/SC"].iter().any(|p| folder.join(p).is_file())
}

/// A problem as SAS/C prints one: `hello.c 6 Error 57: Semi-colon expected` (or Warning). None for any other line.
pub fn sasc_problem(line: &str) -> Option<Problem> {
    let text = line.trim();
    let mut words = text.splitn(4, ' ');
    let (file, number, kind, rest) = (words.next()?, words.next()?, words.next()?, words.next()?);
    let line: usize = number.parse().ok()?;
    let warning = match kind {
        "Error" => false,
        "Warning" => true,
        _ => return None,
    };
    let (code, message) = rest.split_once(": ")?;
    code.trim().parse::<u32>().ok()?;
    if file.is_empty() || !file.contains('.') {
        return None;
    }
    Some(Problem { file: file.into(), line, message: format!("{} {}: {}", kind.to_lowercase(), code.trim(), message.trim()), warning })
}

/// What an Amiga program's return code means, as AmigaDOS has it: 5 a warning, 10 an error, 20 a failure.
pub fn return_code(code: i32) -> String {
    let word = match code {
        0 => return crate::i18n::tr("vamos finished: the program returned 0, all well.").into(),
        1..=9 => "WARN",
        10..=19 => "ERROR",
        _ => "FAIL",
    };
    crate::i18n::tf!("vamos finished: the program returned {code} ({word}).")
}

/// A line vamos's own log printed, rather than the program: "  main:  ERROR:  ..." and the like, with or without
/// a time before it. Its problems are shown as they are; the rest of its chatter isn't.
pub fn is_vamos_log(line: &str) -> bool {
    let words: Vec<&str> = line.split_whitespace().take(3).collect();
    let Some(level) = words.iter().position(|w| matches!(*w, "ERROR:" | "WARNING:" | "INFO:" | "DEBUG:")) else {
        return false;
    };
    // The channel's name just before the level: main:, prog:, dos: and so on.
    level > 0 && words[level - 1].strip_suffix(':').is_some_and(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// A program running in vamos: what it prints, a line at a time, and whether it's finished.
pub struct Running {
    pub what: String,
    child: Child,
    lines: Receiver<String>,
}

impl Running {
    /// Starts vamos in `folder` (the Amiga's current directory), its output read as it comes.
    pub fn start(vamos: &Path, args: &[String], folder: &Path, what: String, wake: Option<egui::Context>) -> Result<Running, String> {
        let mut command = crate::platform::command(vamos);
        command.args(args).current_dir(folder).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|e| crate::i18n::tf!("vamos couldn't be started ({}): {e}", vamos.display()))?;
        let (send, lines) = channel();
        for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
            let send = send.clone();
            let wake = wake.clone();
            std::thread::spawn(move || {
                // Amiga text is ISO 8859-1, a byte to a character.
                for line in std::io::BufReader::new(pipe).split(b'\n').map_while(Result::ok) {
                    let text: String = line.iter().filter(|&&b| b != b'\r').map(|&b| b as char).collect();
                    if send.send(text).is_err() {
                        break;
                    }
                    if let Some(ctx) = &wake {
                        ctx.request_repaint();
                    }
                }
            });
        }
        Ok(Running { what, child, lines })
    }

    /// What's been printed since last asked, and how it ended once it has (with everything it printed read).
    pub fn poll(&mut self) -> (Vec<String>, Option<ExitStatus>) {
        let status = self.child.try_wait().ok().flatten();
        let mut printed: Vec<String> = self.lines.try_iter().collect();
        if let Some(status) = status {
            // The rest of what it printed, now the pipes are closing.
            let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
            while let Ok(line) = self.lines.recv_timeout(until.saturating_duration_since(std::time::Instant::now())) {
                printed.push(line);
            }
            return (printed, Some(status));
        }
        (printed, None)
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The lines for the Output panel from what vamos printed: the program's own as they are, vamos's errors and
/// warnings too, and the rest of vamos's log left out.
pub fn shown_lines(printed: Vec<String>) -> Vec<String> {
    printed.into_iter().filter(|line| !is_vamos_log(line) || line.contains("ERROR") || line.contains("WARNING")).collect()
}

/// vamos's own rows in its fields in the Devices box: the processor, memory and stack, the Workbench its
/// scripts' commands come from, SAS/C's folder, and the program's arguments.
pub fn settings_rows(ui: &mut Ui, hint: &dyn Fn(&str) -> RichText) {
    let mut settings = settings();
    let before = settings.clone();
    ui.label(crate::i18n::tr("Processor:"));
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("vamos-cpu").width(90.0).selected_text(settings.cpu.clone()).show_ui(ui, |ui| {
            for cpu in CPUS {
                ui.selectable_value(&mut settings.cpu, cpu.to_string(), cpu);
            }
        });
        ui.label(crate::i18n::tr("Memory (KiB):"));
        ui.add(egui::DragValue::new(&mut settings.ram_kib).range(256..=1024 * 1024));
        ui.label(crate::i18n::tr("Stack (KiB):"));
        ui.add(egui::DragValue::new(&mut settings.stack_kib).range(4..=1024));
    });
    ui.end_row();
    let folder_row = |ui: &mut Ui, label: &str, value: &mut Option<PathBuf>, empty: &str, title: &str| {
        ui.label(crate::i18n::tr(label));
        ui.horizontal(|ui| {
            let mut text = value.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut text).hint_text(crate::i18n::tr(empty)).desired_width(220.0)).changed() {
                *value = (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()));
            }
            if ui.button(crate::i18n::tr("Choose\u{2026}")).clicked()
                && let Some(chosen) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr(title)).pick_folder()
            {
                *value = Some(chosen);
            }
        });
        ui.end_row();
    };
    folder_row(ui, "Workbench files:", &mut settings.system, "None: programs only, no scripts", "A copy of a Workbench's files, with C, Libs and S in it");
    folder_row(ui, "SAS/C:", &mut settings.sasc, "None", "SAS/C's folder, with c/sc, include and lib in it");
    ui.label(crate::i18n::tr("Arguments:"));
    ui.add(egui::TextEdit::singleline(&mut settings.arguments).hint_text(crate::i18n::tr("Given to the program, as typed in the Shell")).desired_width(320.0));
    ui.end_row();
    ui.label("");
    ui.label(hint(crate::i18n::tr("F9 runs an Amiga program, or an AmigaShell script, here on this computer, its output in the Output panel. SAS/C projects are built with SAS/C itself when its folder is chosen.")));
    ui.end_row();
    if let Some(sasc) = &settings.sasc
        && !has_sasc(sasc)
    {
        ui.label("");
        ui.label(hint(crate::i18n::tr("That folder has no c/sc in it, so it isn't SAS/C's.")));
        ui.end_row();
    }
    if settings != before {
        set_settings(settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_kept_as_text_and_read_back() {
        let settings = Settings { cpu: "68040".into(), ram_kib: 4096, stack_kib: 32, system: Some(PathBuf::from("/amiga/wb31")), sasc: Some(PathBuf::from("/amiga/sc")), arguments: "FILE \"a b\"".into() };
        assert_eq!(Settings::from_text(&settings.to_text()), settings);
        assert_eq!(Settings::from_text("cpu=6502\nram=1\n"), Settings { ram_kib: 256, ..Settings::default() }, "what makes no sense isn't taken");
    }

    #[test]
    fn a_program_is_run_as_vamos_reference_says() {
        let settings = Settings { arguments: "FROM \"My File\" QUIET".into(), ..Settings::default() };
        let program = Path::new("/tmp/run/hello");
        assert_eq!(program_arguments(&settings, program), ["-C", "68020", "-m", "8192", "-s", "16", "--", "/tmp/run/hello", "FROM", "My File", "QUIET"]);
        // With a Workbench: its folders assigned as on an Amiga, vamos's own assigns off.
        let settings = Settings { system: Some(PathBuf::from("/amiga/wb")), ..Settings::default() };
        let args = program_arguments(&settings, program);
        let joined = args.join(" ");
        assert!(joined.starts_with("-C 68020 -m 8192 -s 16 -A off -V sys:/amiga/wb -a c:sys:c -a s:sys:s -a libs:sys:libs -a devs:sys:devs -a l:sys:l -a t:ram: -p c: -- /tmp/run/hello"), "{joined}");
    }

    #[test]
    fn a_script_is_run_with_the_workbench_s_execute() {
        assert!(script_arguments(&Settings::default(), "build.script").unwrap_err().contains("Workbench"));
        let settings = Settings { system: Some(PathBuf::from("/amiga/wb")), arguments: "ALL".into(), ..Settings::default() };
        let args = script_arguments(&settings, "build.script").unwrap();
        assert_eq!(&args[args.len() - 5..], ["-P", "--", "c:Execute", "build.script", "ALL"]);
        assert!(is_script("Startup.script", ""));
        assert!(is_script("Install", ".KEY FROM/A,TO"));
        assert!(is_script("make-it", ".bra {"));
        assert!(!is_script("hello.c", "#include <stdio.h>"));
        assert!(!is_script("notes.txt", ".keyboard layout"));
    }

    #[test]
    fn sasc_builds_with_smake_or_sc_in_the_project_s_folder() {
        let folder = std::env::temp_dir().join(format!("limoniter-vamos-sasc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("src")).unwrap();
        let settings = Settings::default();
        let sasc = Path::new("/amiga/sc");
        let sources = [folder.join("main.c"), folder.join("src").join("util.c")];
        let args = sasc_arguments(&settings, sasc, &folder, &sources, "tool");
        let joined = args.join(" ");
        // SAS/C wants a larger stack and memory than the settings' least.
        assert!(joined.starts_with("-C 68020 -m 8192 -s 64 -V sc:/amiga/sc -a include:sc:include -a lib:sc:lib"), "{joined}");
        assert!(joined.ends_with("-p sc:c -P -- sc:c/sc LINK main.c src/util.c PROGRAMNAME=tool"), "{joined}");
        std::fs::write(folder.join("smakefile"), "tool: main.o\n").unwrap();
        let args = sasc_arguments(&settings, sasc, &folder, &sources, "tool");
        assert_eq!(&args[args.len() - 3..], ["-P", "--", "sc:c/smake"]);
        assert!(!has_sasc(&folder));
        std::fs::create_dir_all(folder.join("c")).unwrap();
        std::fs::write(folder.join("c").join("sc"), "").unwrap();
        assert!(has_sasc(&folder));
        assert_eq!(find_sasc(&Settings { sasc: Some(folder.clone()), ..Settings::default() }), Some(folder.clone()));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn sasc_s_problems_are_read_by_file_and_line() {
        let problem = sasc_problem("hello.c 6 Error 57: Semi-colon expected").unwrap();
        assert_eq!((problem.file.as_str(), problem.line, problem.warning), ("hello.c", 6, false));
        assert_eq!(problem.message, "error 57: Semi-colon expected");
        let warning = sasc_problem("src/util.c 12 Warning 100: no prototype declared for function \"printf\"").unwrap();
        assert!(warning.warning);
        assert_eq!(warning.line, 12);
        assert!(sasc_problem("SAS/C Amiga Compiler 6.58").is_none());
        assert!(sasc_problem("Error 12 in something").is_none());
        assert!(sasc_problem("hello 6 Error 57: no ending isn't a file").is_none());
        // Through the compilers' own reading, which tries SAS/C's first.
        assert_eq!(crate::compilers::problem_in("hello.c 6 Error 57: Semi-colon expected"), Some(problem));
    }

    #[test]
    fn what_vamos_says_is_told_from_what_the_program_says() {
        assert!(is_vamos_log("12:00:01.123     main:  ERROR:  [Errno 2] No such file"));
        assert!(is_vamos_log("   prog:  INFO:  loading program"));
        assert!(!is_vamos_log("Hello, Amiga!"));
        assert!(!is_vamos_log("ERROR: the program's own word"));
        let shown = shown_lines(vec!["Hello".into(), "   prog:  INFO:  loading".into(), "     main:  ERROR:  can't open".into()]);
        assert_eq!(shown, ["Hello", "     main:  ERROR:  can't open"]);
        assert!(return_code(0).contains("all well"));
        assert!(return_code(5).contains("WARN"));
        assert!(return_code(10).contains("ERROR"));
        assert!(return_code(20).contains("FAIL"));
        assert_eq!(split_arguments("  a \"b c\"  \"\" d"), ["a", "b c", "", "d"]);
    }

    /// A stand-in for vamos (a shell script that prints its arguments, and returns 10) shows the whole way a run
    /// goes: started in the folder, its output read line by line, and its return code. Only where there's sh.
    #[cfg(unix)]
    #[test]
    fn a_run_is_followed_to_its_end() {
        use std::os::unix::fs::PermissionsExt;
        let folder = std::env::temp_dir().join(format!("limoniter-vamos-run-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let fake = folder.join("vamos");
        std::fs::write(&fake, "#!/bin/sh\necho \"in $(pwd)\"\nfor a in \"$@\"; do echo \"arg $a\"; done\necho '   prog:  INFO:  done' >&2\nexit 10\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut running = Running::start(&fake, &["--".into(), "hello".into()], &folder, "hello".into(), None).unwrap();
        let mut printed = Vec::new();
        let status = loop {
            let (lines, status) = running.poll();
            printed.extend(lines);
            if let Some(status) = status {
                break status;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(status.code(), Some(10));
        assert!(printed.iter().any(|l| l.starts_with("in ") && l.ends_with(&*folder.file_name().unwrap().to_string_lossy())), "{printed:?}");
        assert_eq!(shown_lines(printed).into_iter().filter(|l| l.starts_with("arg")).collect::<Vec<_>>(), ["arg --", "arg hello"]);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
