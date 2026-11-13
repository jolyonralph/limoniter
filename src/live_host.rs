//! The live link, the window's side: a running machine's memory in hex view and its screen as text, both looked
//! at again every second; and its BASIC program taken out as a listing, or put back in from one. The machine is
//! the built-in Spectrum, a ZEsarUX reached by its remote protocol, or a VICE reached by its binary monitor.

use super::run_host::SessionKind;
use super::*;
use crate::emulators::Link;
use crate::machine_link::{Live, ViceMonitor, Zrcp};
use crate::running::Machine;
use std::time::Instant;

/// How a running machine is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reach {
    /// The built-in Spectrum running for a document.
    BuiltIn(u64),
    Zrcp(u16),
    Vice(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LiveKind {
    Memory,
    Screen,
}

/// A document following a running machine.
pub(super) struct LiveView {
    pub reach: Reach,
    pub machine: Machine,
    pub kind: LiveKind,
    /// What was last read, to tell what's changed since.
    last: Vec<u8>,
    looked: Instant,
    pub problem: Option<String>,
}

const LOOK_EVERY: Duration = Duration::from_secs(1);

/// The Spectrum's system variables a BASIC program is found and put back by.
mod sysvars {
    pub const VARS: u16 = 23627;
    pub const DATADD: u16 = 23639;
    pub const PROG: u16 = 23635;
    pub const NXTLIN: u16 = 23637;
    pub const E_LINE: u16 = 23641;
    pub const K_CUR: u16 = 23643;
    pub const CH_ADD: u16 = 23645;
    pub const WORKSP: u16 = 23649;
    pub const STKBOT: u16 = 23651;
    pub const STKEND: u16 = 23653;
    pub const RAMTOP: u16 = 23730;
}

/// Whether a machine keeps its BASIC as a Spectrum does (the Next too), or as a Commodore does.
fn spectrum_family(machine: Machine) -> bool {
    machine.is_classic_spectrum() || machine == Machine::Next
}

fn word(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes.get(1).copied().unwrap_or(0)])
}

impl Limoniter {
    /// The running machine Machine commands reach: the built-in Spectrum running for the file in front, or else
    /// the one run on most recently that can be reached. With the machine it is, and where it's running.
    pub(super) fn reachable(&self) -> Option<(Reach, Machine, String)> {
        let doc_id = self.docs.get(self.active).map(|doc| doc.id);
        let reach = |session: &run_host::Session| match &session.kind {
            SessionKind::BuiltIn { doc_id, .. } => Some(Reach::BuiltIn(*doc_id)),
            SessionKind::App { port: Some(port), emulator, .. } => match emulator.link {
                Link::Zrcp => Some(Reach::Zrcp(*port)),
                Link::ViceMonitor => Some(Reach::Vice(*port)),
                // XRoar's GDB stub is for debugging; its memory and BASIC aren't followed from here.
                Link::Gdb | Link::None => None,
            },
            SessionKind::App { .. } => None,
        };
        let own = self.running.sessions.iter().find(|s| matches!(s.kind, SessionKind::BuiltIn { doc_id: d, .. } if Some(d) == doc_id));
        own.into_iter().chain(self.running.sessions.iter().rev()).find_map(|session| reach(session).map(|r| (r, session.machine, session.target.clone())))
    }

    /// Whether a machine reached this way is still running.
    fn still_running(&self, reach: Reach) -> bool {
        self.running.sessions.iter().any(|session| match (&session.kind, reach) {
            (SessionKind::BuiltIn { doc_id, .. }, Reach::BuiltIn(wanted)) => *doc_id == wanted,
            (SessionKind::App { port: Some(port), .. }, Reach::Zrcp(wanted) | Reach::Vice(wanted)) => *port == wanted,
            _ => false,
        })
    }

