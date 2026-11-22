//! Running a notebook's cells: which kernel it uses, the queue of cells waiting to run, and what the kernel's events
//! do to the cells (outputs added, counts set, a clear, a question asked). The window calls `pump` each frame;
//! nothing here draws, and the kernel is only ever reached through `KernelLink`, so all of it is tested with a fake.

use crate::jupyter::link::{KernelEvent, KernelLink, KernelSpec, KernelStatus};
use crate::notebook::{CellKind, Notebook};
use crate::notebook_output::add_stream;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

/// Starts a kernel in a folder. The notebook is given one of these, so it doesn't care how kernels are reached.
pub type StartKernel = fn(&KernelSpec, &Path) -> Result<Box<dyn KernelLink>, String>;
/// The kernels that can be started.
pub type ListKernels = fn() -> Vec<KernelSpec>;

/// How the window reaches kernels.
#[derive(Clone, Copy)]
pub struct Kernels {
    pub start: StartKernel,
    pub list: ListKernels,
}

impl Kernels {
    /// Kernels that don't run anything: they say what they were given. For the tests.
    #[cfg(test)]
    pub fn fake() -> Self {
        Self { start: |spec, _| Ok(Box::new(FakeKernel::new(&spec.language))), list: fake_specs }
    }
}

/// The kernels the window uses: the ones installed on this computer.
pub fn kernels() -> Kernels {
    // The window's tests use kernels that run nothing, so they behave the same on every computer.
    #[cfg(test)]
    return Kernels::fake();
    #[cfg(not(test))]
    Kernels { start: crate::jupyter::start, list: crate::jupyter::kernel_specs }
}

#[cfg(test)]
fn fake_specs() -> Vec<KernelSpec> {
    let spec = |name: &str, display: &str, language: &str| KernelSpec { name: name.into(), display_name: display.into(), language: language.into(), argv: Vec::new(), env: Vec::new(), dir: PathBuf::new() };
    vec![spec("python3", "Python 3 (stand-in: runs nothing)", "python"), spec("ir", "R (stand-in: runs nothing)", "R")]
}

/// A kernel that runs nothing: each cell's code comes back as its output, with a line saying so.
#[cfg(test)]
pub struct FakeKernel {
    language: String,
    next_request: u64,
    count: u32,
    events: Vec<KernelEvent>,
    status: KernelStatus,
}

#[cfg(test)]
impl FakeKernel {
    pub fn new(language: &str) -> Self {
        Self { language: language.to_string(), next_request: 1, count: 0, events: vec![KernelEvent::Status(KernelStatus::Idle)], status: KernelStatus::Idle }
    }
}

#[cfg(test)]
impl KernelLink for FakeKernel {
    fn execute(&mut self, code: &str) -> u64 {
        let request = self.next_request;
        self.next_request += 1;
        self.count += 1;
        let text = format!("{}\n{code}\n", "(A stand-in kernel: nothing was run. Here is the code that was given.)");
        self.events.push(KernelEvent::Status(KernelStatus::Busy));
        self.events.push(KernelEvent::Output { request, output: serde_json::json!({"output_type": "stream", "name": "stdout", "text": text}) });
        self.events.push(KernelEvent::Done { request, execution_count: Some(self.count), ok: true });
        self.events.push(KernelEvent::Status(KernelStatus::Idle));
        request
    }
    /// The window's own questions get answers made up: a list of two variables, and a table of two columns.
    fn execute_quietly(&mut self, code: &str) -> u64 {
        let request = self.next_request;
        self.next_request += 1;
        let text = if code.contains("__limoniter_list") || code.contains("esc(class(v)[1])") {
            "@@LIMONITER@@[{\"name\":\"frame\",\"type\":\"pandas.DataFrame\",\"size\":\"250000 \u{d7} 2\",\"kind\":\"frame\",\"value\":\"\"},{\"name\":\"n\",\"type\":\"int\",\"size\":\"\",\"kind\":\"\",\"value\":\"5\"}]@@LIMONITER@@".to_string()
        } else {
            "@@LIMONITER@@{\"columns\":[\"id\",\"name\"],\"kinds\":[\"int\",\"unknown\"],\"rows\":[[1,\"a\"],[2,\"b\"],[3,null]],\"total\":250000}@@LIMONITER@@".to_string()
        };
        self.events.push(KernelEvent::Output { request, output: serde_json::json!({"output_type": "stream", "name": "stdout", "text": text}) });
        self.events.push(KernelEvent::Done { request, execution_count: Some(self.count), ok: true });
        request
    }
    fn poll(&mut self) -> Vec<KernelEvent> {
        std::mem::take(&mut self.events)
    }
    fn status(&self) -> KernelStatus {
        self.status
    }
    fn interrupt(&mut self) {}
    fn restart(&mut self) -> Result<(), String> {
        self.count = 0;
        Ok(())
    }
    fn send_input(&mut self, _text: &str) {}
    fn shutdown(&mut self) {
        self.status = KernelStatus::Dead;
    }
    fn language(&self) -> String {
        self.language.clone()
    }
}

