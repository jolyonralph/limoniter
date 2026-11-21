//! The window's side of vamos: an Amiga program run on this computer through vamos when it's where F9 runs Amiga
//! programs (File, Devices), and an AmigaShell script run with F9 through vamos and the Workbench's Execute, each
//! with what it prints in the Output panel as it comes, and its return code at the end.

use super::*;
use crate::amiga_vamos::{self as vamos, Running};
use crate::running::Runnable;

impl Limoniter {
    /// The vamos to run with: the one in the Devices list, else the one found where pip puts it.
    fn vamos_program(&self) -> Option<PathBuf> {
        let listed = self.sends.devices.iter().find_map(|d| match &d.place {
            crate::deploy::Place::Emulator { emulator, program, .. } if emulator == "vamos" && program.is_file() => Some(program.clone()),
            _ => None,
        });
        listed.or_else(vamos::find)
    }

    /// Runs an Amiga program through vamos: the program in a folder of its own, the Amiga's current directory the
    /// source's folder (so it finds its files as it would there), what it prints in the Output panel.
    pub(super) fn run_in_vamos(&mut self, program: &Path, run: &Runnable) -> Result<(), String> {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let folder = std::env::temp_dir().join(format!("limoniter-vamos-{}", std::process::id())).join(number.to_string());
        std::fs::create_dir_all(&folder).map_err(|e| crate::i18n::tf!("Couldn't make the copy to run.\n\n{e}"))?;
        let file = folder.join(&run.name);
        std::fs::write(&file, run.bytes.as_slice()).map_err(|e| crate::i18n::tf!("Couldn't make the copy to run.\n\n{e}"))?;
        let here = self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(Path::parent).map(Path::to_path_buf).filter(|f| f.is_dir()).unwrap_or(folder);
        let args = vamos::program_arguments(&vamos::settings(), &file);
        self.start_vamos(program, &args, &here, run.name.clone())
    }

    /// F9 on an AmigaShell script runs it through vamos. False when the file in front isn't one.
    pub(super) fn run_script_in_vamos(&mut self) -> bool {
        let Some(doc) = self.docs.get(self.active) else {
            return false;
        };
        if self.no_file() || doc.hex_mode || doc.browser.is_some() || doc.text.is_disk_backed() || !vamos::is_script(&doc.name(), doc.text.all_lines().first().map_or("", |l| l.as_ref())) {
            return false;
        }
        let index = self.active;
        if (self.docs[index].path.is_none() || self.docs[index].is_modified()) && (!self.save(index) || self.docs[index].path.is_none()) {
            self.message("Run on machine", crate::i18n::tr("Save the script first, so vamos can read it.").into());
            return true;
        }
        let Some(program) = self.vamos_program() else {
            self.message("Run on machine", crate::i18n::tf!("This AmigaShell script runs through vamos, which wasn't found. {}", vamos::HOW_TO_GET));
            return true;
        };
        let Some(path) = self.docs[index].path.clone() else { return true };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let outcome = vamos::script_arguments(&vamos::settings(), &name).and_then(|args| self.start_vamos(&program, &args, &folder, name.clone()));
        match outcome {
            Ok(()) => self.running.note = Some((crate::i18n::tf!("Running {name} in vamos"), std::time::Instant::now())),
            Err(problem) => self.message("Run on machine", problem),
        }
        true
    }

    fn start_vamos(&mut self, program: &Path, args: &[String], folder: &Path, what: String) -> Result<(), String> {
        self.script_print(crate::i18n::tf!("Running {what} in vamos, in {}\u{2026}", folder.display()), true);
        self.script_print(format!("vamos {}", args.join(" ")), false);
        let running = Running::start(program, args, folder, what, self.waker.clone())?;
        self.vamos_runs.push(running);
        Ok(())
    }

    /// What the programs running in vamos have printed, into the Output panel; and how each ended.
    pub(super) fn vamos_tick(&mut self, ctx: &Context) {
        if self.vamos_runs.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(200));
        let mut lines = Vec::new();
        self.vamos_runs.retain_mut(|running| {
            let (printed, status) = running.poll();
            lines.extend(vamos::shown_lines(printed));
            match status {
                Some(status) => {
                    lines.push(match status.code() {
                        Some(code) => vamos::return_code(code),
                        None => crate::i18n::tf!("vamos was stopped ({}).", running.what),
                    });
                    false
                }
                None => true,
            }
        });
        for line in lines {
            self.script_print(line, false);
        }
    }

    /// Stops whatever's running in vamos (Run, Stop running).
    pub(super) fn stop_vamos(&mut self) {
        for mut running in self.vamos_runs.drain(..) {
            running.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    // The tests need sh for their stand-in vamos, so on Windows there are none to use these.
    #[cfg(unix)]
    use super::*;
    #[cfg(unix)]
    use crate::app::tests::{app_with, draw};

    /// With a stand-in for vamos in the Devices list, F9 on a script runs it through vamos, and its output comes
    /// into the Output panel with its return code. Only where there's sh.
    #[cfg(unix)]
    #[test]
    fn f9_runs_a_script_through_vamos_and_shows_what_it_printed() {
        use std::os::unix::fs::PermissionsExt;
        let folder = std::env::temp_dir().join(format!("limoniter-vamos-host-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let fake = folder.join("vamos");
        std::fs::write(&fake, "#!/bin/sh\nfor a in \"$@\"; do echo \"arg $a\"; done\nexit 5\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let script = folder.join("build.script");
        std::fs::write(&script, "echo \"hello\"\n").unwrap();
        let (mut app, ctx) = app_with("");
        app.sends.devices = vec![crate::deploy::Device { name: "vamos".into(), place: crate::deploy::Place::Emulator { emulator: "vamos".into(), program: fake.clone(), options: String::new() }, preset: None, eject: false }];
        assert!(app.open_path(&script));
        // Without a Workbench to take Execute from, it says so.
        vamos::set_settings(vamos::Settings::default());
        assert!(app.run_script_in_vamos());
        assert!(app.vamos_runs.is_empty());
        vamos::set_settings(vamos::Settings { system: Some(folder.clone()), ..vamos::Settings::default() });
        app.dialog = Dialog::None;
        assert!(app.run_script_in_vamos());
        assert_eq!(app.vamos_runs.len(), 1);
        let started = std::time::Instant::now();
        while !app.vamos_runs.is_empty() && started.elapsed().as_secs() < 20 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            app.vamos_tick(&ctx);
        }
        draw(&mut app, &ctx, Vec::new());
        let output = app.script_output.join("\n");
        assert!(output.contains("arg c:Execute") && output.contains("arg build.script"), "{output}");
        assert!(output.contains("returned 5 (WARN)"), "{output}");
        vamos::set_settings(vamos::Settings::default());
        // Something that isn't a script isn't run this way.
        let source = folder.join("hello.c");
        std::fs::write(&source, "int main(void) { return 0; }\n").unwrap();
        assert!(app.open_path(&source));
        assert!(!app.run_script_in_vamos());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