    /// Does something to a running machine through whatever reaches it.
    fn reach_live<R>(&mut self, reach: Reach, act: impl FnOnce(&mut dyn Live) -> Result<R, String>) -> Result<R, String> {
        match reach {
            Reach::BuiltIn(doc_id) => {
                let session = self.running.sessions.iter_mut().find(|s| matches!(s.kind, SessionKind::BuiltIn { doc_id: d, .. } if d == doc_id)).ok_or("The built-in Spectrum has been switched off.")?;
                let SessionKind::BuiltIn { machine, .. } = &mut session.kind else {
                    return Err("The built-in Spectrum has been switched off.".into());
                };
                act(machine.as_mut())
            }
            Reach::Zrcp(port) => act(&mut Zrcp::connect(port)?),
            Reach::Vice(port) => {
                let mut link = ViceMonitor::connect(port)?;
                let result = act(&mut link);
                // VICE stops the machine while it's being looked at.
                link.resume()?;
                result
            }
        }
    }

    /// The machine's screen as text.
    pub(super) fn screen_of(&mut self, reach: Reach, machine: Machine) -> Result<Vec<String>, String> {
        if let Reach::Zrcp(port) = reach {
            return Zrcp::connect(port)?.screen_text().map(|text| text.lines().map(|line| line.trim_end().to_string()).collect());
        }
        if spectrum_family(machine) {
            return self.reach_live(reach, |live| {
                let memory = live.read(0, 0x5B00)?;
                Ok(crate::zx_machine::screen_text(|address| memory.get(usize::from(address)).copied().unwrap_or(0)))
            });
        }
        // A Commodore's screen is a page of screen codes, wherever the VIC chip has been told it is.
        self.reach_live(reach, |live| {
            let d018 = live.read(0xD018, 1)?[0];
            let dd00 = live.read(0xDD00, 1)?[0];
            let base = u16::from(3 - (dd00 & 3)) * 0x4000 + u16::from(d018 >> 4) * 0x400;
            let codes = live.read(base, 1000)?;
            // Each code as the character it shows, for reading rather than for saving byte for byte.
            let set = charset::known("screen-c64");
            let shown = |code: u8| set.entry(0, code).and_then(|entry| entry.unicode).unwrap_or('.');
            Ok(codes.chunks(40).map(|row| row.iter().map(|code| shown(*code)).collect::<String>().trim_end().to_string()).collect())
        })
    }

    /// Machine, Its memory: the whole 64K the CPU sees, in hex view, looked at again every second. Typing in it
    /// changes the machine's memory.
    pub(super) fn open_machine_memory(&mut self) {
        let Some((reach, machine, target)) = self.reachable() else {
            return self.message("Its memory", "Nothing is running that Limoniter can reach.".into());
        };
        match self.reach_live(reach, |live| live.read(0, 0x10000)) {
            Ok(memory) => {
                let mut doc = Document::from_image_file(format!("{target} memory"), Vec::new(), None);
                doc.hex.load(memory.clone());
                doc.hex_mode = true;
                doc.live = Some(Box::new(LiveView { reach, machine, kind: LiveKind::Memory, last: memory, looked: Instant::now(), problem: None }));
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
            }
            Err(problem) => self.message("Its memory", problem),
        }
    }

    /// Machine, Its screen as text: read-only, looked at again every second.
    pub(super) fn open_machine_screen(&mut self) {
        let Some((reach, machine, target)) = self.reachable() else {
            return self.message("Its screen", "Nothing is running that Limoniter can reach.".into());
        };
        match self.screen_of(reach, machine) {
            Ok(lines) => {
                let mut doc = Document::untitled(format!("{target} screen"), &PRESETS[0]);
                doc.text.set_content(lines.clone());
                doc.text.mark_saved();
                doc.set_read_only(true);
                doc.live = Some(Box::new(LiveView { reach, machine, kind: LiveKind::Screen, last: lines.join("\n").into_bytes(), looked: Instant::now(), problem: None }));
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
            }
            Err(problem) => self.message("Its screen", problem),
        }
    }

