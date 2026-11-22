//! A running Jupyter kernel, reached over its five channels: shell (requests and their replies), iopub (what the
//! code prints and shows, and whether the kernel is busy), stdin (the code asking for typed input), control
//! (interrupts and shutdown) and heartbeat (whether it's still there).
//!
//! Threads do the waiting. One starts the kernel's program, connects, asks `kernel_info` until the kernel answers
//! on both shell and iopub (asking again until iopub speaks, as jupyter_client's "nudge" does, since a SUB can
//! miss what's published before its subscription arrives), and then keeps a heartbeat and watches for the
//! program ending. One for each channel reads messages. All of them hand what they find to the `Kernel`, whose
//! `poll` turns it into `KernelEvent`s on the window's own thread.

use super::link::{KernelEvent, KernelLink, KernelSpec, KernelStatus};
use super::process::{self, ConnectionInfo, Launcher, Process, SpawnLauncher};
use super::wire::{Message, Session};
use super::zmtp::{self, SocketType};
// The channels by their bare names, so a message type beside one isn't taken for a label to translate.
use Channel::{Iopub, Shell, Stdin};
use crate::i18n::{tf, tr};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// How long a kernel has to start and answer.
const START_TIME: Duration = Duration::from_secs(60);
/// How often the heartbeat is sent, and how long its echo may take.
const HEARTBEAT: Duration = Duration::from_secs(3);
/// Missed heartbeats in a row, while the kernel isn't busy, before it's taken to be dead.
const MISSED_BEATS: u32 = 3;
/// How long a kernel asked to shut down has to end before it's killed, as jupyter_client's shutdown_wait_time.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);
/// How long after a request's reply its last outputs may still arrive on iopub (before its "idle").
const IDLE_WAIT: Duration = Duration::from_secs(2);

/// The kernel's channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Shell,
    Control,
    Stdin,
    Iopub,
}

/// The window's egui context, woken when a kernel has something to say.
static REPAINT: Mutex<Option<eframe::egui::Context>> = Mutex::new(None);

/// Lets kernels wake the window when something arrives, rather than waiting for its next frame.
pub fn set_repaint(ctx: &eframe::egui::Context) {
    *REPAINT.lock().unwrap_or_else(|e| e.into_inner()) = Some(ctx.clone());
}

fn repaint() {
    if let Some(ctx) = REPAINT.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        ctx.request_repaint();
    }
}

/// What the threads tell the `Kernel`, each with the generation (the start it belongs to) so that word from a
/// kernel since restarted is passed over.
enum Wire {
    /// Connected and answering: these are the channels to send on.
    Ready(Live),
    Message(Channel, Box<Message>),
    Died(String),
}

type Tagged = (u64, Wire);

/// The channels requests go out on.
struct Live {
    shell: zmtp::Sender,
    control: zmtp::Sender,
    stdin: zmtp::Sender,
}

/// One start of the kernel's program.
struct Run {
    process: Arc<Mutex<Box<dyn Process>>>,
    /// Set when the link is done with this run, so its threads stop and its ending isn't reported as a death.
    stop: Arc<AtomicBool>,
    /// Every connection, to close at the end.
    sockets: Arc<Mutex<Vec<zmtp::Sender>>>,
    file: PathBuf,
}

/// What the `Kernel` shares with the thread that restarts it.
struct Slot {
    generation: u64,
    gone: bool,
    run: Option<Run>,
}

/// Everything a run's threads need to start a kernel.
#[derive(Clone)]
struct Starter {
    launcher: Arc<dyn Launcher>,
    session: Arc<Session>,
    name: String,
    tx: mpsc::Sender<Tagged>,
}

/// A kernel, as the notebook talks to it.
pub struct Kernel {
    starter: Starter,
    spec_language: String,
    message_interrupts: bool,
    slot: Arc<Mutex<Slot>>,
    generation: u64,
    rx: mpsc::Receiver<Tagged>,
    live: Option<Live>,
    /// Requests made before the kernel could take them.
    queue: Vec<(Channel, Message)>,
    tracker: Tracker,
    next_request: u64,
    /// Events made outside `poll` (by restart and shutdown), given at the next.
    waiting: Vec<KernelEvent>,
    gone: bool,
    /// Starts made again because a port was taken, since the last time the kernel was ready.
    relaunches: u32,
}

