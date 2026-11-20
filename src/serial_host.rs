//! The serial terminal's window: choosing the port (or the machine on the network), what came in shown as
//! lines, as a screen or as hex, a line to type into, a log file, and XMODEM and YMODEM transfers of the file in
//! front or of files chosen, with what's received opened in the editor or saved.

use super::*;
use crate::serial_link::{self as link, Chars, Ending, Flow, Kind, Lines, LogKind, Parity, Session, View};
use crate::xmodem::{Incoming, Outgoing, Protocol};
use std::collections::VecDeque;

const TITLE: &str = "Serial terminal";

/// One row of the hex view: up to sixteen bytes that went one way at one time.
struct HexRow {
    burst: u64,
    time: String,
    incoming: bool,
    bytes: Vec<u8>,
}

/// The most rows the hex view keeps.
const MOST_HEX_ROWS: usize = 200_000;

pub(super) struct SerialWindow {
    session: Option<Session>,
    ports: Vec<String>,
    lines: Lines,
    screen: vt100::Parser,
    hex: VecDeque<HexRow>,
    /// How far into the traffic the views have read.
    seen: (u64, usize),
    /// The character set and echo the lines were made with; changing either makes them again.
    made_with: (Chars, bool),
    typed: String,
    typed_hex: bool,
    /// XMODEM's padding taken off what's received.
    trim: bool,
    log_kind: LogKind,
    /// Something to say: a problem, or how a transfer ended.
    note: Option<String>,
    received: Vec<Incoming>,
    /// The speed being typed, where it isn't one of the usual ones.
    speed_text: String,
}

impl SerialWindow {
    fn new(prefs: &link::Prefs) -> SerialWindow {
        SerialWindow {
            session: None,
            ports: link::ports(),
            lines: Lines::new(prefs.chars),
            screen: vt100::Parser::new(25, 80, 2000),
            hex: VecDeque::new(),
            seen: (0, 0),
            made_with: (prefs.chars, prefs.echo),
            typed: String::new(),
            typed_hex: false,
            trim: true,
            log_kind: LogKind::Raw,
            note: None,
            received: Vec::new(),
            speed_text: prefs.speed.to_string(),
        }
    }

    /// Reads what's new in the traffic into the three views. A new character set or echo setting makes the
    /// lines again from everything still kept.
    fn catch_up(&mut self, prefs: &link::Prefs) {
        let Some(session) = &self.session else { return };
        let traffic = link::lock(&session.shared.traffic);
        if self.made_with != (prefs.chars, prefs.echo) {
            self.made_with = (prefs.chars, prefs.echo);
            self.lines = Lines::new(prefs.chars);
            for burst in traffic.bursts.iter().filter(|b| b.incoming || prefs.echo) {
                self.lines.feed(&burst.bytes, burst.at);
            }
        }
        let (new, place) = traffic.since(self.seen);
        for (burst, bytes) in new {
            if burst.incoming || prefs.echo {
                self.lines.feed(bytes, burst.at);
            }
            if burst.incoming {
                self.screen.process(bytes);
            }
            let mut bytes = bytes;
            // The rest of a row the burst began.
            if let Some(last) = self.hex.back_mut().filter(|r| r.burst == burst.id && r.bytes.len() < 16) {
                let room = (16 - last.bytes.len()).min(bytes.len());
                last.bytes.extend_from_slice(&bytes[..room]);
                bytes = &bytes[room..];
            }
            let time = burst.at.format("%H:%M:%S%.3f").to_string();
            for row in bytes.chunks(16) {
                self.hex.push_back(HexRow { burst: burst.id, time: time.clone(), incoming: burst.incoming, bytes: row.to_vec() });
            }
            while self.hex.len() > MOST_HEX_ROWS {
                self.hex.pop_front();
            }
        }
        self.seen = place;
    }

    /// Bytes shown as if they had come in, with no connection: for a screenshot of the window at work.
    pub(super) fn show_sample(&mut self, bytes: &[u8]) {
        let at = chrono::Local::now();
        self.lines.feed(bytes, at);
        self.screen.process(bytes);
        let time = at.format("%H:%M:%S%.3f").to_string();
        for row in bytes.chunks(16) {
            self.hex.push_back(HexRow { burst: u64::MAX, time: time.clone(), incoming: true, bytes: row.to_vec() });
        }
    }