    /// Machine, Take its BASIC program: the program in the machine's memory, as a listing that can be edited and
    /// put back.
    pub(super) fn take_machine_basic(&mut self) {
        let Some((reach, machine, target)) = self.reachable() else {
            return self.message("Take its BASIC program", "Nothing is running that Limoniter can reach.".into());
        };
        let taken = self.reach_live(reach, |live| {
            if spectrum_family(machine) {
                let vars = live.read(sysvars::PROG, 2).and_then(|prog| Ok((word(&prog), word(&live.read(sysvars::VARS, 2)?))))?;
                let (prog, vars) = vars;
                if vars <= prog {
                    return Ok((Vec::new(), "bas"));
                }
                let program = live.read(prog, usize::from(vars - prog))?;
                Ok((basic::plus3dos_file(&program, None), "bas"))
            } else {
                let start = word(&live.read(0x2B, 2)?);
                let end = word(&live.read(0x2D, 2)?);
                if end <= start + 2 {
                    return Ok((Vec::new(), "prg"));
                }
                let mut file = start.to_le_bytes().to_vec();
                file.extend(live.read(start, usize::from(end - start))?);
                Ok((file, "prg"))
            }
        });
        let (file, ending) = match taken {
            Ok(taken) => taken,
            Err(problem) => return self.message("Take its BASIC program", problem),
        };
        if file.is_empty() {
            return self.message("Take its BASIC program", format!("There's no BASIC program in {target}'s memory."));
        }
        let listing = basic::list(&file, ending, charset::utf8());
        if listing.lines.is_empty() {
            return self.message("Take its BASIC program", "The program in memory couldn't be listed.".into());
        }
        // Put back, it goes into the same kind of machine it came from.
        let back = if spectrum_family(machine) {
            if listing.dialect == basic::ZxDialect::Next.name() { basic::Machine::Next } else { basic::Machine::Spectrum }
        } else {
            basic::machine_of(listing.dialect, "prg", &file).unwrap_or(basic::Machine::Commodore(basic::CbmMachine::C64))
        };
        let mut lines = listing.lines;
        let zx = if spectrum_family(machine) { self.prepare_spectrum_listing(&mut lines, None) } else { None };
        let mut doc = Document::untitled(format!("{target} BASIC"), &PRESETS[0]);
        doc.text.set_content(lines);
        doc.text.zx = zx;
        doc.listing_of = Some((back, "program".into()));
        let grammar = if spectrum_family(machine) { highlight::BASIC_SINCLAIR } else { highlight::BASIC_COMMODORE };
        if let Some((_, syntax)) = highlight::engine().and_then(|engine| engine.names().iter().find(|(name, _)| name == grammar)) {
            doc.set_language(Language::Named(*syntax));
        }
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
    }