/// Where a cell is in running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellState {
    Idle,
    /// Waiting for the cells before it.
    Queued,
    Running,
}

/// What a running cell has asked the person to type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputAsk {
    pub uid: u64,
    pub prompt: String,
    pub password: bool,
}

/// Code run on the kernel for the window's own use (the variables list), whose output comes back to the caller
/// rather than going to a cell.
struct SideRun {
    request: u64,
    token: u64,
    /// What it printed.
    output: String,
    /// What it failed with, if it did.
    error: Option<String>,
}

pub struct Session {
    kernels: Kernels,
    /// The kernel chosen, once there is one.
    pub spec: Option<KernelSpec>,
    link: Option<Box<dyn KernelLink>>,
    starting: Option<Receiver<Result<Box<dyn KernelLink>, String>>>,
    /// What the window shows; changes as the kernel's events come.
    pub status: KernelStatus,
    /// Why the kernel couldn't start or has stopped, in words for the person.
    pub problem: Option<String>,
    queue: VecDeque<u64>,
    /// The request being run and the cell it came from.
    running: Option<(u64, u64)>,
    /// Cells whose outputs are to be cleared when their next output comes (`clear_output(wait=True)`).
    clear_next: HashSet<u64>,
    displays: HashMap<String, (u64, usize)>,
    pub input: Option<InputAsk>,
    folder: PathBuf,
    repaint: Option<eframe::egui::Context>,
    /// Code asked for the window's own use, waiting for the kernel to be free of cells.
    side_queue: VecDeque<(u64, String)>,
    side_running: Option<SideRun>,
    /// Answers ready to be taken, by the number `ask` gave.
    side_done: Vec<(u64, Result<String, String>)>,
    side_count: u64,
}

impl Session {
    pub fn new(kernels: Kernels, folder: &Path) -> Self {
        Self { kernels, spec: None, link: None, starting: None, status: KernelStatus::Dead, problem: None, queue: VecDeque::new(), running: None, clear_next: HashSet::new(), displays: HashMap::new(), input: None, folder: folder.to_path_buf(), repaint: None, side_queue: VecDeque::new(), side_running: None, side_done: Vec::new(), side_count: 0 }
    }

    /// For tests: a session with a link already made.
    #[cfg(test)]
    pub fn with_link(kernels: Kernels, spec: Option<KernelSpec>, link: Box<dyn KernelLink>) -> Self {
        let mut session = Self::new(kernels, Path::new("."));
        session.status = link.status();
        session.spec = spec;
        session.link = Some(link);
        session
    }

    /// The kernel's language ("python", "R") once it has said, for colouring a notebook that doesn't name one.
    pub fn language(&self) -> Option<String> {
        self.link.as_ref().map(|link| link.language()).filter(|l| !l.is_empty())
    }

    pub fn set_folder(&mut self, folder: &Path) {
        self.folder = folder.to_path_buf();
    }

    pub fn set_repaint(&mut self, ctx: &eframe::egui::Context) {
        if self.repaint.is_none() {
            self.repaint = Some(ctx.clone());
            crate::jupyter::set_repaint(ctx);
        }
    }

    /// The kernel a notebook asks for: by its kernelspec's name, else the first of its language. `None` means
    /// the person has to choose.
    pub fn pick_spec(specs: &[KernelSpec], notebook: &Notebook) -> Option<KernelSpec> {
        if let Some(name) = notebook.kernel_name()
            && let Some(found) = specs.iter().find(|s| s.name == name)
        {
            return Some(found.clone());
        }
        let language = notebook.language().or_else(|| notebook.metadata.get("kernelspec")?.get("language")?.as_str())?;
        let same = |a: &str| a.eq_ignore_ascii_case(language);
        specs.iter().find(|s| same(&s.language)).cloned()
    }

    /// A kernel that has stopped is started again, so running a cell after a crash just works.
    pub fn revive(&mut self) {
        if self.status == KernelStatus::Dead && self.link.is_some() && self.starting.is_none() {
            self.restart();
        }
    }

    pub fn is_running_anything(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }

    pub fn has_kernel(&self) -> bool {
        self.link.is_some() || self.starting.is_some()
    }

    pub fn state_of(&self, uid: u64) -> CellState {
        if self.running.is_some_and(|(_, running)| running == uid) {
            CellState::Running
        } else if self.queue.contains(&uid) {
            CellState::Queued
        } else {
            CellState::Idle
        }
    }