    fn clear(&mut self, prefs: &link::Prefs) {
        self.lines = Lines::new(prefs.chars);
        self.screen = vt100::Parser::new(25, 80, 2000);
        self.hex.clear();
        if let Some(session) = &self.session {
            let mut traffic = link::lock(&session.shared.traffic);
            // Kept traffic goes too, so that changing the character set doesn't bring it back.
            traffic.bursts.clear();
        }
    }
}

/// What the window asked for, done once it's drawn.
enum Asked {
    Connect,
    Disconnect,
    Send(Vec<u8>),
    SendFront,
    SendChosen,
    Receive,
    StartLog,
    StopLog,
    Clear,
    Open(usize),
    Save(usize),
}

impl Limoniter {
    pub(super) fn open_serial_terminal(&mut self) {
        if self.serial.is_none() {
            self.serial = Some(Box::new(SerialWindow::new(&self.serial_prefs)));
        }
    }

    pub(super) fn serial_window(&mut self, ctx: &Context) {
        let Some(mut window) = self.serial.take() else { return };
        let before = self.serial_prefs.clone();
        let mut prefs = self.serial_prefs.clone();
        window.catch_up(&prefs);
        let ultimates: Vec<String> = self.sends.devices.iter().filter_map(|d| if let crate::deploy::Place::Ultimate { host } = &d.place { Some(host.clone()) } else { None }).collect();
        let mut asked = Vec::new();
        let mut open = true;
        egui::Window::new(crate::i18n::tr(TITLE)).open(&mut open).resizable(true).default_size([820.0, 560.0]).show(ctx, |ui| {
            draw_serial(ui, &mut window, &mut prefs, &ultimates, &mut asked);
        });
        for asked in asked {
            self.serial_asked(&mut window, &mut prefs, asked);
        }
        if window.session.as_ref().is_some_and(|s| s.shared.changed.swap(false, std::sync::atomic::Ordering::Relaxed)) {
            ctx.request_repaint();
        }
        if window.session.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        self.serial_prefs = prefs;
        if self.serial_prefs != before {
            self.save_settings_if_changed();
        }
        if open {
            self.serial = Some(window);
        }
    }