    /// Machine, Put this listing into it: the listing in front, tokenised, in place of the program in the machine's
    /// memory, as LOAD would leave it (its variables are cleared). On a Commodore it's then RUN.
    pub(super) fn put_machine_basic(&mut self) {
        let Some((reach, machine, target)) = self.reachable() else {
            return self.message("Put this listing into it", "Nothing is running that Limoniter can reach.".into());
        };
        let doc = &self.docs[self.active];
        let Some((listing_machine, _)) = doc.listing_of.clone() else {
            return self.message("Put this listing into it", "This isn't a BASIC listing. Take its BASIC program, or open a program's listing, first.".into());
        };
        let lines = doc.text.all_lines();
        let tokenised = match listing_machine {
            basic::Machine::Spectrum if spectrum_family(machine) => basic::tokenise_spectrum(&lines, basic::ZxDialect::Spectrum),
            basic::Machine::Next if spectrum_family(machine) => basic::tokenise_spectrum(&lines, basic::ZxDialect::Next),
            basic::Machine::Commodore(cbm) if !spectrum_family(machine) => basic::tokenise_commodore(&lines, cbm),
            _ => return self.message("Put this listing into it", format!("This listing is for another kind of machine than {target}.")),
        };
        let program = match tokenised {
            Ok(program) => program,
            Err(problem) => return self.message("Put this listing into it", format!("The listing couldn't be tokenised, so nothing was changed.\n\n{problem}")),
        };
        // A running program is stopped first, so the new one goes in while BASIC is waiting: a Commodore as
        // RUN/STOP would stop it (BASIC checks the key through $91), a Spectrum by pressing BREAK.
        if let Err(problem) = self.stop_basic(reach) {
            return self.message("Put this listing into it", problem);
        }
        let done = self.reach_live(reach, |live| {
            if spectrum_family(machine) {
                let prog = word(&live.read(sysvars::PROG, 2)?);
                let ramtop = word(&live.read(sysvars::RAMTOP, 2)?);
                let vars = prog.checked_add(program.len() as u16).filter(|end| u32::from(*end) + 512 < u32::from(ramtop)).ok_or("The program is too large for the room the machine has.")?;
                let e_line = vars + 1;
                let worksp = e_line + 2;
                live.write(prog, &program)?;
                live.write(vars, &[0x80, 0x0D, 0x80])?;
                for (at, value) in [(sysvars::VARS, vars), (sysvars::E_LINE, e_line), (sysvars::K_CUR, e_line), (sysvars::CH_ADD, e_line), (sysvars::WORKSP, worksp), (sysvars::STKBOT, worksp), (sysvars::STKEND, worksp), (sysvars::DATADD, prog - 1), (sysvars::NXTLIN, vars)] {
                    live.write(at, &value.to_le_bytes())?;
                }
                Ok(())
            } else {
                let load = word(&program);
                let body = &program[2..];
                let end = load.checked_add(body.len() as u16).ok_or("The program is too large for the room the machine has.")?;
                live.write(load, body)?;
                for at in [0x2D, 0x2F, 0x31] {
                    live.write(at, &end.to_le_bytes())?;
                }
                Ok(())
            }
        });
        if let Err(problem) = done {
            return self.message("Put this listing into it", problem);
        }
        let ran = self.type_run(reach, machine).is_ok();
        self.running.note = Some((if ran { format!("Put the listing into {target}, and ran it") } else { format!("Put the listing into {target}: type RUN there to run it") }, Instant::now()));
    }

    /// Stops the BASIC program a machine is running, if it's running one, as the person would at its keyboard.
    fn stop_basic(&mut self, reach: Reach) -> Result<(), String> {
        use rustzx_core::zx::keys::CompoundKey;
        match reach {
            Reach::Vice(_) => {
                self.reach_live(reach, |live| live.write(0x91, &[0x7F]))?;
                std::thread::sleep(Duration::from_millis(400));
            }
            Reach::Zrcp(port) => {
                // ZEsarUX's own key numbers: Caps Shift and Space together are BREAK.
                let mut link = Zrcp::connect(port)?;
                link.command("send-keys-event 135 1")?;
                link.command("send-keys-event 128 1")?;
                std::thread::sleep(Duration::from_millis(300));
                link.command("send-keys-event 128 0")?;
                link.command("send-keys-event 135 0")?;
                std::thread::sleep(Duration::from_millis(500));
            }
            Reach::BuiltIn(doc_id) => {
                let machine = self.built_in(doc_id).ok_or("The built-in Spectrum has been switched off.")?;
                machine.compound(CompoundKey::Break, true);
                for _ in 0..10 {
                    machine.frame()?;
                }
                machine.compound(CompoundKey::Break, false);
                for _ in 0..25 {
                    machine.frame()?;
                }
            }
        }
        Ok(())
    }