    /// Starts the kernel on another thread (starting one waits for it to answer), dropping any there was.
    pub fn start(&mut self, spec: KernelSpec) {
        self.stop();
        self.problem = None;
        self.status = KernelStatus::Starting;
        let (send, receive) = std::sync::mpsc::channel();
        let (start, folder, ctx) = (self.kernels.start, self.folder.clone(), self.repaint.clone());
        let for_thread = spec.clone();
        self.spec = Some(spec);
        self.starting = Some(receive);
        std::thread::spawn(move || {
            let made = start(&for_thread, &folder);
            let _ = send.send(made);
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    /// Lets go of the kernel: shut down, and nothing queued.
    pub fn stop(&mut self) {
        if let Some(mut link) = self.link.take() {
            link.shutdown();
        }
        self.starting = None;
        self.queue.clear();
        self.running = None;
        self.input = None;
        self.clear_next.clear();
        self.displays.clear();
        self.forget_side_runs(crate::i18n::tr("The kernel was stopped.").to_string());
        self.status = KernelStatus::Dead;
    }

    /// Puts cells in the queue to run, in order. Cells that aren't code are passed over. Starts the kernel first
    /// if there isn't one and the notebook has said which it wants; returns false if there is no kernel to run on.
    pub fn run(&mut self, notebook: &mut Notebook, uids: &[u64]) -> bool {
        if !self.has_kernel() {
            return false;
        }
        for &uid in uids {
            let Some(at) = notebook.index_of(uid) else { continue };
            let cell = &mut notebook.cells[at];
            if cell.kind != CellKind::Code || self.queue.contains(&uid) || self.running.is_some_and(|(_, u)| u == uid) {
                continue;
            }
            // Nothing in it: there's nothing to run, and what it showed before is gone.
            if cell.source.trim().is_empty() {
                cell.clear_outputs();
                continue;
            }
            self.queue.push_back(uid);
        }
        self.start_next(notebook);
        true
    }

    fn start_next(&mut self, notebook: &mut Notebook) {
        if self.running.is_some() || self.input.is_some() {
            return;
        }
        let Some(link) = self.link.as_mut() else { return };
        while let Some(uid) = self.queue.pop_front() {
            let Some(at) = notebook.index_of(uid) else { continue };
            let cell = &mut notebook.cells[at];
            cell.clear_outputs();
            self.displays.retain(|_, (u, _)| *u != uid);
            let request = link.execute(&cell.source);
            self.running = Some((request, uid));
            return;
        }
        // With no cell waiting, what the window asked for itself goes next.
        if self.side_running.is_none()
            && self.status != KernelStatus::Starting
            && let Some((token, code)) = self.side_queue.pop_front()
        {
            let request = link.execute_quietly(&code);
            self.side_running = Some(SideRun { request, token, output: String::new(), error: None });
        }
    }

    /// Asks the kernel to run `code` for the window's own use, once no cell is waiting. What it prints comes back
    /// through `take_answers` with the number returned, and the code doesn't appear in any cell, count as an
    /// execution, or enter the kernel's history.
    pub fn ask(&mut self, code: String) -> u64 {
        self.side_count += 1;
        self.side_queue.push_back((self.side_count, code));
        self.side_count
    }

    /// The answers that have come to what `ask` was given: the text the code printed, or what it failed with.
    pub fn take_answers(&mut self) -> Vec<(u64, Result<String, String>)> {
        std::mem::take(&mut self.side_done)
    }

    /// Whether something asked for with `ask` is still waiting or running.
    pub fn is_asking(&self) -> bool {
        self.side_running.is_some() || !self.side_queue.is_empty()
    }

    fn forget_side_runs(&mut self, why: String) {
        let waiting = std::mem::take(&mut self.side_queue);
        let running = self.side_running.take();
        for token in running.map(|r| r.token).into_iter().chain(waiting.into_iter().map(|(token, _)| token)) {
            self.side_done.push((token, Err(why.clone())));
        }
    }

    /// What the kernel said about something asked for with `ask`. True if the event was about that.
    fn side_event(&mut self, event: &KernelEvent) -> bool {
        let Some(run) = self.side_running.as_mut() else { return false };
        match event {
            KernelEvent::Output { request, output } if *request == run.request => {
                match output.get("output_type").and_then(Value::as_str) {
                    Some("stream") if output.get("name").and_then(Value::as_str) != Some("stderr") => run.output.push_str(&crate::notebook_output::text_of(output.get("text").unwrap_or(&Value::Null))),
                    Some("error") => {
                        let word = |key: &str| output.get(key).and_then(Value::as_str).unwrap_or("");
                        run.error = Some(format!("{}: {}", word("ename"), word("evalue")));
                    }
                    _ => {}
                }
                true
            }
            KernelEvent::ClearOutput { request, .. } if *request == run.request => true,
            KernelEvent::InputRequest { request, .. } if *request == run.request => true,
            KernelEvent::Done { request, ok, .. } if *request == run.request => {
                let (token, output, error) = (run.token, std::mem::take(&mut run.output), run.error.take());
                self.side_running = None;
                let result = match error {
                    Some(why) => Err(why),
                    None if !*ok => Err(crate::i18n::tr("The kernel couldn't run it.").to_string()),
                    None => Ok(output),
                };
                self.side_done.push((token, result));
                true
            }
            _ => false,
        }
    }

    /// Stops the cell that's running, and drops the cells waiting after it, as Jupyter does.
    pub fn interrupt(&mut self) {
        self.queue.clear();
        if let Some(link) = self.link.as_mut() {
            link.interrupt();
        }
    }

    /// Starts the kernel afresh, and drops everything that was queued.
    pub fn restart(&mut self) {
        self.queue.clear();
        self.running = None;
        self.input = None;
        self.clear_next.clear();
        self.displays.clear();
        self.forget_side_runs(crate::i18n::tr("The kernel was restarted.").to_string());
        self.problem = None;
        if let Some(link) = self.link.as_mut() {
            match link.restart() {
                Ok(()) => self.status = KernelStatus::Starting,
                Err(why) => {
                    self.problem = Some(why);
                    self.status = KernelStatus::Dead;
                }
            }
        }
    }

    pub fn answer(&mut self, text: &str) {
        if self.input.take().is_some()
            && let Some(link) = self.link.as_mut()
        {
            link.send_input(text);
        }
    }

    /// Takes in what has happened since the last call: a kernel that has finished starting, and the events of
    /// one that's running. Returns whether the notebook changed (an output, a count) so the window can keep the
    /// text in step.
    pub fn pump(&mut self, notebook: &mut Notebook) -> bool {
        let mut changed = false;
        if let Some(receive) = &self.starting {
            match receive.try_recv() {
                Ok(Ok(link)) => {
                    self.status = link.status();
                    self.link = Some(link);
                    self.starting = None;
                }
                Ok(Err(why)) => {
                    self.problem = Some(why);
                    self.status = KernelStatus::Dead;
                    self.starting = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.problem = Some(crate::i18n::tr("The kernel couldn't be started.").to_string());
                    self.status = KernelStatus::Dead;
                    self.starting = None;
                }
            }
        }
        let events = self.link.as_mut().map(|link| link.poll()).unwrap_or_default();
        for event in events {
            changed |= self.apply(notebook, event);
        }
        // A kernel that's ready, or a cell that's finished, lets the next one begin.
        let before = self.running;
        self.start_next(notebook);
        changed || before != self.running
    }

    fn apply(&mut self, notebook: &mut Notebook, event: KernelEvent) -> bool {
        if self.side_event(&event) {
            return false;
        }
        match event {
            KernelEvent::Status(status) => {
                self.status = status;
                false
            }
            KernelEvent::Died(why) => {
                self.status = KernelStatus::Dead;
                self.problem = Some(why);
                self.queue.clear();
                self.running = None;
                self.input = None;
                self.forget_side_runs(crate::i18n::tr("The kernel has stopped.").to_string());
                false
            }
            KernelEvent::InputRequest { request, prompt, password } => {
                if let Some((running, uid)) = self.running
                    && running == request
                {
                    self.input = Some(InputAsk { uid, prompt, password });
                }
                false
            }
            KernelEvent::Output { request, output } => {
                let Some(uid) = self.cell_of(request) else { return false };
                let Some(at) = notebook.index_of(uid) else { return false };
                let cell = &mut notebook.cells[at];
                if self.clear_next.remove(&uid) {
                    cell.outputs.clear();
                    self.displays.retain(|_, (u, _)| *u != uid);
                }
                if output.get("output_type").and_then(Value::as_str) == Some("stream") {
                    let name = output.get("name").and_then(Value::as_str).unwrap_or("stdout").to_string();
                    add_stream(&mut cell.outputs, &name, &crate::notebook_output::text_of(output.get("text").unwrap_or(&Value::Null)));
                } else {
                    if let Some(id) = output.get("transient").and_then(|t| t.get("display_id")).and_then(Value::as_str) {
                        self.displays.insert(id.to_string(), (uid, cell.outputs.len()));
                    }
                    cell.outputs.push(output);
                }
                true
            }
            KernelEvent::ClearOutput { request, wait } => {
                let Some(uid) = self.cell_of(request) else { return false };
                if wait {
                    self.clear_next.insert(uid);
                    return false;
                }
                let Some(at) = notebook.index_of(uid) else { return false };
                notebook.cells[at].outputs.clear();
                self.displays.retain(|_, (u, _)| *u != uid);
                true
            }
            KernelEvent::UpdateDisplay { display_id, output } => {
                let Some(&(uid, index)) = self.displays.get(&display_id) else { return false };
                let Some(at) = notebook.index_of(uid) else { return false };
                let Some(slot) = notebook.cells[at].outputs.get_mut(index) else { return false };
                *slot = output;
                true
            }
            KernelEvent::Done { request, execution_count, ok } => {
                let Some(uid) = self.cell_of(request) else { return false };
                self.running = None;
                self.input = None;
                self.clear_next.remove(&uid);
                if !ok {
                    // As in Jupyter, a cell that failed stops the ones after it from running.
                    self.queue.clear();
                }
                let Some(at) = notebook.index_of(uid) else { return false };
                notebook.cells[at].execution_count = execution_count.map(u64::from);
                true
            }
        }
    }

    fn cell_of(&self, request: u64) -> Option<u64> {
        self.running.filter(|(r, _)| *r == request).map(|(_, uid)| uid)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(link) = self.link.as_mut() {
            link.shutdown();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// What a test's kernel was asked, and what it will say next.
    #[derive(Default)]
    pub struct Shared {
        pub executed: Vec<String>,
        pub interrupts: usize,
        pub restarts: usize,
        pub inputs: Vec<String>,
        pub shut_down: bool,
        /// Events to hand over at the next poll.
        pub events: Vec<KernelEvent>,
    }

    pub struct Scripted(pub Arc<Mutex<Shared>>);

    impl KernelLink for Scripted {
        fn execute(&mut self, code: &str) -> u64 {
            let mut shared = self.0.lock().unwrap();
            shared.executed.push(code.to_string());
            shared.executed.len() as u64
        }
        fn poll(&mut self) -> Vec<KernelEvent> {
            std::mem::take(&mut self.0.lock().unwrap().events)
        }
        fn status(&self) -> KernelStatus {
            KernelStatus::Idle
        }
        fn interrupt(&mut self) {
            self.0.lock().unwrap().interrupts += 1;
        }
        fn restart(&mut self) -> Result<(), String> {
            self.0.lock().unwrap().restarts += 1;
            Ok(())
        }
        fn send_input(&mut self, text: &str) {
            self.0.lock().unwrap().inputs.push(text.to_string());
        }
        fn shutdown(&mut self) {
            self.0.lock().unwrap().shut_down = true;
        }
        fn language(&self) -> String {
            "python".into()
        }
    }

    pub fn scripted() -> (Session, Arc<Mutex<Shared>>) {
        let shared = Arc::new(Mutex::new(Shared::default()));
        (Session::with_link(Kernels::fake(), None, Box::new(Scripted(shared.clone()))), shared)
    }

    fn stream(text: &str) -> Value {
        serde_json::json!({"output_type": "stream", "name": "stdout", "text": text})
    }

    fn notebook_of(sources: &[&str]) -> Notebook {
        let mut nb = Notebook::new("python3", "Python 3", "python");
        nb.cells[0].source = sources[0].into();
        for (i, source) in sources.iter().enumerate().skip(1) {
            nb.insert_cell(i, CellKind::Code);
            nb.cells[i].source = (*source).into();
        }
        nb
    }

    fn say(shared: &Arc<Mutex<Shared>>, events: Vec<KernelEvent>) {
        shared.lock().unwrap().events.extend(events);
    }

    #[test]
    fn code_asked_for_by_the_window_waits_for_the_cells_and_answers_to_the_caller() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["print(1)"]);
        let uid = nb.cells[0].uid;
        assert!(session.run(&mut nb, &[uid]));
        let token = session.ask("listing".into());
        assert!(session.is_asking());
        // The cell is running, so the listing hasn't been sent.
        session.pump(&mut nb);
        assert_eq!(shared.lock().unwrap().executed, ["print(1)"]);
        say(&shared, vec![KernelEvent::Output { request: 1, output: stream("one\n") }, KernelEvent::Done { request: 1, execution_count: Some(1), ok: true }]);
        session.pump(&mut nb);
        assert_eq!(shared.lock().unwrap().executed, ["print(1)", "listing"], "sent once the cell is done");
        say(&shared, vec![KernelEvent::Output { request: 2, output: stream("part ") }, KernelEvent::Output { request: 2, output: stream("two") }, KernelEvent::Output { request: 2, output: serde_json::json!({"output_type": "stream", "name": "stderr", "text": "warning"}) }, KernelEvent::Done { request: 2, execution_count: Some(1), ok: true }]);
        assert!(!session.pump(&mut nb), "nothing of the notebook changed");
        assert_eq!(session.take_answers(), vec![(token, Ok("part two".to_string()))]);
        assert!(session.take_answers().is_empty() && !session.is_asking());
        assert_eq!(nb.cells[0].execution_count, Some(1), "the listing isn't an execution");
        assert_eq!(nb.cells[0].outputs.len(), 1, "and none of its output went into the cell");
    }

    #[test]
    fn code_asked_for_by_the_window_that_fails_says_why_and_a_restart_drops_the_waiting() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["1"]);
        let first = session.ask("bad".into());
        let second = session.ask("never sent".into());
        session.pump(&mut nb);
        assert_eq!(shared.lock().unwrap().executed, ["bad"], "one at a time");
        say(&shared, vec![KernelEvent::Output { request: 1, output: serde_json::json!({"output_type": "error", "ename": "NameError", "evalue": "name 'x' is not defined", "traceback": []}) }, KernelEvent::Done { request: 1, execution_count: None, ok: false }]);
        session.pump(&mut nb);
        assert_eq!(session.take_answers(), vec![(first, Err("NameError: name 'x' is not defined".to_string()))]);
        assert_eq!(shared.lock().unwrap().executed, ["bad", "never sent"], "the next goes after");
        session.restart();
        assert_eq!(session.take_answers(), vec![(second, Err("The kernel was restarted.".to_string()))]);
        assert!(!session.is_asking());
    }

    #[test]
    fn cells_run_one_at_a_time_in_order_and_each_gets_its_own_output() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["print(1)", "print(2)", "print(3)"]);
        let uids: Vec<u64> = nb.cells.iter().map(|c| c.uid).collect();
        assert!(session.run(&mut nb, &uids));
        assert_eq!(shared.lock().unwrap().executed, vec!["print(1)"], "only the first is sent");
        assert_eq!(session.state_of(uids[0]), CellState::Running);
        assert_eq!(session.state_of(uids[1]), CellState::Queued);
        say(&shared, vec![KernelEvent::Output { request: 1, output: stream("1\n") }, KernelEvent::Done { request: 1, execution_count: Some(1), ok: true }]);
        assert!(session.pump(&mut nb));
        assert_eq!(nb.cells[0].outputs, vec![stream("1\n")]);
        assert_eq!(nb.cells[0].execution_count, Some(1));
        assert_eq!(shared.lock().unwrap().executed, vec!["print(1)", "print(2)"], "the next goes as soon as the last is done");
        say(&shared, vec![KernelEvent::Output { request: 2, output: stream("2\n") }, KernelEvent::Done { request: 2, execution_count: Some(2), ok: true }]);
        session.pump(&mut nb);
        say(&shared, vec![KernelEvent::Output { request: 3, output: stream("3\n") }, KernelEvent::Done { request: 3, execution_count: Some(3), ok: true }]);
        session.pump(&mut nb);
        assert_eq!(nb.cells.iter().map(|c| c.outputs.len()).collect::<Vec<_>>(), vec![1, 1, 1]);
        assert_eq!(nb.cells[2].outputs[0]["text"], "3\n");
        assert!(!session.is_running_anything());
        assert_eq!(session.state_of(uids[2]), CellState::Idle);
    }

    #[test]
    fn running_a_cell_clears_what_it_showed_before_and_its_count() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["1"]);
        nb.cells[0].outputs.push(stream("old"));
        nb.cells[0].execution_count = Some(9);
        let uid = nb.cells[0].uid;
        session.run(&mut nb, &[uid]);
        assert!(nb.cells[0].outputs.is_empty() && nb.cells[0].execution_count.is_none());
        say(&shared, vec![KernelEvent::Done { request: 1, execution_count: Some(10), ok: true }]);
        session.pump(&mut nb);
        assert_eq!(nb.cells[0].execution_count, Some(10));
    }

    #[test]
    fn pieces_of_a_stream_are_joined_and_other_outputs_are_kept_in_order() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["x"]);
        let uid = nb.cells[0].uid;
        session.run(&mut nb, &[uid]);
        let result = serde_json::json!({"output_type": "execute_result", "data": {"text/plain": "2"}, "execution_count": 1, "metadata": {}});
        say(&shared, vec![
            KernelEvent::Output { request: 1, output: stream("a") },
            KernelEvent::Output { request: 1, output: stream("b\n") },
            KernelEvent::Output { request: 1, output: result.clone() },
            KernelEvent::Output { request: 1, output: stream("c") },
        ]);
        session.pump(&mut nb);
        assert_eq!(nb.cells[0].outputs, vec![stream("ab\n"), result, stream("c")]);
    }

    #[test]
    fn an_error_stops_the_cells_queued_after_it() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["1/0", "print(2)"]);
        let uids: Vec<u64> = nb.cells.iter().map(|c| c.uid).collect();
        session.run(&mut nb, &uids);
        let error = serde_json::json!({"output_type": "error", "ename": "ZeroDivisionError", "evalue": "division by zero", "traceback": ["tb"]});
        say(&shared, vec![KernelEvent::Output { request: 1, output: error.clone() }, KernelEvent::Done { request: 1, execution_count: Some(1), ok: false }]);
        session.pump(&mut nb);
        assert_eq!(nb.cells[0].outputs, vec![error]);
        assert_eq!(shared.lock().unwrap().executed.len(), 1, "the second cell never went");
        assert_eq!(session.state_of(uids[1]), CellState::Idle);
    }

    #[test]
    fn interrupt_asks_the_kernel_and_drops_the_queue() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["import time; time.sleep(30)", "print(2)"]);
        let uids: Vec<u64> = nb.cells.iter().map(|c| c.uid).collect();
        session.run(&mut nb, &uids);
        session.interrupt();
        assert_eq!(shared.lock().unwrap().interrupts, 1);
        assert_eq!(session.state_of(uids[1]), CellState::Idle);
        assert_eq!(session.state_of(uids[0]), CellState::Running, "it's the kernel's reply that ends it");
        say(&shared, vec![KernelEvent::Done { request: 1, execution_count: None, ok: false }]);
        session.pump(&mut nb);
        assert_eq!(shared.lock().unwrap().executed.len(), 1);
        assert!(!session.is_running_anything());
    }

    #[test]
    fn restart_forgets_what_was_waiting() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["a", "b"]);
        let uids: Vec<u64> = nb.cells.iter().map(|c| c.uid).collect();
        session.run(&mut nb, &uids);
        session.restart();
        assert_eq!(shared.lock().unwrap().restarts, 1);
        assert!(!session.is_running_anything());
        assert_eq!(session.status, KernelStatus::Starting);
    }

    #[test]
    fn a_question_is_held_until_it_is_answered() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["input('Name? ')"]);
        let uid = nb.cells[0].uid;
        session.run(&mut nb, &[uid]);
        say(&shared, vec![KernelEvent::InputRequest { request: 1, prompt: "Name? ".into(), password: false }]);
        session.pump(&mut nb);
        assert_eq!(session.input, Some(InputAsk { uid, prompt: "Name? ".into(), password: false }));
        session.answer("Ada");
        assert_eq!(shared.lock().unwrap().inputs, vec!["Ada"]);
        assert!(session.input.is_none());
        // An answer when nothing was asked goes nowhere.
        session.answer("again");
        assert_eq!(shared.lock().unwrap().inputs.len(), 1);
    }

    #[test]
    fn clear_output_now_and_after_waiting() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["x"]);
        let uid = nb.cells[0].uid;
        session.run(&mut nb, &[uid]);
        say(&shared, vec![KernelEvent::Output { request: 1, output: stream("a") }, KernelEvent::ClearOutput { request: 1, wait: false }, KernelEvent::Output { request: 1, output: stream("b") }]);
        session.pump(&mut nb);
        assert_eq!(nb.cells[0].outputs, vec![stream("b")]);
        say(&shared, vec![KernelEvent::ClearOutput { request: 1, wait: true }]);
        session.pump(&mut nb);
        assert_eq!(nb.cells[0].outputs, vec![stream("b")], "with wait, nothing goes until something new comes");
        say(&shared, vec![KernelEvent::Output { request: 1, output: stream("c") }]);
        session.pump(&mut nb);
        assert_eq!(nb.cells[0].outputs, vec![stream("c")]);
    }

    #[test]
    fn a_display_is_updated_in_place() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["x"]);
        let uid = nb.cells[0].uid;
        session.run(&mut nb, &[uid]);
        let shown = |text: &str, id: Option<&str>| {
            let mut o = serde_json::json!({"output_type": "display_data", "data": {"text/plain": text}, "metadata": {}});
            if let Some(id) = id {
                o["transient"] = serde_json::json!({"display_id": id});
            }
            o
        };
        say(&shared, vec![KernelEvent::Output { request: 1, output: shown("one", Some("d1")) }, KernelEvent::Output { request: 1, output: stream("between") }, KernelEvent::UpdateDisplay { display_id: "d1".into(), output: shown("two", None) }, KernelEvent::UpdateDisplay { display_id: "nobody".into(), output: shown("x", None) }]);
        session.pump(&mut nb);
        assert_eq!(nb.cells[0].outputs.len(), 2);
        assert_eq!(nb.cells[0].outputs[0]["data"]["text/plain"], "two");
    }

    #[test]
    fn outputs_from_an_old_request_or_a_cell_that_was_deleted_go_nowhere() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["a", "b"]);
        let uid = nb.cells[0].uid;
        session.run(&mut nb, &[uid]);
        nb.delete_cell(0);
        say(&shared, vec![KernelEvent::Output { request: 7, output: stream("stray") }, KernelEvent::Output { request: 1, output: stream("late") }, KernelEvent::Done { request: 1, execution_count: Some(1), ok: true }]);
        session.pump(&mut nb);
        assert!(nb.cells.iter().all(|c| c.outputs.is_empty()));
        assert!(!session.is_running_anything());
    }

    #[test]
    fn a_kernel_that_dies_says_why_and_stops_everything() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["a", "b"]);
        let uids: Vec<u64> = nb.cells.iter().map(|c| c.uid).collect();
        session.run(&mut nb, &uids);
        say(&shared, vec![KernelEvent::Status(KernelStatus::Busy), KernelEvent::Died("it crashed".into())]);
        session.pump(&mut nb);
        assert_eq!(session.status, KernelStatus::Dead);
        assert_eq!(session.problem.as_deref(), Some("it crashed"));
        assert!(!session.is_running_anything());
    }

    #[test]
    fn markdown_cells_and_empty_cells_are_not_sent() {
        let (mut session, shared) = scripted();
        let mut nb = notebook_of(&["", "real", "  \n"]);
        nb.insert_cell(0, CellKind::Markdown);
        nb.cells[0].source = "# text".into();
        nb.cells[1].outputs.push(stream("stale"));
        let uids: Vec<u64> = nb.cells.iter().map(|c| c.uid).collect();
        session.run(&mut nb, &uids);
        assert_eq!(shared.lock().unwrap().executed, vec!["real"]);
        assert!(nb.cells[1].outputs.is_empty(), "an empty cell run shows nothing");
    }

    #[test]
    fn running_without_a_kernel_says_so_rather_than_queueing() {
        let mut session = Session::new(Kernels::fake(), Path::new("."));
        let mut nb = notebook_of(&["1"]);
        let uid = nb.cells[0].uid;
        assert!(!session.run(&mut nb, &[uid]));
        assert!(!session.is_running_anything());
    }

    #[test]
    fn the_kernel_comes_from_the_notebooks_name_then_its_language() {
        let specs = fake_specs();
        let mut nb = Notebook::new("ir", "R", "R");
        assert_eq!(Session::pick_spec(&specs, &nb).map(|s| s.name), Some("ir".to_string()));
        nb.set_kernel("my-own-kernel", "Mine", "python");
        assert_eq!(Session::pick_spec(&specs, &nb).map(|s| s.name), Some("python3".to_string()), "an unknown name falls back to the language");
        nb.set_kernel("my-own-kernel", "Mine", "julia");
        assert_eq!(Session::pick_spec(&specs, &nb), None, "nothing matches, so the person is asked");
        nb.metadata.clear();
        assert_eq!(Session::pick_spec(&specs, &nb), None);
        nb.set_kernel("x", "X", "r");
        assert_eq!(Session::pick_spec(&specs, &nb).map(|s| s.name), Some("ir".to_string()), "language names are compared without case");
    }

    #[test]
    fn a_kernel_started_in_the_background_arrives_by_pump() {
        let kernels = Kernels { start: |spec, _| Ok(Box::new(FakeKernel::new(&spec.language))), list: fake_specs };
        let mut session = Session::new(kernels, Path::new("."));
        let mut nb = notebook_of(&["print('hi')"]);
        let uid = nb.cells[0].uid;
        session.start(fake_specs().remove(0));
        assert_eq!(session.status, KernelStatus::Starting);
        for _ in 0..200 {
            session.pump(&mut nb);
            if session.link.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(session.run(&mut nb, &[uid]));
        for _ in 0..50 {
            session.pump(&mut nb);
        }
        assert_eq!(nb.cells[0].execution_count, Some(1));
        let text = nb.cells[0].outputs[0]["text"].as_str().unwrap();
        assert!(text.contains("print('hi')") && text.contains("nothing was run"), "{text}");
        assert_eq!(session.status, KernelStatus::Idle);
    }

    #[test]
    fn a_cell_runs_on_a_real_python_kernel_when_there_is_one() {
        let Some(spec) = crate::jupyter::kernel_specs().into_iter().find(|s| s.language.eq_ignore_ascii_case("python")) else {
            println!("No Python kernel here, so the notebook wasn't run on a real one.");
            return;
        };
        let kernels = Kernels { start: crate::jupyter::start, list: crate::jupyter::kernel_specs };
        let mut session = Session::new(kernels, Path::new("."));
        let mut nb = notebook_of(&["print('hi'); 6 * 7"]);
        let uid = nb.cells[0].uid;
        session.start(spec);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut asked = false;
        while std::time::Instant::now() < deadline && nb.cells[0].execution_count.is_none() {
            session.pump(&mut nb);
            if !asked && session.link.is_some() {
                asked = session.run(&mut nb, &[uid]);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // The rest of the output arrives just after the count, on another socket.
        for _ in 0..50 {
            session.pump(&mut nb);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(nb.cells[0].execution_count, Some(1), "{:?}", session.problem);
        let outputs = serde_json::to_string(&nb.cells[0].outputs).unwrap();
        assert!(outputs.contains("hi\\n") && outputs.contains("42"), "{outputs}");
    }

    #[test]
    fn a_kernel_that_fails_to_start_says_why() {
        let kernels = Kernels { start: |_, _| Err("no such program".into()), list: fake_specs };
        let mut session = Session::new(kernels, Path::new("."));
        let mut nb = notebook_of(&["1"]);
        session.start(fake_specs().remove(0));
        for _ in 0..200 {
            session.pump(&mut nb);
            if session.problem.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(session.problem.as_deref(), Some("no such program"));
        assert_eq!(session.status, KernelStatus::Dead);
    }

    #[test]
    fn dropping_the_session_shuts_the_kernel_down() {
        let (session, shared) = scripted();
        drop(session);
        assert!(shared.lock().unwrap().shut_down);
    }
}