    fn serial_asked(&mut self, window: &mut SerialWindow, prefs: &mut link::Prefs, asked: Asked) {
        match asked {
            Asked::Connect => match link::open(prefs) {
                Ok(connection) => {
                    window.session = Some(Session::start(connection, prefs.describe()));
                    window.seen = (0, 0);
                    window.made_with = (prefs.chars, prefs.echo);
                    window.note = None;
                }
                Err(problem) => window.note = Some(problem),
            },
            Asked::Disconnect => {
                window.session = None;
                window.note = None;
            }
            Asked::Send(bytes) => {
                if let Some(session) = &window.session {
                    session.send(bytes);
                }
            }
            Asked::SendFront => {
                if self.no_file() {
                    window.note = Some(crate::i18n::tr("Open the file to send first.").into());
                    return;
                }
                let doc = &self.docs[self.active];
                let name = doc.path_or_name().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file.bin".into());
                let modified = doc.path.as_deref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
                let data = self.document_bytes();
                if let Some(session) = &window.session {
                    session.send_files(prefs.protocol, vec![Outgoing { name, data, modified }]);
                }
                window.note = None;
            }
            Asked::SendChosen => {
                let picker = rfd::FileDialog::new().set_title(crate::i18n::tr("Files to send"));
                let chosen = if prefs.protocol == Protocol::Ymodem { picker.pick_files().unwrap_or_default() } else { picker.pick_file().into_iter().collect() };
                if chosen.is_empty() {
                    return;
                }
                let mut files = Vec::new();
                for path in chosen {
                    match std::fs::read(&path) {
                        Ok(data) => {
                            let modified = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
                            files.push(Outgoing { name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), data, modified });
                        }
                        Err(e) => {
                            window.note = Some(crate::i18n::tf!("Couldn't read {}: {e}", path.display()));
                            return;
                        }
                    }
                }
                if let Some(session) = &window.session {
                    session.send_files(prefs.protocol, files);
                }
                window.note = None;
            }
            Asked::Receive => {
                if let Some(session) = &window.session {
                    session.receive_files(prefs.protocol, window.trim);
                }
                window.received.clear();
                window.note = None;
            }
            Asked::StartLog => {
                let Some(session) = &window.session else { return };
                let ending = if window.log_kind == LogKind::Raw { "bin" } else { "txt" };
                let picker = rfd::FileDialog::new().set_title(crate::i18n::tr("Log to a file")).set_file_name(format!("serial-{}.{ending}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
                if let Some(path) = picker.save_file()
                    && let Err(problem) = session.log_to(Some((&path, window.log_kind)))
                {
                    window.note = Some(problem);
                }
            }
            Asked::StopLog => {
                if let Some(session) = &window.session {
                    let _ = session.log_to(None);
                }
            }
            Asked::Clear => window.clear(prefs),
            Asked::Open(index) => {
                if let Some(file) = window.received.get(index) {
                    let name = file.name.clone().unwrap_or_else(|| "received.bin".into());
                    self.open_bytes_as_new(&name, file.data.clone());
                }
            }
            Asked::Save(index) => {
                if let Some(file) = window.received.get(index) {
                    let picker = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the file received")).set_file_name(file.name.clone().unwrap_or_else(|| "received.bin".into()));
                    if let Some(path) = picker.save_file()
                        && let Err(e) = std::fs::write(&path, &file.data)
                    {
                        window.note = Some(crate::i18n::tf!("Couldn't write to {}: {e}", path.display()));
                    }
                }
            }
        }
    }
}

/// The connection's choices: the port and its format, or the machine and its port.
fn draw_connection(ui: &mut Ui, window: &mut SerialWindow, prefs: &mut link::Prefs, ultimates: &[String]) {
    ui.horizontal_wrapped(|ui| {
        ui.radio_value(&mut prefs.kind, Kind::Serial, crate::i18n::tr("Serial port"));
        ui.radio_value(&mut prefs.kind, Kind::Network, crate::i18n::tr("Network"));
        ui.separator();
        match prefs.kind {
            Kind::Serial => {
                egui::ComboBox::from_id_salt("serial-port").selected_text(if prefs.port.is_empty() { crate::i18n::tr("Choose\u{2026}").to_string() } else { prefs.port.clone() }).width(150.0).show_ui(ui, |ui| {
                    if window.ports.is_empty() {
                        ui.label(crate::i18n::tr("No serial ports were found. Type the port's name in the box beside."));
                    }
                    for port in &window.ports {
                        ui.selectable_value(&mut prefs.port, port.clone(), port);
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut prefs.port).hint_text(if cfg!(windows) { "COM3" } else { "/dev/ttyUSB0" }).desired_width(110.0)).on_hover_text(crate::i18n::tr("The port's name, typed: a pseudo-terminal or a port not listed"));
                if ui.button(crate::i18n::tr("Look again")).on_hover_text(crate::i18n::tr("List the serial ports again, after plugging one in")).clicked() {
                    window.ports = link::ports();
                }
                ui.label(crate::i18n::tr("Speed"));
                egui::ComboBox::from_id_salt("serial-speed").selected_text(prefs.speed.to_string()).width(80.0).show_ui(ui, |ui| {
                    for speed in link::SPEEDS {
                        if ui.selectable_value(&mut prefs.speed, speed, speed.to_string()).clicked() {
                            window.speed_text = speed.to_string();
                        }
                    }
                });
                if ui.add(egui::TextEdit::singleline(&mut window.speed_text).desired_width(56.0)).on_hover_text(crate::i18n::tr("Any other speed, in bits a second")).changed()
                    && let Ok(speed) = window.speed_text.trim().parse::<u32>()
                    && speed > 0
                {
                    prefs.speed = speed;
                }
            }
            Kind::Network => {
                ui.label(crate::i18n::tr("Machine"));
                ui.add(egui::TextEdit::singleline(&mut prefs.host).hint_text("192.168.1.64").desired_width(150.0));
                ui.label(crate::i18n::tr("Port"));
                ui.add(egui::DragValue::new(&mut prefs.tcp_port).range(1..=65535));
                ui.checkbox(&mut prefs.telnet, crate::i18n::tr("Telnet")).on_hover_text(crate::i18n::tr("Answer telnet's questions and keep them out of what's shown: for telnet servers and BBSes, not for a plain socket"));
                ui.menu_button(crate::i18n::tr("Ultimate 64\u{2026}"), |ui| {
                    ui.label(crate::i18n::tr("The Ultimate 64 and Ultimate II+ take connections on the network: its modem (as a C64 terminal program sees it) on the port set in its Modem settings, 6400 unless changed; and its own menu over telnet, on port 23, once Telnet is turned on in its network settings."));
                    let hosts: Vec<String> = if ultimates.is_empty() { vec![prefs.host.clone()] } else { ultimates.to_vec() };
                    for host in hosts.iter().filter(|h| !h.trim().is_empty()) {
                        if ui.button(crate::i18n::tf!("{host}: its modem, port 6400")).clicked() {
                            (prefs.host, prefs.tcp_port, prefs.telnet) = (host.clone(), 6400, false);
                            ui.close();
                        }
                        if ui.button(crate::i18n::tf!("{host}: its menu over telnet, port 23")).clicked() {
                            (prefs.host, prefs.tcp_port, prefs.telnet, prefs.view) = (host.clone(), 23, true, View::Screen);
                            ui.close();
                        }
                    }
                    if ultimates.is_empty() {
                        ui.label(crate::i18n::tr("An Ultimate set up in File, Send to device is offered here by name."));
                    }
                });
            }
        }
    });
    if prefs.kind == Kind::Serial {
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::tr("Data bits"));
            egui::ComboBox::from_id_salt("serial-bits").selected_text(prefs.data_bits.to_string()).width(40.0).show_ui(ui, |ui| {
                for bits in [8u8, 7, 6, 5] {
                    ui.selectable_value(&mut prefs.data_bits, bits, bits.to_string());
                }
            });
            ui.label(crate::i18n::tr("Parity"));
            let parity_label = |p: Parity| match p {
                Parity::None => crate::i18n::tr("None"),
                Parity::Odd => crate::i18n::tr("Odd"),
                Parity::Even => crate::i18n::tr("Even"),
            };
            egui::ComboBox::from_id_salt("serial-parity").selected_text(parity_label(prefs.parity)).width(60.0).show_ui(ui, |ui| {
                for parity in [Parity::None, Parity::Odd, Parity::Even] {
                    ui.selectable_value(&mut prefs.parity, parity, parity_label(parity));
                }
            });
            ui.label(crate::i18n::tr("Stop bits"));
            egui::ComboBox::from_id_salt("serial-stop").selected_text(prefs.stop_bits.to_string()).width(40.0).show_ui(ui, |ui| {
                for stop in [1u8, 2] {
                    ui.selectable_value(&mut prefs.stop_bits, stop, stop.to_string());
                }
            });
            ui.label(crate::i18n::tr("Flow control"));
            egui::ComboBox::from_id_salt("serial-flow").selected_text(crate::i18n::tr(prefs.flow.label())).width(140.0).show_ui(ui, |ui| {
                for flow in Flow::ALL {
                    ui.selectable_value(&mut prefs.flow, flow, crate::i18n::tr(flow.label()));
                }
            });
        });
    }
}

