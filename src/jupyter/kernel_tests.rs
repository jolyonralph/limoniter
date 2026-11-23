//! The kernel link against a fake kernel built from the ZMTP and wire pieces (run everywhere), and against real
//! ipykernel and IRkernel kernels where this computer has them (skipped with a note where it hasn't).

use super::kernel::{Channel, Kernel, Tracker};
use super::link::{KernelEvent, KernelLink, KernelStatus};
use super::process::{ConnectionInfo, Launcher, Process};
use super::wire::{Message, Session};
use super::zmtp::{self, SocketType};
use serde_json::{Value, json};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

// Long, because a kernel starting while every other test runs can take many times its usual second.
const WAIT: Duration = Duration::from_secs(90);

/// What one fake kernel has been through, for the tests to look at.
#[derive(Default)]
struct FakeState {
    ended: AtomicBool,
    interrupted: AtomicBool,
    asked_to_shut_down: AtomicBool,
    last_words: Mutex<String>,
    iopub: Mutex<Option<zmtp::Sender>>,
    stdin: Mutex<Option<zmtp::Sender>>,
    sockets: Mutex<Vec<zmtp::Sender>>,
}

impl FakeState {
    fn end(&self) {
        self.ended.store(true, Ordering::SeqCst);
        for socket in self.sockets.lock().unwrap().iter() {
            socket.close();
        }
    }
}

/// Starts fake kernels, each a few threads listening on the connection file's ports.
#[derive(Default)]
struct FakeLauncher {
    launches: AtomicU32,
    /// How many starts are to fail as if another program had taken a port.
    clashes: AtomicU32,
    states: Mutex<Vec<Arc<FakeState>>>,
}

struct FakeProcess(Arc<FakeState>);

impl Process for FakeProcess {
    fn ended(&mut self) -> Option<String> {
        self.0.ended.load(Ordering::SeqCst).then(|| "fake ended".to_string())
    }
    fn interrupt(&mut self) {
        self.0.interrupted.store(true, Ordering::SeqCst);
    }
    fn kill(&mut self) {
        self.0.end();
    }
    fn last_words(&self) -> String {
        let words = self.0.last_words.lock().unwrap();
        if words.is_empty() { "fake: died".into() } else { words.clone() }
    }
}

/// Accepts connections on a port as this kind of socket, handing each to `then` on a thread of its own.
fn listen(listener: TcpListener, kind: SocketType, state: Arc<FakeState>, then: impl Fn(zmtp::Sender, zmtp::Receiver) + Send + Sync + 'static) {
    let then = Arc::new(then);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if state.ended.load(Ordering::SeqCst) {
                return;
            }
            let (state, then) = (state.clone(), then.clone());
            std::thread::spawn(move || {
                if let Ok((sender, receiver, _)) = zmtp::handshake(stream, kind, None, true, zmtp::MINOR, Duration::from_secs(5)) {
                    state.sockets.lock().unwrap().push(sender.clone());
                    then(sender, receiver);
                }
            });
        }
    });
}

/// A message to `parent` from the fake.
fn reply(session: &Session, msg_type: &str, content: Value, parent: &Message) -> Vec<Vec<u8>> {
    session.encode(&session.make_msg(msg_type, content, parent.header.clone()))
}

fn publish(state: &FakeState, session: &Session, msg_type: &str, content: Value, parent: &Message) {
    if let Some(iopub) = state.iopub.lock().unwrap().as_ref() {
        let mut message = session.make_msg(msg_type, content, parent.header.clone());
        message.ids = vec![format!("kernel.fake.{msg_type}").into_bytes()];
        let _ = iopub.send(&session.encode(&message));
    }
}