    /// Types RUN, as each machine takes it: a Commodore's keyboard buffer has the letters; a 48K Spectrum makes
    /// RUN from R, as the first key of a line; later Spectrums, with their editor, take the letters.
    fn type_run(&mut self, reach: Reach, machine: Machine) -> Result<(), String> {
        let keyword = machine == Machine::Spectrum48;
        match reach {
            Reach::Vice(port) => {
                let mut link = ViceMonitor::connect(port)?;
                link.type_text("RUN\n")?;
                link.resume()
            }
            Reach::Zrcp(port) => {
                let mut link = Zrcp::connect(port)?;
                let keys: &[u32] = if keyword { &[114] } else { &[114, 117, 110] };
                for key in keys.iter().chain([&129]) {
                    link.command(&format!("send-keys-event {key} 1"))?;
                    std::thread::sleep(Duration::from_millis(80));
                    link.command(&format!("send-keys-event {key} 0"))?;
                    std::thread::sleep(Duration::from_millis(80));
                }
                Ok(())
            }
            Reach::BuiltIn(doc_id) => {
                let machine = self.built_in(doc_id).ok_or("The built-in Spectrum has been switched off.")?;
                for c in if keyword { "r\n" } else { "run\n" }.chars() {
                    machine.type_char(c);
                }
                Ok(())
            }
        }
    }

    /// The built-in Spectrum running for a document.
    fn built_in(&mut self, doc_id: u64) -> Option<&mut crate::zx_machine::BuiltIn> {
        self.running.sessions.iter_mut().find_map(|session| match &mut session.kind {
            SessionKind::BuiltIn { machine, doc_id: d, .. } if *d == doc_id => Some(machine.as_mut()),
            _ => None,
        })
    }

    /// Follows the documents showing a running machine, looking again every second. Edits to memory go to the
    /// machine as they're made. True while anything is being followed.
    pub(super) fn follow_live_views(&mut self) -> bool {
        let mut following = false;
        for index in 0..self.docs.len() {
            let Some(view) = &self.docs[index].live else {
                continue;
            };
            let (reach, machine, kind) = (view.reach, view.machine, view.kind);
            if view.problem.is_some() {
                continue;
            }
            if !self.still_running(reach) {
                if let Some(view) = &mut self.docs[index].live {
                    view.problem = Some("The machine has stopped, so this is as it was last.".into());
                }
                continue;
            }
            following = true;
            let edited = kind == LiveKind::Memory && self.docs[index].hex.is_modified();
            if !edited && view.looked.elapsed() < LOOK_EVERY {
                continue;
            }
            let result = match kind {
                LiveKind::Memory if edited => {
                    // What's changed since it was last read goes to the machine, a stretch at a time.
                    let now = self.docs[index].hex.bytes();
                    let last = view.last.clone();
                    let written = self.reach_live(reach, |live| {
                        let mut at = 0;
                        while at < now.len().min(last.len()) {
                            if now[at] == last[at] {
                                at += 1;
                                continue;
                            }
                            let start = at;
                            while at < now.len().min(last.len()) && now[at] != last[at] {
                                at += 1;
                            }
                            live.write(start as u16, &now[start..at])?;
                        }
                        Ok(())
                    });
                    written.map(|()| {
                        let doc = &mut self.docs[index];
                        doc.hex.mark_saved();
                        now
                    })
                }
                LiveKind::Memory => self.reach_live(reach, |live| live.read(0, 0x10000)).map(|memory| {
                    let doc = &mut self.docs[index];
                    if doc.live.as_ref().is_some_and(|v| v.last != memory) {
                        doc.hex.refresh(memory.clone());
                    }
                    memory
                }),
                LiveKind::Screen => self.screen_of(reach, machine).map(|lines| {
                    let joined = lines.join("\n").into_bytes();
                    let doc = &mut self.docs[index];
                    if doc.live.as_ref().is_some_and(|v| v.last != joined) {
                        doc.text.set_content(lines);
                        doc.text.mark_saved();
                    }
                    joined
                }),
            };
            if let Some(view) = &mut self.docs[index].live {
                view.looked = Instant::now();
                match result {
                    Ok(seen) => view.last = seen,
                    Err(problem) => view.problem = Some(problem),
                }
            }
        }
        following
    }
}

#[cfg(test)]
#[path = "live_host_tests.rs"]
mod tests;