fn draw_serial(ui: &mut Ui, window: &mut SerialWindow, prefs: &mut link::Prefs, ultimates: &[String], asked: &mut Vec<Asked>) {
    let connected = window.session.is_some();
    let transferring = window.session.as_ref().is_some_and(Session::transferring);
    ui.add_enabled_ui(!connected, |ui| draw_connection(ui, window, prefs, ultimates));
    ui.horizontal_wrapped(|ui| {
        if let Some(session) = &window.session {
            if ui.button(crate::i18n::tr("Disconnect")).clicked() {
                asked.push(Asked::Disconnect);
            }
            match session.ended() {
                Some(why) => ui.colored_label(theme::rainbow_mistake(), crate::i18n::tf!("The connection ended: {why}")),
                None => ui.label(crate::i18n::tf!("Connected: {}", session.described)),
            };
            if session.serial {
                if ui.button(crate::i18n::tr("Reset (DTR)")).on_hover_text(crate::i18n::tr("Drop DTR for a moment, which resets an Arduino")).clicked() {
                    session.pulse_dtr();
                }
                if ui.button(crate::i18n::tr("Send a break")).clicked() {
                    session.send_break();
                }
            }
        } else if ui.button(crate::i18n::tr("Connect")).clicked() {
            asked.push(Asked::Connect);
        }
    });
    ui.separator();
    // How what comes in is shown.
    ui.horizontal_wrapped(|ui| {
        ui.radio_value(&mut prefs.view, View::Lines, crate::i18n::tr("Text"));
        ui.radio_value(&mut prefs.view, View::Screen, crate::i18n::tr("Screen (VT100)"));
        ui.radio_value(&mut prefs.view, View::Hex, crate::i18n::tr("Hex"));
        ui.separator();
        ui.add_enabled_ui(prefs.view == View::Lines, |ui| {
            egui::ComboBox::from_id_salt("serial-chars").selected_text(crate::i18n::tr(prefs.chars.label())).width(200.0).show_ui(ui, |ui| {
                for chars in Chars::ALL {
                    ui.selectable_value(&mut prefs.chars, chars, crate::i18n::tr(chars.label()));
                }
            });
            ui.checkbox(&mut prefs.times, crate::i18n::tr("Times"));
            ui.checkbox(&mut prefs.echo, crate::i18n::tr("Show what's sent")).on_hover_text(crate::i18n::tr("For a machine that doesn't echo what's typed to it"));
        });
        if ui.button(crate::i18n::tr("Clear")).clicked() {
            asked.push(Asked::Clear);
        }
        if let Some(session) = &window.session {
            match session.log_path() {
                Some(path) => {
                    if ui.button(crate::i18n::tr("Stop logging")).on_hover_text(path.display().to_string()).clicked() {
                        asked.push(Asked::StopLog);
                    }
                }
                None => {
                    egui::ComboBox::from_id_salt("serial-log-kind")
                        .selected_text(if window.log_kind == LogKind::Raw { crate::i18n::tr("What came in, as it came") } else { crate::i18n::tr("Both ways, in hex with times") })
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut window.log_kind, LogKind::Raw, crate::i18n::tr("What came in, as it came"));
                            ui.selectable_value(&mut window.log_kind, LogKind::Hex, crate::i18n::tr("Both ways, in hex with times"));
                        });
                    if ui.button(crate::i18n::tr("Log to a file\u{2026}")).clicked() {
                        asked.push(Asked::StartLog);
                    }
                }
            }
        }
    });
    // The bottom rows' room is kept for them.
    let below = ui.spacing().interact_size.y * 4.0 + 40.0 + if window.received.is_empty() { 0.0 } else { 26.0 * window.received.len().min(4) as f32 };
    let height = (ui.available_height() - below).max(120.0);
    let font = egui::FontId::monospace(13.0);
    let row_height = ui.fonts_mut(|f| f.row_height(&font));
    let frame = egui::Frame::NONE.fill(theme::editor_back()).inner_margin(4.0);
    frame.show(ui, |ui| {
        ui.set_min_height(height);
        ui.set_max_height(height);
        ui.set_min_width(ui.available_width());
        match prefs.view {
            View::Lines => {
                let count = window.lines.lines.len();
                egui::ScrollArea::both().id_salt("serial-lines").stick_to_bottom(true).auto_shrink([false, false]).show_rows(ui, row_height, count, |ui, range| {
                    for (at, line) in window.lines.lines.range(range) {
                        let text = if prefs.times { format!("{}  {line}", at.format("%H:%M:%S%.3f")) } else { line.clone() };
                        ui.add(egui::Label::new(RichText::new(text).font(font.clone()).color(theme::editor_text())).extend());
                    }
                });
            }
            View::Hex => {
                let count = window.hex.len();
                egui::ScrollArea::both().id_salt("serial-hex").stick_to_bottom(true).auto_shrink([false, false]).show_rows(ui, row_height, count, |ui, range| {
                    for row in window.hex.range(range) {
                        let colour = if row.incoming { theme::editor_text() } else { theme::accent() };
                        ui.add(egui::Label::new(RichText::new(link::hex_row(&row.time, row.incoming, &row.bytes)).font(font.clone()).color(colour)).extend());
                    }
                });
            }
            View::Screen => {
                let typed = draw_screen(ui, &window.screen, &font);
                if !typed.is_empty() {
                    asked.push(Asked::Send(typed));
                }
            }
        }
    });
    // The line to type into.
    ui.horizontal(|ui| {
        let response = ui.add_enabled(connected, egui::TextEdit::singleline(&mut window.typed).hint_text(if window.typed_hex { crate::i18n::tr("Bytes in hex, such as 1B 5B 41") } else { crate::i18n::tr("Type a line to send") }).desired_width(ui.available_width() - 330.0).font(font.clone()));
        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.checkbox(&mut window.typed_hex, crate::i18n::tr("Hex"));
        ui.add_enabled_ui(!window.typed_hex, |ui| {
            ui.label(crate::i18n::tr("Ending"));
            egui::ComboBox::from_id_salt("serial-ending").selected_text(crate::i18n::tr(prefs.ending.label())).width(70.0).show_ui(ui, |ui| {
                for ending in Ending::ALL {
                    ui.selectable_value(&mut prefs.ending, ending, crate::i18n::tr(ending.label()));
                }
            });
        });
        if (ui.add_enabled(connected, egui::Button::new(crate::i18n::tr("Send"))).clicked() || entered) && connected {
            match link::typed_bytes(&window.typed, prefs.chars, prefs.ending, window.typed_hex) {
                Ok(bytes) => {
                    asked.push(Asked::Send(bytes));
                    window.typed.clear();
                    window.note = None;
                }
                Err(problem) => window.note = Some(problem),
            }
            response.request_focus();
        }
    });
    // Files, with XMODEM and YMODEM.
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(!transferring, |ui| {
            egui::ComboBox::from_id_salt("serial-protocol").selected_text(crate::i18n::tr(prefs.protocol.label())).width(250.0).show_ui(ui, |ui| {
                for protocol in Protocol::ALL {
                    ui.selectable_value(&mut prefs.protocol, protocol, crate::i18n::tr(protocol.label()));
                }
            });
        });
        let can = connected && !transferring;
        if ui.add_enabled(can, egui::Button::new(crate::i18n::tr("Send the file in front"))).clicked() {
            asked.push(Asked::SendFront);
        }
        if ui.add_enabled(can, egui::Button::new(crate::i18n::tr("Send files\u{2026}"))).clicked() {
            asked.push(Asked::SendChosen);
        }
        if ui.add_enabled(can, egui::Button::new(crate::i18n::tr("Receive"))).clicked() {
            asked.push(Asked::Receive);
        }
        if prefs.protocol != Protocol::Ymodem {
            ui.checkbox(&mut window.trim, crate::i18n::tr("Take the padding off")).on_hover_text(crate::i18n::tr("XMODEM sends whole blocks, padded with byte $1A: take that off the end of what's received"));
        }
    });
    if let Some(session) = &window.session {
        let transfer = link::lock(&session.shared.transfer).clone();
        if let Some(transfer) = transfer {
            ui.horizontal_wrapped(|ui| {
                let p = &transfer.progress;
                ui.label(crate::i18n::tr(transfer.protocol.label()));
                let name = if p.file.is_empty() { String::new() } else { format!("{} ", p.file) };
                let (done, retries) = (p.done, p.retries);
                match &transfer.outcome {
                    None => {
                        let fraction = p.total.filter(|t| *t > 0).map_or(0.0, |t| done as f32 / t as f32);
                        let words = if transfer.sending { crate::i18n::tf!("Sending {name}: {done} bytes, {retries} tried again") } else { crate::i18n::tf!("Receiving {name}: {done} bytes, {retries} tried again") };
                        ui.add(egui::ProgressBar::new(fraction).text(words).desired_width(420.0));
                        if done == 0 && p.files_done == 0 {
                            ui.label(crate::i18n::tr("Waiting for the other side to begin\u{2026}"));
                        }
                        if ui.button(crate::i18n::tr("Stop")).clicked() {
                            session.stop_transfer();
                        }
                    }
                    Some(Ok(files)) if transfer.sending => {
                        let _ = files;
                        ui.label(crate::i18n::tf!("Sent, with {retries} blocks tried again."));
                    }
                    Some(Ok(files)) => {
                        let count = files.len();
                        ui.label(crate::i18n::tf!("Received {count} files, with {retries} blocks tried again."));
                        if window.received.is_empty() && !files.is_empty() {
                            window.received = files.clone();
                        }
                    }
                    Some(Err(problem)) => {
                        ui.colored_label(theme::rainbow_mistake(), problem);
                    }
                }
            });
        }
    }
    let mut chosen = None;
    for (index, file) in window.received.iter().enumerate() {
        ui.horizontal(|ui| {
            let name = file.name.clone().unwrap_or_else(|| crate::i18n::tr("The file received").into());
            let size = file.data.len();
            ui.label(crate::i18n::tf!("{name}, {size} bytes"));
            if ui.button(crate::i18n::tr("Open")).clicked() {
                chosen = Some(Asked::Open(index));
            }
            if ui.button(crate::i18n::tr("Save\u{2026}")).clicked() {
                chosen = Some(Asked::Save(index));
            }
        });
    }
    asked.extend(chosen);
    if let Some(note) = &window.note {
        ui.colored_label(theme::rainbow_mistake(), note);
    }
}