impl Launcher for FakeLauncher {
    fn launch(&self, info: &ConnectionInfo, _file: &Path, held: Vec<TcpListener>) -> Result<Box<dyn Process>, String> {
        // The ports are listened on as they were held, in the order shell, iopub, stdin, control, heartbeat.
        let [shell, iopub, stdin, control, hb]: [TcpListener; 5] = held.try_into().unwrap();
        assert_eq!(shell.local_addr().unwrap().port(), info.shell_port);
        self.launches.fetch_add(1, Ordering::SeqCst);
        let state = Arc::new(FakeState::default());
        self.states.lock().unwrap().push(state.clone());
        if self.clashes.load(Ordering::SeqCst) > 0 {
            self.clashes.fetch_sub(1, Ordering::SeqCst);
            *state.last_words.lock().unwrap() = "zmq.error.ZMQError: Address already in use (addr='tcp://127.0.0.1:1')".into();
            state.ended.store(true, Ordering::SeqCst);
            return Ok(Box::new(FakeProcess(state)));
        }
        let session = Arc::new(Session::new(&info.key));
        let (input_tx, input_rx) = mpsc::channel::<String>();
        let input_rx = Arc::new(Mutex::new(input_rx));

        listen(hb, SocketType::Router, state.clone(), |sender, mut receiver| {
            while let Ok(frames) = receiver.recv() {
                let _ = sender.send(&frames);
            }
        });
        let s = state.clone();
        listen(iopub, SocketType::XPub, state.clone(), move |sender, mut receiver| {
            // Nothing is published until the subscription has come, as with a real PUB.
            if let Ok(zmtp::Incoming::Command(name, _)) = receiver.recv_any() {
                assert_eq!(name, "SUBSCRIBE");
                *s.iopub.lock().unwrap() = Some(sender);
            }
        });
        let (s, sess) = (state.clone(), session.clone());
        let input_tx = Mutex::new(input_tx);
        listen(stdin, SocketType::Router, state.clone(), move |sender, mut receiver| {
            *s.stdin.lock().unwrap() = Some(sender);
            while let Ok(frames) = receiver.recv() {
                let message = sess.decode(frames).unwrap();
                assert_eq!(message.msg_type(), "input_reply");
                let _ = input_tx.lock().unwrap().send(message.content["value"].as_str().unwrap().to_string());
            }
        });
        let (s, sess) = (state.clone(), session.clone());
        listen(control, SocketType::Router, state.clone(), move |sender, mut receiver| {
            while let Ok(frames) = receiver.recv() {
                let message = sess.decode(frames).unwrap();
                match message.msg_type() {
                    "interrupt_request" => {
                        s.interrupted.store(true, Ordering::SeqCst);
                        let _ = sender.send(&reply(&sess, "interrupt_reply", json!({"status": "ok"}), &message));
                    }
                    "shutdown_request" => {
                        s.asked_to_shut_down.store(true, Ordering::SeqCst);
                        let _ = sender.send(&reply(&sess, "shutdown_reply", message.content.clone(), &message));
                        s.end();
                    }
                    _ => {}
                }
            }
        });
        let (s, sess) = (state.clone(), session.clone());
        let count = Arc::new(AtomicU32::new(0));
        listen(shell, SocketType::Router, state.clone(), move |sender, mut receiver| {
            while let Ok(frames) = receiver.recv() {
                let message = sess.decode(frames).unwrap();
                let publish = |msg_type: &str, content: Value| publish(&s, &sess, msg_type, content, &message);
                publish("status", json!({"execution_state": "busy"}));
                match message.msg_type() {
                    "kernel_info_request" => {
                        let info = json!({"status": "ok", "protocol_version": "5.3", "implementation": "fake", "language_info": {"name": "fakelang"}});
                        let _ = sender.send(&reply(&sess, "kernel_info_reply", info, &message));
                    }
                    "complete_request" => {
                        let content = json!({"status": "ok", "matches": ["print"], "cursor_start": 0, "cursor_end": message.content["cursor_pos"]});
                        let _ = sender.send(&reply(&sess, "complete_reply", content, &message));
                    }
                    "execute_request" => {
                        let n = count.fetch_add(1, Ordering::SeqCst) + 1;
                        let code = message.content["code"].as_str().unwrap().to_string();
                        publish("execute_input", json!({"code": code, "execution_count": n}));
                        let mut status = "ok";
                        let mut payload = json!([]);
                        match code.as_str() {
                            "input" => {
                                if let Some(stdin) = s.stdin.lock().unwrap().as_ref() {
                                    let _ = stdin.send(&reply(&sess, "input_request", json!({"prompt": "Name? ", "password": false}), &message));
                                }
                                let name = input_rx.lock().unwrap().recv_timeout(WAIT).unwrap();
                                publish("stream", json!({"name": "stdout", "text": format!("hello {name}\n")}));
                            }
                            "error" => {
                                publish("error", json!({"ename": "ValueError", "evalue": "bad", "traceback": ["one", "two"]}));
                                status = "error";
                            }
                            "sleep" => {
                                let start = Instant::now();
                                while !s.interrupted.swap(false, Ordering::SeqCst) && start.elapsed() < WAIT && !s.ended.load(Ordering::SeqCst) {
                                    std::thread::sleep(Duration::from_millis(10));
                                }
                                publish("error", json!({"ename": "KeyboardInterrupt", "evalue": "", "traceback": []}));
                                status = "error";
                            }
                            "display" => {
                                publish("display_data", json!({"data": {"text/plain": "first"}, "metadata": {}, "transient": {"display_id": "d1"}}));
                                publish("update_display_data", json!({"data": {"text/plain": "second"}, "metadata": {}, "transient": {"display_id": "d1"}}));
                                publish("clear_output", json!({"wait": true}));
                            }
                            "page" => payload = json!([{"source": "page", "data": {"text/plain": "Help!"}, "start": 0}]),
                            "forged" => {
                                // Signed with another key, so it must be passed over.
                                let forger = Session::new("not the key");
                                if let Some(iopub) = s.iopub.lock().unwrap().as_ref() {
                                    let _ = iopub.send(&forger.encode(&forger.make_msg("stream", json!({"name": "stdout", "text": "forged"}), message.header.clone())));
                                }
                                publish("stream", json!({"name": "stdout", "text": "genuine"}));
                            }
                            "die" => {
                                s.end();
                                return;
                            }
                            _ => {
                                publish("stream", json!({"name": "stdout", "text": format!("out: {code}\n")}));
                                publish("execute_result", json!({"execution_count": n, "data": {"text/plain": "42"}, "metadata": {}}));
                            }
                        }
                        let content = json!({"status": status, "execution_count": n, "payload": payload, "user_expressions": {}});
                        let _ = sender.send(&reply(&sess, "execute_reply", content, &message));
                    }
                    _ => {}
                }
                publish("status", json!({"execution_state": "idle"}));
            }
        });
        Ok(Box::new(FakeProcess(state)))
    }
}