/// Starts a kernel from its kernelspec, in this folder. It returns once the program has started; the kernel says
/// it's ready with `Status(Idle)` and requests made before then wait for it.
pub fn start_kernel(spec: &KernelSpec, cwd: &Path) -> Result<Kernel, String> {
    let launcher = Arc::new(SpawnLauncher { spec: spec.clone(), cwd: cwd.to_path_buf() });
    Kernel::with_launcher(launcher, &spec.name, &spec.language, super::specs::interrupt_mode(spec) == "message")
}

impl Kernel {
    /// A kernel started by this launcher (the tests give one of their own).
    pub fn with_launcher(launcher: Arc<dyn Launcher>, name: &str, language: &str, message_interrupts: bool) -> Result<Kernel, String> {
        let (tx, rx) = mpsc::channel();
        let starter = Starter { launcher, session: Arc::new(Session::new(&super::wire::new_key())), name: name.to_string(), tx };
        let run = begin(&starter, 0)?;
        Ok(Kernel {
            starter,
            spec_language: language.to_string(),
            message_interrupts,
            slot: Arc::new(Mutex::new(Slot { generation: 0, gone: false, run: Some(run) })),
            generation: 0,
            rx,
            live: None,
            queue: Vec::new(),
            tracker: Tracker::default(),
            next_request: 0,
            waiting: vec![KernelEvent::Status(KernelStatus::Starting)],
            gone: false,
            relaunches: 0,
        })
    }

    fn send_request(&mut self, channel: Channel, msg_type: &str, content: Value, execute: bool) -> u64 {
        self.next_request += 1;
        let request = self.next_request;
        let message = self.starter.session.make_msg(msg_type, content, json!({}));
        if self.gone {
            if execute {
                self.waiting.push(KernelEvent::Done { request, execution_count: None, ok: false });
            }
            return request;
        }
        self.tracker.sent(message.msg_id(), request, execute);
        self.send(channel, message);
        request
    }

    fn send(&mut self, channel: Channel, message: Message) {
        match &self.live {
            Some(live) => {
                let sender = match channel {
                    Channel::Shell | Channel::Iopub => &live.shell,
                    Channel::Control => &live.control,
                    Channel::Stdin => &live.stdin,
                };
                // A failed send means the kernel has gone, which the watching thread reports.
                let _ = sender.send(&self.starter.session.encode(&message));
            }
            None => self.queue.push((channel, message)),
        }
    }

    /// Asks for ways to finish the code at `cursor_pos` (counted in characters, as protocol 5.2 counts). The
    /// `complete_reply`'s content comes from `reply` once it's in.
    #[allow(dead_code)] // For the `# %%` cells and the R console, which come next in 0.16.
    pub fn complete(&mut self, code: &str, cursor_pos: usize) -> u64 {
        self.send_request(Shell, "complete_request", json!({"code": code, "cursor_pos": cursor_pos}), false)
    }

    /// Asks about what's at `cursor_pos` (its help, with `detail_level` 1 its source too). The `inspect_reply`'s
    /// content comes from `reply`.
    #[allow(dead_code)] // For the `# %%` cells and the R console, which come next in 0.16.
    pub fn inspect(&mut self, code: &str, cursor_pos: usize, detail_level: u8) -> u64 {
        self.send_request(Shell, "inspect_request", json!({"code": code, "cursor_pos": cursor_pos, "detail_level": detail_level}), false)
    }

    /// The reply to a `complete` or `inspect`, once it has come (after a `poll`), given once.
    #[allow(dead_code)] // For the `# %%` cells and the R console, which come next in 0.16.
    pub fn reply(&mut self, request: u64) -> Option<Value> {
        self.tracker.replies.remove(&request)
    }

    /// The kernel's `kernel_info_reply`, once it has answered.
    #[allow(dead_code)] // For the `# %%` cells and the R console, which come next in 0.16.
    pub fn info(&self) -> Option<&Value> {
        self.tracker.info.as_ref()
    }