/// The VT100 screen, drawn cell by cell in its colours, and the keys typed while it has the focus (clicked) as
/// the bytes a terminal sends for them.
fn draw_screen(ui: &mut Ui, parser: &vt100::Parser, font: &egui::FontId) -> Vec<u8> {
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let cell = egui::vec2(ui.fonts_mut(|f| f.glyph_width(font, 'M')), ui.fonts_mut(|f| f.row_height(font)));
    let size = egui::vec2(cell.x * f32::from(cols), cell.y * f32::from(rows));
    let mut typed = Vec::new();
    egui::ScrollArea::both().id_salt("serial-screen").auto_shrink([false, false]).show(ui, |ui| {
        let (area, response) = ui.allocate_exact_size(size, egui::Sense::click());
        if response.clicked() {
            response.request_focus();
        }
        let focused = response.has_focus();
        if focused {
            ui.memory_mut(|m| m.set_focus_lock_filter(response.id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
            let application = screen.application_cursor();
            for event in ui.input(|i| i.events.clone()) {
                match event {
                    egui::Event::Text(text) => typed.extend(text.as_bytes()),
                    egui::Event::Paste(text) => typed.extend(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes()),
                    egui::Event::Key { key, pressed: true, modifiers, .. } => typed.extend(crate::terminal::key_bytes(key, modifiers, application).unwrap_or_default()),
                    _ => {}
                }
            }
        }
        let painter = ui.painter_at(area);
        let background = egui::Color32::from_rgb(0x16, 0x16, 0x16);
        let foreground = egui::Color32::from_rgb(0xD4, 0xD4, 0xD4);
        painter.rect_filled(area, 0.0, background);
        for row in 0..rows {
            for col in 0..cols {
                let Some(c) = screen.cell(row, col) else { continue };
                let (mut fg, mut bg) = (crate::terminal::colour(c.fgcolor(), foreground), crate::terminal::colour(c.bgcolor(), background));
                if c.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let at = egui::pos2(area.min.x + f32::from(col) * cell.x, area.min.y + f32::from(row) * cell.y);
                if bg != background {
                    painter.rect_filled(egui::Rect::from_min_size(at, cell), 0.0, bg);
                }
                if c.has_contents() {
                    painter.text(at, egui::Align2::LEFT_TOP, c.contents(), font.clone(), fg);
                }
            }
        }
        if !screen.hide_cursor() {
            let (row, col) = screen.cursor_position();
            let at = egui::pos2(area.min.x + f32::from(col) * cell.x, area.min.y + f32::from(row) * cell.y);
            let shape = egui::Rect::from_min_size(at, cell);
            if focused {
                painter.rect_filled(shape, 0.0, foreground.gamma_multiply(0.7));
            } else {
                painter.rect_stroke(shape, 0.0, egui::Stroke::new(1.0, foreground.gamma_multiply(0.6)), egui::StrokeKind::Inside);
            }
        }
    });
    typed
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, draw};
    use super::*;
    use std::time::Instant;

    #[test]
    fn the_window_connects_shows_what_comes_and_receives_a_file() {
        // A far end on this computer: says hello, waits for a line, then sends a file with YMODEM.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let far = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
            let mut line = crate::xmodem::Stream::new(stream);
            crate::xmodem::Line::put(&mut line, b"READY.\r\n\x1b[31mred\x1b[0m\r\n").unwrap();
            let mut got = Vec::new();
            while !got.ends_with(b"\r") {
                if let Some(b) = crate::xmodem::Line::get(&mut line, Duration::from_secs(10)).unwrap() {
                    got.push(b);
                }
            }
            let file = Outgoing { name: "demo.prg".into(), data: vec![0x01, 0x08, 0x0B, 0x08], modified: None };
            crate::xmodem::send(&mut line, Protocol::Ymodem, &[file], &crate::xmodem::Timing::default(), &std::sync::atomic::AtomicBool::new(false), &mut |_| {}).unwrap();
            got
        });
        let (mut app, ctx) = app_with("");
        app.serial_prefs = link::Prefs { kind: Kind::Network, host: "127.0.0.1".into(), tcp_port: port, ending: Ending::Cr, ..link::Prefs::default() };
        app.open_serial_terminal();
        let mut window = app.serial.take().unwrap();
        let mut prefs = app.serial_prefs.clone();
        app.serial_asked(&mut window, &mut prefs, Asked::Connect);
        assert!(window.session.is_some(), "{:?}", window.note);
        app.serial = Some(window);
        let settle = |app: &mut Limoniter, what: &dyn Fn(&SerialWindow) -> bool| {
            let began = Instant::now();
            loop {
                draw(app, &ctx, Vec::new());
                if what(app.serial.as_ref().unwrap()) {
                    break;
                }
                assert!(began.elapsed() < Duration::from_secs(20), "it never came");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        settle(&mut app, &|w| w.lines.lines.len() >= 2);
        let window = app.serial.as_ref().unwrap();
        let shown: Vec<&str> = window.lines.lines.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(&shown[..2], ["READY.", "red"]);
        assert_eq!(window.screen.screen().contents().lines().next(), Some("READY."));
        assert!(window.hex.iter().any(|r| r.incoming && r.bytes.starts_with(b"READY.")));
        // A line typed, then the file received.
        let mut window = app.serial.take().unwrap();
        let mut prefs = app.serial_prefs.clone();
        app.serial_asked(&mut window, &mut prefs, Asked::Send(link::typed_bytes("go", Chars::Ascii, Ending::Cr, false).unwrap()));
        app.serial_asked(&mut window, &mut prefs, Asked::Receive);
        app.serial = Some(window);
        settle(&mut app, &|w| !w.received.is_empty());
        assert_eq!(far.join().unwrap(), b"go\r");
        let tabs = app.docs.len();
        let mut window = app.serial.take().unwrap();
        assert_eq!(window.received[0].name.as_deref(), Some("demo.prg"));
        app.serial_asked(&mut window, &mut prefs, Asked::Open(0));
        assert!(app.docs.len() >= tabs);
        assert_eq!(app.document_bytes(), [0x01, 0x08, 0x0B, 0x08]);
        // The hex view shows what was sent, too.
        app.serial = Some(window);
        draw(&mut app, &ctx, Vec::new());
        assert!(app.serial.as_ref().unwrap().hex.iter().any(|r| !r.incoming && r.bytes == b"go\r"));
    }
}