fn fake_kernel(message_interrupts: bool) -> (Kernel, Arc<FakeLauncher>) {
    let launcher = Arc::new(FakeLauncher::default());
    let kernel = Kernel::with_launcher(launcher.clone(), "fake", "fakelang", message_interrupts).unwrap();
    (kernel, launcher)
}

/// Polls until `until` is true of an event, giving every event seen.
fn events_until(kernel: &mut dyn KernelLink, mut until: impl FnMut(&KernelEvent) -> bool) -> Vec<KernelEvent> {
    let start = Instant::now();
    let mut seen = Vec::new();
    while start.elapsed() < WAIT {
        for event in kernel.poll() {
            let stop = until(&event);
            seen.push(event);
            if stop {
                return seen;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("waited too long; saw {seen:#?}");
}

fn until_done(kernel: &mut dyn KernelLink, request: u64) -> Vec<KernelEvent> {
    events_until(kernel, |e| matches!(e, KernelEvent::Done { request: r, .. } if *r == request))
}

fn outputs(events: &[KernelEvent], request: u64) -> Vec<Value> {
    events
        .iter()
        .filter_map(|e| match e {
            KernelEvent::Output { request: r, output } if *r == request => Some(output.clone()),
            _ => None,
        })
        .collect()
}

fn done(events: &[KernelEvent]) -> (Option<u32>, bool) {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            KernelEvent::Done { execution_count, ok, .. } => Some((*execution_count, *ok)),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_fake_kernel_runs_code_given_before_it_was_ready() {
    let (mut kernel, _) = fake_kernel(false);
    assert_eq!(kernel.status(), KernelStatus::Starting);
    let request = kernel.execute("1+1");
    let events = until_done(&mut kernel, request);
    assert_eq!(events.first(), Some(&KernelEvent::Status(KernelStatus::Starting)));
    assert!(events.contains(&KernelEvent::Status(KernelStatus::Idle)));
    assert_eq!(
        outputs(&events, request),
        vec![
            json!({"output_type": "stream", "name": "stdout", "text": "out: 1+1\n"}),
            json!({"output_type": "execute_result", "execution_count": 1, "data": {"text/plain": "42"}, "metadata": {}}),
        ]
    );
    assert_eq!(done(&events), (Some(1), true));
    assert_eq!(kernel.language(), "fakelang");
    assert_eq!(kernel.info().unwrap()["implementation"], "fake");

    // Then errors, input, display updates, help pages and forged messages.
    let request = kernel.execute("error");
    let events = until_done(&mut kernel, request);
    assert_eq!(outputs(&events, request), vec![json!({"output_type": "error", "ename": "ValueError", "evalue": "bad", "traceback": ["one", "two"]})]);
    assert_eq!(done(&events), (Some(2), false));

    let request = kernel.execute("input");
    let events = events_until(&mut kernel, |e| matches!(e, KernelEvent::InputRequest { .. }));
    assert_eq!(events.last(), Some(&KernelEvent::InputRequest { request, prompt: "Name? ".into(), password: false }));
    kernel.send_input("Ada");
    let events = until_done(&mut kernel, request);
    assert_eq!(outputs(&events, request)[0]["text"], "hello Ada\n");

    let request = kernel.execute("display");
    let events = until_done(&mut kernel, request);
    assert_eq!(outputs(&events, request), vec![json!({"output_type": "display_data", "data": {"text/plain": "first"}, "metadata": {}, "transient": {"display_id": "d1"}})]);
    let second = json!({"output_type": "display_data", "data": {"text/plain": "second"}, "metadata": {}, "transient": {"display_id": "d1"}});
    assert!(events.contains(&KernelEvent::UpdateDisplay { display_id: "d1".into(), output: second }));
    assert!(events.contains(&KernelEvent::ClearOutput { request, wait: true }));

    let request = kernel.execute("page");
    let events = until_done(&mut kernel, request);
    assert_eq!(outputs(&events, request), vec![json!({"output_type": "display_data", "data": {"text/plain": "Help!"}, "metadata": {}})]);

    let request = kernel.execute("forged");
    let events = until_done(&mut kernel, request);
    assert_eq!(outputs(&events, request), vec![json!({"output_type": "stream", "name": "stdout", "text": "genuine"})]);

    let request = kernel.complete("pri", 3);
    let start = Instant::now();
    let reply = loop {
        kernel.poll();
        if let Some(reply) = kernel.reply(request) {
            break reply;
        }
        assert!(start.elapsed() < WAIT);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(reply["matches"], json!(["print"]));
    assert_eq!(reply["cursor_end"], 3);
}

#[test]
fn a_fake_kernel_is_interrupted_by_signal_or_message() {
    for message_interrupts in [false, true] {
        let (mut kernel, launcher) = fake_kernel(message_interrupts);
        let request = kernel.execute("sleep");
        // Once the reply to the first command has come in, the code is surely running.
        events_until(&mut kernel, |e| e == &KernelEvent::Status(KernelStatus::Idle));
        std::thread::sleep(Duration::from_millis(100));
        kernel.interrupt();
        let events = until_done(&mut kernel, request);
        assert_eq!(outputs(&events, request)[0]["ename"], "KeyboardInterrupt");
        assert!(!done(&events).1);
        assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn a_fake_kernel_restarts_afresh() {
    let (mut kernel, launcher) = fake_kernel(false);
    let request = kernel.execute("a");
    assert_eq!(done(&until_done(&mut kernel, request)), (Some(1), true));
    // Something running when the restart comes is finished, unsuccessfully.
    let sleeping = kernel.execute("sleep");
    kernel.restart().unwrap();
    let events = events_until(&mut kernel, |e| e == &KernelEvent::Status(KernelStatus::Idle));
    assert!(events.contains(&KernelEvent::Done { request: sleeping, execution_count: None, ok: false }));
    assert!(events.contains(&KernelEvent::Status(KernelStatus::Starting)));
    let first = launcher.states.lock().unwrap()[0].clone();
    assert!(first.asked_to_shut_down.load(Ordering::SeqCst));
    assert!(first.ended.load(Ordering::SeqCst));
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 2);
    // A new kernel counts from one again.
    let request = kernel.execute("b");
    assert_eq!(done(&until_done(&mut kernel, request)), (Some(1), true));
}

#[test]
fn a_fake_kernel_that_dies_says_so() {
    let (mut kernel, _) = fake_kernel(false);
    let request = kernel.execute("die");
    let events = events_until(&mut kernel, |e| matches!(e, KernelEvent::Died(_)));
    let Some(KernelEvent::Died(why)) = events.last() else { unreachable!() };
    assert!(why.contains("fake ended") && why.ends_with("fake: died"), "{why}");
    assert!(events.contains(&KernelEvent::Done { request, execution_count: None, ok: false }));
    assert_eq!(kernel.status(), KernelStatus::Dead);
    // It can be brought back.
    kernel.restart().unwrap();
    events_until(&mut kernel, |e| e == &KernelEvent::Status(KernelStatus::Idle));
}

#[test]
fn a_fake_kernel_is_shut_down_with_a_request() {
    let (mut kernel, launcher) = fake_kernel(false);
    let request = kernel.execute("a");
    until_done(&mut kernel, request);
    kernel.shutdown();
    assert_eq!(kernel.status(), KernelStatus::Dead);
    let request = kernel.execute("b");
    let events = kernel.poll();
    assert!(events.contains(&KernelEvent::Done { request, execution_count: None, ok: false }));
    assert!(kernel.restart().is_err());
    let state = launcher.states.lock().unwrap()[0].clone();
    let start = Instant::now();
    while !state.ended.load(Ordering::SeqCst) {
        assert!(start.elapsed() < WAIT);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(state.asked_to_shut_down.load(Ordering::SeqCst));
}

#[test]
fn a_kernel_whose_port_was_taken_is_started_again() {
    let launcher = Arc::new(FakeLauncher::default());
    launcher.clashes.store(2, Ordering::SeqCst);
    let mut kernel = Kernel::with_launcher(launcher.clone(), "fake", "fakelang", false).unwrap();
    let request = kernel.execute("a");
    let events = until_done(&mut kernel, request);
    assert_eq!(done(&events), (Some(1), true));
    assert!(!events.iter().any(|e| matches!(e, KernelEvent::Died(_))));
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 3);
    // More than three clashes in a row is reported.
    let launcher = Arc::new(FakeLauncher::default());
    launcher.clashes.store(10, Ordering::SeqCst);
    let mut kernel = Kernel::with_launcher(launcher.clone(), "fake", "fakelang", false).unwrap();
    let events = events_until(&mut kernel, |e| matches!(e, KernelEvent::Died(_)));
    assert!(matches!(events.last(), Some(KernelEvent::Died(why)) if why.contains("Address already in use")));
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 4);
}

#[test]
fn a_launch_that_fails_is_an_error() {
    struct Fails;
    impl Launcher for Fails {
        fn launch(&self, _: &ConnectionInfo, file: &Path, _: Vec<TcpListener>) -> Result<Box<dyn Process>, String> {
            assert!(file.is_file());
            Err("no such program".into())
        }
    }
    assert_eq!(Kernel::with_launcher(Arc::new(Fails), "x", "x", false).err().unwrap(), "no such program");
}

/// The tracker alone, given messages in the orders the channels may deliver them.
#[test]
fn the_tracker_waits_for_both_reply_and_idle() {
    let session = Session::new("k");
    let request_message = session.make_msg("execute_request", json!({}), json!({}));
    let answer = |msg_type: &str, content: Value| session.make_msg(msg_type, content, request_message.header.clone());
    let now = Instant::now();
    for idle_first in [false, true] {
        let mut tracker = Tracker::default();
        let mut out = Vec::new();
        tracker.ready(&mut out);
        tracker.sent(request_message.msg_id(), 7, true);
        let reply = answer("execute_reply", json!({"status": "ok", "execution_count": 3}));
        let idle = answer("status", json!({"execution_state": "idle"}));
        let (first, second) = if idle_first { ((Channel::Iopub, &idle), (Channel::Shell, &reply)) } else { ((Channel::Shell, &reply), (Channel::Iopub, &idle)) };
        tracker.handle(first.0, first.1, now, &mut out);
        assert!(!out.iter().any(|e| matches!(e, KernelEvent::Done { .. })));
        tracker.handle(second.0, second.1, now, &mut out);
        assert_eq!(out.last(), Some(&KernelEvent::Done { request: 7, execution_count: Some(3), ok: true }));
    }

    // An idle that never comes is waited for only a while.
    let mut tracker = Tracker::default();
    let mut out = Vec::new();
    tracker.sent(request_message.msg_id(), 8, true);
    tracker.handle(Channel::Shell, &answer("execute_reply", json!({"status": "error", "execution_count": 4})), now, &mut out);
    tracker.tick(now + Duration::from_millis(100), &mut out);
    assert!(out.is_empty());
    tracker.tick(now + Duration::from_secs(3), &mut out);
    assert_eq!(out, vec![KernelEvent::Done { request: 8, execution_count: Some(4), ok: false }]);

    // An aborted request finishes at once; messages for others' requests are passed over.
    let mut tracker = Tracker::default();
    let mut out = Vec::new();
    tracker.sent(request_message.msg_id(), 9, true);
    let stranger = session.make_msg("stream", json!({"name": "stdout", "text": "?"}), session.header("execute_request"));
    tracker.handle(Channel::Iopub, &stranger, now, &mut out);
    tracker.handle(Channel::Shell, &answer("execute_reply", json!({"status": "aborted"})), now, &mut out);
    assert_eq!(out, vec![KernelEvent::Done { request: 9, execution_count: None, ok: false }]);
    // Before the kernel is ready, kernel_info's busy and idle don't change the status.
    tracker.handle(Channel::Iopub, &answer("status", json!({"execution_state": "busy"})), now, &mut out);
    assert_eq!(tracker.status, KernelStatus::Starting);
}

// Real kernels. Each skips, saying so, when this computer hasn't one.

fn real_spec(language: &str) -> Option<super::link::KernelSpec> {
    let spec = super::specs::kernel_specs().into_iter().find(|s| s.language.eq_ignore_ascii_case(language));
    if spec.is_none() {
        println!("No {language} Jupyter kernel is installed here, so this test was skipped.");
    }
    spec
}

fn run(kernel: &mut dyn KernelLink, code: &str) -> (Vec<Value>, (Option<u32>, bool)) {
    let request = kernel.execute(code);
    let mut events = until_done(kernel, request);
    // The reply can come before the last of the output, which travels on another socket: the kernel says it's idle
    // only after all of it, so that's waited for too (for a few seconds at most).
    let done_at = events.iter().rposition(|e| matches!(e, KernelEvent::Done { .. })).unwrap_or(0);
    let began = Instant::now();
    while !events[done_at..].iter().any(|e| e == &KernelEvent::Status(KernelStatus::Idle)) && began.elapsed() < Duration::from_secs(5) {
        events.extend(kernel.poll());
        std::thread::sleep(Duration::from_millis(5));
    }
    if let Some(KernelEvent::Died(why)) = events.iter().find(|e| matches!(e, KernelEvent::Died(_))) {
        panic!("the kernel died running {code:?}: {why}");
    }
    (outputs(&events, request), done(&events))
}

#[test]
fn a_real_ipykernel_runs_prints_plots_fails_asks_and_is_interrupted() {
    let Some(spec) = real_spec("python") else { return };
    let mut kernel = super::start(&spec, &std::env::temp_dir()).unwrap();

    let (out, (count, ok)) = run(kernel.as_mut(), "print(1); 1+1; import sys; sys.stderr.write(\"e\")");
    assert!(ok, "{out:?}");
    assert_eq!(count, Some(1));
    assert_eq!(kernel.language(), "python");
    let text = |name: &str| out.iter().filter(|o| o["name"] == name).map(|o| o["text"].as_str().unwrap().to_string()).collect::<String>();
    assert_eq!(text("stdout"), "1\n");
    assert_eq!(text("stderr"), "e");
    // The last line's value (sys.stderr.write gives 1) is the result.
    assert_eq!(out.iter().find(|o| o["output_type"] == "execute_result").unwrap()["data"]["text/plain"], "1");

    let (out, (_, ok)) = run(kernel.as_mut(), "%matplotlib inline\nimport matplotlib.pyplot as plt\nplt.plot([1, 2, 3])\nplt.show()");
    if ok {
        let png = out.iter().find_map(|o| o["data"]["image/png"].as_str()).expect("a PNG");
        // Base64 for the PNG signature.
        assert!(png.starts_with("iVBORw0KGgo"), "{}", &png[..20.min(png.len())]);
    } else {
        println!("matplotlib isn't installed with this kernel, so the plot wasn't tried: {out:?}");
    }

    let (out, (_, ok)) = run(kernel.as_mut(), "def f():\n    raise ValueError('bad')\nf()");
    assert!(!ok);
    let error = out.iter().find(|o| o["output_type"] == "error").unwrap();
    assert_eq!(error["ename"], "ValueError");
    assert_eq!(error["evalue"], "bad");
    assert!(error["traceback"].as_array().unwrap().len() >= 2);

    let request = kernel.execute("name = input('Who? ')\nprint('hi', name)");
    let events = events_until(kernel.as_mut(), |e| matches!(e, KernelEvent::InputRequest { .. }));
    assert!(matches!(events.last(), Some(KernelEvent::InputRequest { prompt, password: false, .. }) if prompt == "Who? "));
    kernel.send_input("Ada");
    let events = until_done(kernel.as_mut(), request);
    assert!(outputs(&events, request).iter().any(|o| o["text"] == "hi Ada\n"), "{events:?}");

    let request = kernel.execute("while True:\n    pass");
    events_until(kernel.as_mut(), |e| e == &KernelEvent::Status(KernelStatus::Busy));
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    kernel.interrupt();
    let events = until_done(kernel.as_mut(), request);
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(outputs(&events, request).iter().any(|o| o["ename"] == "KeyboardInterrupt"), "{events:?}");

    run(kernel.as_mut(), "kept = 5");
    kernel.restart().unwrap();
    events_until(kernel.as_mut(), |e| e == &KernelEvent::Status(KernelStatus::Idle));
    let (out, (count, ok)) = run(kernel.as_mut(), "kept");
    assert!(!ok);
    assert_eq!(count, Some(1));
    assert_eq!(out.iter().find(|o| o["output_type"] == "error").unwrap()["ename"], "NameError");
    kernel.shutdown();
}

#[test]
fn a_real_ipykernel_takes_interrupts_by_message_and_completes() {
    let Some(spec) = real_spec("python") else { return };
    let launcher = Arc::new(super::process::SpawnLauncher { spec: spec.clone(), cwd: std::env::temp_dir() });
    let mut kernel = Kernel::with_launcher(launcher, &spec.name, &spec.language, true).unwrap();
    let request = kernel.execute("while True:\n    pass");
    events_until(&mut kernel, |e| e == &KernelEvent::Status(KernelStatus::Busy));
    std::thread::sleep(Duration::from_millis(300));
    kernel.interrupt();
    let events = until_done(&mut kernel, request);
    assert!(outputs(&events, request).iter().any(|o| o["ename"] == "KeyboardInterrupt"), "{events:?}");
    let request = kernel.complete("import sy", 9);
    let start = Instant::now();
    let reply = loop {
        kernel.poll();
        if let Some(reply) = kernel.reply(request) {
            break reply;
        }
        assert!(start.elapsed() < WAIT);
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(reply["matches"].as_array().unwrap().iter().any(|m| m == "sys"), "{reply}");
    std::thread::sleep(Duration::from_secs(11));
    let (out, (_, ok)) = run(&mut kernel, "2 * 3");
    assert!(ok, "{out:?}");
}

#[test]
fn a_real_irkernel_runs_r() {
    let Some(spec) = real_spec("R") else { return };
    let mut kernel = super::start(&spec, &std::env::temp_dir()).unwrap();
    let (out, (count, ok)) = run(kernel.as_mut(), "cat('hello\\n'); 1 + 1");
    assert!(ok, "{out:?}");
    // IRkernel now and then counts something of its own first on a busy machine, so any count will do here.
    assert!(count.is_some_and(|n| n >= 1), "{count:?} {out:?}");
    assert_eq!(kernel.language(), "R");
    assert!(out.iter().any(|o| o["text"] == "hello\n"), "{out:?}");
    assert!(out.iter().any(|o| o["data"]["text/plain"].as_str().is_some_and(|t| t.contains('2'))), "{out:?}");
    let (out, (_, ok)) = run(kernel.as_mut(), "stop('bad')");
    assert!(!ok);
    assert!(out.iter().any(|o| o["output_type"] == "error"), "{out:?}");
    // Idle for longer than three heartbeats, it must still be taken to be alive.
    std::thread::sleep(Duration::from_secs(11));
    let (out, (_, ok)) = run(kernel.as_mut(), "2 * 3");
    assert!(ok, "{out:?}");
    assert_eq!(kernel.status(), KernelStatus::Idle);
}

#[test]
fn a_real_ipykernel_installs_a_small_package_with_the_code_the_install_box_sends() {
    use crate::notebook_install::{Tongue, install_code, outcome};
    let Some(spec) = real_spec("python") else { return };
    let mut kernel = super::start(&spec, &std::env::temp_dir()).unwrap();
    let (out, (_, ok)) = run(kernel.as_mut(), &install_code(Tongue::Python, &["tabulate".to_string()]));
    let printed: String = out.iter().filter(|o| o["output_type"] == "stream").map(|o| o["text"].as_str().unwrap_or("").to_string()).collect();
    let failure = out.iter().find(|o| o["output_type"] == "error").map(|o| format!("{}: {}", o["ename"], o["evalue"]));
    let result = outcome(&printed, failure.as_deref());
    if let Err(why) = &result {
        let offline = ["Connection", "connection", "Max retries", "Temporary failure", "Name or service", "No matching distribution", "Could not find a version", "timed out"].iter().any(|w| printed.contains(w) || why.contains(w));
        if offline {
            println!("There is no network here (or the index can't be reached), so the real install was skipped: {why}");
            kernel.shutdown();
            return;
        }
    }
    assert!(ok && result.is_ok(), "{result:?}\n{printed}");
    // The package went where this very kernel looks, so it can be used at once, with no restart.
    let (out, (_, ok)) = run(kernel.as_mut(), "import tabulate\nprint(tabulate.__name__)");
    assert!(ok, "{out:?}");
    assert!(out.iter().any(|o| o["text"] == "tabulate\n"), "{out:?}");
    // A name that doesn't exist fails in the way the box recognises, though the cell itself doesn't fail.
    let (out, _) = run(kernel.as_mut(), &install_code(Tongue::Python, &["limoniter-no-such-package-xyz".to_string()]));
    let printed: String = out.iter().filter(|o| o["output_type"] == "stream").map(|o| o["text"].as_str().unwrap_or("").to_string()).collect();
    assert!(outcome(&printed, None).is_err(), "{printed}");
    kernel.shutdown();
}