    /// Ends this run and starts no other: `shutdown_request` now, then a thread waits for the program to end and
    /// kills it if it takes too long.
    fn end_run(&mut self, restart: bool) -> Option<(Run, Option<zmtp::Sender>)> {
        let run = {
            let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
            slot.generation += 1;
            slot.run.take()
        };
        self.generation += 1;
        let control = self.live.take().map(|live| live.control);
        let run = run?;
        run.stop.store(true, Ordering::SeqCst);
        // A kernel reads its connection file only as it starts, so it goes now, even if Limoniter is closing
        // and the thread that waits for the kernel to end doesn't get to finish.
        let _ = std::fs::remove_file(&run.file);
        if let Some(control) = &control {
            let message = self.starter.session.make_msg("shutdown_request", json!({"restart": restart}), json!({}));
            let _ = control.send(&self.starter.session.encode(&message));
        }
        Some((run, control))
    }

    /// Ends this run (asking it to shut down when it's connected) and starts the program again on a thread, with
    /// fresh ports. Requests waiting in the queue go to the new run.
    fn relaunch(&mut self, restart: bool) -> Result<(), String> {
        let old = self.end_run(restart);
        let generation = self.generation;
        let (starter, slot) = (self.starter.clone(), self.slot.clone());
        std::thread::Builder::new()
            .name("jupyter-restart".into())
            .spawn(move || {
                if let Some((run, control)) = old {
                    finish(run, control.is_some());
                }
                match begin(&starter, generation) {
                    Ok(run) => {
                        let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
                        if slot.generation == generation && !slot.gone {
                            slot.run = Some(run);
                        } else {
                            // Restarted or shut down again meanwhile: this run isn't wanted.
                            drop(slot);
                            finish(run, false);
                        }
                    }
                    Err(why) => {
                        let _ = starter.tx.send((generation, Wire::Died(why)));
                        repaint();
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Starts the program and the thread that connects to it and watches it.
fn begin(starter: &Starter, generation: u64) -> Result<Run, String> {
    let (info, held) = ConnectionInfo::new(starter.session.key(), &starter.name)?;
    let file = process::connection_file(&starter.session.id, generation);
    process::write_connection_file(&info, &file)?;
    let process = match starter.launcher.launch(&info, &file, held) {
        Ok(process) => Arc::new(Mutex::new(process)),
        Err(e) => {
            let _ = std::fs::remove_file(&file);
            return Err(e);
        }
    };
    let run = Run { process, stop: Arc::new(AtomicBool::new(false)), sockets: Arc::new(Mutex::new(Vec::new())), file };
    let (process, stop, sockets, starter) = (run.process.clone(), run.stop.clone(), run.sockets.clone(), starter.clone());
    std::thread::Builder::new()
        .name("jupyter-kernel".into())
        .spawn(move || watch(generation, info, starter, process, stop, sockets))
        .map_err(|e| e.to_string())?;
    Ok(run)
}

/// Why a kernel's program stopped, with its last words.
fn died_message(how: &str, last_words: &str) -> String {
    let mut text = tf!("The kernel stopped ({how}).");
    if !last_words.trim().is_empty() {
        text.push_str("\n\n");
        text.push_str(last_words.trim_end());
    }
    text
}

/// The thread for one run: connect, wait for the kernel to answer, then keep a heartbeat until the end.
fn watch(generation: u64, info: ConnectionInfo, starter: Starter, process: Arc<Mutex<Box<dyn Process>>>, stop: Arc<AtomicBool>, sockets: Arc<Mutex<Vec<zmtp::Sender>>>) {
    let tell = |wire: Wire| {
        let _ = starter.tx.send((generation, wire));
        repaint();
    };
    let ended = || {
        let mut process = process.lock().unwrap_or_else(|e| e.into_inner());
        process.ended().map(|how| {
            // Its last lines may still be on their way through the pipe.
            std::thread::sleep(Duration::from_millis(100));
            died_message(&how, &process.last_words())
        })
    };
    let identity = starter.session.id.as_bytes().to_vec();
    let deadline = Instant::now() + START_TIME;
    let connect = |port: u16, kind: SocketType| -> Result<(zmtp::Sender, zmtp::Receiver), Option<String>> {
        loop {
            if stop.load(Ordering::SeqCst) {
                return Err(None);
            }
            if let Some(why) = ended() {
                return Err(Some(why));
            }
            match zmtp::connect(ConnectionInfo::address(port), kind, Some(&identity), Duration::from_secs(2)) {
                Ok((sender, receiver, _)) => {
                    sockets.lock().unwrap_or_else(|e| e.into_inner()).push(sender.clone());
                    return Ok((sender, receiver));
                }
                Err(e) if Instant::now() > deadline => return Err(Some(tf!("Couldn't reach the kernel: {e}"))),
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    };
    type Pair = (zmtp::Sender, zmtp::Receiver);
    let connected = (|| -> Result<(Pair, Pair, Pair, Pair, Pair), Option<String>> {
        let iopub = connect(info.iopub_port, SocketType::Sub)?;
        iopub.0.subscribe(b"").map_err(|e| Some(e.to_string()))?;
        Ok((iopub, connect(info.shell_port, SocketType::Dealer)?, connect(info.control_port, SocketType::Dealer)?, connect(info.stdin_port, SocketType::Dealer)?, connect(info.hb_port, SocketType::Req)?))
    })();
    let (iopub, shell, control, stdin, mut heartbeat) = match connected {
        Ok(sockets) => sockets,
        Err(why) => {
            if let Some(why) = why.filter(|_| !stop.load(Ordering::SeqCst)) {
                tell(Wire::Died(why));
            }
            return;
        }
    };

    // The readers. They also note what the start and the heartbeat need to know.
    let answered = Arc::new(AtomicBool::new(false));
    let published = Arc::new(AtomicBool::new(false));
    let busy = Arc::new(AtomicBool::new(false));
    for (channel, mut receiver) in [(Channel::Iopub, iopub.1), (Channel::Shell, shell.1), (Channel::Control, control.1), (Channel::Stdin, stdin.1)] {
        let (tx, session, stop) = (starter.tx.clone(), starter.session.clone(), stop.clone());
        let (answered, published, busy) = (answered.clone(), published.clone(), busy.clone());
        let _ = std::thread::Builder::new().name("jupyter-channel".into()).spawn(move || {
            while let Ok(frames) = receiver.recv() {
                // A message that isn't signed with this kernel's key is passed over, as jupyter_client does.
                let Ok(message) = session.decode(frames) else { continue };
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                match (channel, message.msg_type()) {
                    (Iopub, "status") => {
                        published.store(true, Ordering::SeqCst);
                        busy.store(message.content["execution_state"] == "busy", Ordering::SeqCst);
                    }
                    (Channel::Iopub, _) => published.store(true, Ordering::SeqCst),
                    (Shell, "kernel_info_reply") => answered.store(true, Ordering::SeqCst),
                    _ => {}
                }
                if tx.send((generation, Wire::Message(channel, Box::new(message)))).is_err() {
                    break;
                }
                repaint();
            }
        });
    }

    // kernel_info, asked again each second until both shell and iopub have answered.
    let mut asked: Option<Instant> = None;
    while !(answered.load(Ordering::SeqCst) && published.load(Ordering::SeqCst)) {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        if let Some(why) = ended() {
            tell(Wire::Died(why));
            return;
        }
        if Instant::now() > deadline {
            tell(Wire::Died(tr("The kernel started but didn't answer.").to_string()));
            return;
        }
        if asked.is_none_or(|at| at.elapsed() > Duration::from_secs(1)) {
            let message = starter.session.make_msg("kernel_info_request", json!({}), json!({}));
            let _ = shell.0.send(&starter.session.encode(&message));
            asked = Some(Instant::now());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    tell(Wire::Ready(Live { shell: shell.0, control: control.0, stdin: stdin.0 }));

    // The heartbeat, and watching for the end.
    let mut missed = 0;
    let mut next_beat = Instant::now() + HEARTBEAT;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        if stop.load(Ordering::SeqCst) {
            return;
        }
        if let Some(why) = ended() {
            if !stop.load(Ordering::SeqCst) {
                tell(Wire::Died(why));
            }
            return;
        }
        if Instant::now() < next_beat {
            continue;
        }
        next_beat = Instant::now() + HEARTBEAT;
        let (sender, receiver) = &mut heartbeat;
        let beat = receiver.set_timeout(Some(HEARTBEAT)).is_ok() && sender.send(&[b"ping"]).is_ok() && receiver.recv().is_ok_and(|echo| echo.len() == 1 && echo[0] == b"ping");
        if beat {
            missed = 0;
            continue;
        }
        // Some kernels (IRkernel) answer the heartbeat only between runs, so a busy kernel's silence is no sign.
        if !busy.load(Ordering::SeqCst) {
            missed += 1;
        }
        if missed >= MISSED_BEATS {
            tell(Wire::Died(tr("The kernel stopped answering.").to_string()));
            return;
        }
        // A REQ that missed its answer can't ask again, so the heartbeat starts afresh.
        // If it can't be reached, the closed one stays and the beats go on being missed.
        sender.close();
        if let Ok((fresh_sender, fresh_receiver, _)) = zmtp::connect(ConnectionInfo::address(info.hb_port), SocketType::Req, None, Duration::from_secs(2)) {
            sockets.lock().unwrap_or_else(|e| e.into_inner()).push(fresh_sender.clone());
            heartbeat = (fresh_sender, fresh_receiver);
        }
    }
}

/// The end of a run, on its own thread: wait for the program to end after its `shutdown_request` (killing it
/// when it takes too long, or at once when it never connected), close the connections and remove the
/// connection file.
fn finish(run: Run, asked: bool) {
    run.stop.store(true, Ordering::SeqCst);
    let deadline = Instant::now() + if asked { SHUTDOWN_WAIT } else { Duration::ZERO };
    loop {
        let mut process = run.process.lock().unwrap_or_else(|e| e.into_inner());
        if process.ended().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            process.kill();
            break;
        }
        drop(process);
        std::thread::sleep(Duration::from_millis(50));
    }
    for socket in run.sockets.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        socket.close();
    }
    let _ = std::fs::remove_file(&run.file);
}

impl KernelLink for Kernel {
    fn execute(&mut self, code: &str) -> u64 {
        let content = json!({"code": code, "silent": false, "store_history": true, "user_expressions": {}, "allow_stdin": true, "stop_on_error": true});
        self.send_request(Shell, "execute_request", content, true)
    }

    fn execute_quietly(&mut self, code: &str) -> u64 {
        let content = json!({"code": code, "silent": false, "store_history": false, "user_expressions": {}, "allow_stdin": false, "stop_on_error": false});
        self.send_request(Shell, "execute_request", content, true)
    }

    fn poll(&mut self) -> Vec<KernelEvent> {
        let mut out = std::mem::take(&mut self.waiting);
        while let Ok((generation, wire)) = self.rx.try_recv() {
            if generation != self.generation {
                continue;
            }
            match wire {
                Wire::Ready(live) => {
                    for (channel, message) in std::mem::take(&mut self.queue) {
                        let sender = match channel {
                            Channel::Shell | Channel::Iopub => &live.shell,
                            Channel::Control => &live.control,
                            Channel::Stdin => &live.stdin,
                        };
                        let _ = sender.send(&self.starter.session.encode(&message));
                    }
                    self.live = Some(live);
                    self.tracker.ready(&mut out);
                    self.relaunches = 0;
                }
                Wire::Message(channel, message) => self.tracker.handle(channel, &message, Instant::now(), &mut out),
                // Another program took one of the kernel's ports between their being chosen and its starting
                // (jupyter_client has the same gap), so it's started again on others, keeping what's waiting.
                Wire::Died(why) if !self.tracker.is_ready() && why.contains("Address already in use") && self.relaunches < 3 => {
                    self.relaunches += 1;
                    if let Err(why) = self.relaunch(false) {
                        self.queue.clear();
                        self.tracker.died(why, &mut out);
                    }
                }
                Wire::Died(why) => {
                    self.live = None;
                    self.queue.clear();
                    self.tracker.died(why, &mut out);
                }
            }
        }
        self.tracker.tick(Instant::now(), &mut out);
        out
    }

    fn status(&self) -> KernelStatus {
        self.tracker.status
    }

    fn interrupt(&mut self) {
        if self.message_interrupts {
            if let Some(live) = &self.live {
                let message = self.starter.session.make_msg("interrupt_request", json!({}), json!({}));
                let _ = live.control.send(&self.starter.session.encode(&message));
            }
        } else if let Some(run) = &self.slot.lock().unwrap_or_else(|e| e.into_inner()).run {
            run.process.lock().unwrap_or_else(|e| e.into_inner()).interrupt();
        }
    }

    /// As jupyter_client's restart_kernel: `shutdown_request` with restart set, a wait for the program to end
    /// (killing it after five seconds), then a fresh start. This returns at once; the rest happens on a thread,
    /// and the kernel says `Status(Idle)` again once it's back, or `Died` if it couldn't start.
    fn restart(&mut self) -> Result<(), String> {
        if self.gone {
            return Err(tr("The kernel has been shut down.").to_string());
        }
        self.queue.clear();
        self.tracker.restarting(&mut self.waiting);
        self.relaunch(true)
    }

    fn send_input(&mut self, text: &str) {
        if let Some(parent) = self.tracker.input_parent.take() {
            let message = self.starter.session.make_msg("input_reply", json!({"value": text}), parent);
            self.send(Channel::Stdin, message);
        }
    }

    fn shutdown(&mut self) {
        if self.gone {
            return;
        }
        self.gone = true;
        self.slot.lock().unwrap_or_else(|e| e.into_inner()).gone = true;
        let old = self.end_run(false);
        self.queue.clear();
        self.tracker.stopped(&mut self.waiting);
        self.tracker.set_status(KernelStatus::Dead, &mut self.waiting);
        if let Some((run, control)) = old {
            let _ = std::thread::Builder::new().name("jupyter-shutdown".into()).spawn(move || finish(run, control.is_some()));
        }
    }

    fn language(&self) -> String {
        self.tracker.info.as_ref().and_then(|info| info["language_info"]["name"].as_str()).map(str::to_string).unwrap_or_else(|| self.spec_language.clone())
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// An execute request not yet finished: finished takes both its reply on shell and its "idle" on iopub, since
/// the two channels may deliver in either order and outputs can come until the idle.
#[derive(Default)]
struct Running {
    reply: Option<(Option<u32>, bool)>,
    replied_at: Option<Instant>,
    idle: bool,
}

/// Turns the kernel's messages into `KernelEvent`s, keeping what it must between them. It has no threads or
/// sockets, so the tests can give it messages directly.
pub struct Tracker {
    pub status: KernelStatus,
    ready: bool,
    /// Each request's msg_id, to its number.
    requests: HashMap<String, u64>,
    running: BTreeMap<u64, Running>,
    pub replies: HashMap<u64, Value>,
    /// The header of the last `input_request`, which its reply answers.
    pub input_parent: Option<Value>,
    pub info: Option<Value>,
}

impl Default for Tracker {
    fn default() -> Tracker {
        Tracker { status: KernelStatus::Starting, ready: false, requests: HashMap::new(), running: BTreeMap::new(), replies: HashMap::new(), input_parent: None, info: None }
    }
}

/// An nbformat `display_data` output from a display_data or update_display_data message. Its `transient`
/// (holding the `display_id`) is kept, so the notebook can find it again for `UpdateDisplay`; nbformat doesn't
/// save `transient`, so it's to be left out when the notebook is saved.
fn display_output(content: &Value) -> Value {
    let mut output = json!({"output_type": "display_data", "data": object_or_empty(&content["data"]), "metadata": object_or_empty(&content["metadata"])});
    if content["transient"]["display_id"].is_string() {
        output["transient"] = json!({"display_id": content["transient"]["display_id"]});
    }
    output
}

fn object_or_empty(value: &Value) -> Value {
    if value.is_object() { value.clone() } else { json!({}) }
}

impl Tracker {
    pub fn set_status(&mut self, status: KernelStatus, out: &mut Vec<KernelEvent>) {
        if self.status != status {
            self.status = status;
            out.push(KernelEvent::Status(status));
        }
    }

    pub fn sent(&mut self, msg_id: &str, request: u64, execute: bool) {
        self.requests.insert(msg_id.to_string(), request);
        if execute {
            self.running.insert(request, Running::default());
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// The kernel has answered and requests can go.
    pub fn ready(&mut self, out: &mut Vec<KernelEvent>) {
        self.ready = true;
        self.set_status(KernelStatus::Idle, out);
    }

    fn finish(&mut self, request: u64, out: &mut Vec<KernelEvent>) {
        if let Some(running) = self.running.remove(&request) {
            let (execution_count, ok) = running.reply.unwrap_or((None, false));
            out.push(KernelEvent::Done { request, execution_count, ok });
        }
    }

    pub fn handle(&mut self, channel: Channel, message: &Message, now: Instant, out: &mut Vec<KernelEvent>) {
        let request = self.requests.get(message.parent_id()).copied();
        let content = &message.content;
        match (channel, message.msg_type()) {
            (Iopub, "status") => {
                let status = match content["execution_state"].as_str() {
                    Some("busy") => KernelStatus::Busy,
                    Some("idle") => KernelStatus::Idle,
                    _ => return,
                };
                // While starting, kernel_info's own busy and idle aren't news.
                if self.ready {
                    self.set_status(status, out);
                }
                if status == KernelStatus::Idle
                    && let Some(request) = request
                    && let Some(running) = self.running.get_mut(&request)
                {
                    running.idle = true;
                    if running.reply.is_some() {
                        self.finish(request, out);
                    }
                }
            }
            (Iopub, "update_display_data") => {
                if let Some(display_id) = content["transient"]["display_id"].as_str() {
                    out.push(KernelEvent::UpdateDisplay { display_id: display_id.to_string(), output: display_output(content) });
                }
            }
            (Channel::Iopub, kind) => {
                let Some(request) = request else { return };
                let output = match kind {
                    "stream" => json!({"output_type": "stream", "name": content["name"].as_str().unwrap_or("stdout"), "text": content["text"].as_str().unwrap_or_default()}),
                    "display_data" => display_output(content),
                    "execute_result" => json!({
                        "output_type": "execute_result",
                        "execution_count": content["execution_count"],
                        "data": object_or_empty(&content["data"]),
                        "metadata": object_or_empty(&content["metadata"]),
                    }),
                    "error" => json!({
                        "output_type": "error",
                        "ename": content["ename"].as_str().unwrap_or_default(),
                        "evalue": content["evalue"].as_str().unwrap_or_default(),
                        "traceback": if content["traceback"].is_array() { content["traceback"].clone() } else { json!([]) },
                    }),
                    "clear_output" => {
                        out.push(KernelEvent::ClearOutput { request, wait: content["wait"].as_bool().unwrap_or(false) });
                        return;
                    }
                    _ => return,
                };
                out.push(KernelEvent::Output { request, output });
            }
            (Shell, "kernel_info_reply") => self.info = Some(content.clone()),
            (Shell, "execute_reply") => {
                let Some(request) = request.filter(|r| self.running.contains_key(r)) else { return };
                // Help asked for with `?` comes back as a "page" payload, shown with the outputs as JupyterLab does.
                for payload in content["payload"].as_array().into_iter().flatten().filter(|p| p["source"] == "page") {
                    out.push(KernelEvent::Output { request, output: json!({"output_type": "display_data", "data": object_or_empty(&payload["data"]), "metadata": {}}) });
                }
                let status = content["status"].as_str().unwrap_or_default();
                let count = content["execution_count"].as_u64().and_then(|c| u32::try_from(c).ok());
                if let Some(running) = self.running.get_mut(&request) {
                    running.reply = Some((count, status == "ok"));
                    running.replied_at = Some(now);
                    // A request skipped after an earlier error gets no idle of its own.
                    if running.idle || status == "aborted" {
                        self.finish(request, out);
                    }
                }
            }
            (Channel::Shell, _) => {
                if let Some(request) = request {
                    self.replies.insert(request, content.clone());
                }
            }
            (Stdin, "input_request") => {
                let Some(request) = request else { return };
                self.input_parent = Some(message.header.clone());
                out.push(KernelEvent::InputRequest { request, prompt: content["prompt"].as_str().unwrap_or_default().to_string(), password: content["password"].as_bool().unwrap_or(false) });
            }
            _ => {}
        }
    }

    /// Finishes requests whose reply came a while ago but whose idle never did.
    pub fn tick(&mut self, now: Instant, out: &mut Vec<KernelEvent>) {
        let late: Vec<u64> = self.running.iter().filter(|(_, r)| r.replied_at.is_some_and(|at| now.duration_since(at) > IDLE_WAIT)).map(|(&request, _)| request).collect();
        for request in late {
            self.finish(request, out);
        }
    }

    /// Every unfinished request has failed, since the kernel has gone.
    pub fn stopped(&mut self, out: &mut Vec<KernelEvent>) {
        for request in self.running.keys().copied().collect::<Vec<_>>() {
            self.finish(request, out);
        }
        self.input_parent = None;
        self.ready = false;
    }

    pub fn restarting(&mut self, out: &mut Vec<KernelEvent>) {
        self.stopped(out);
        self.info = None;
        self.set_status(KernelStatus::Starting, out);
    }

    pub fn died(&mut self, why: String, out: &mut Vec<KernelEvent>) {
        self.stopped(out);
        self.set_status(KernelStatus::Dead, out);
        out.push(KernelEvent::Died(why));
    }
}
