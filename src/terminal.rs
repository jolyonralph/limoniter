//! A terminal in the window: the person's shell on a pseudo-terminal (ConPTY on Windows), its output read into a
//! VT100 screen as an xterm would show it (colours, the cursor, full-screen programs), and keys turned into what
//! a terminal sends.

use eframe::egui::{self, Color32, FontId, Key, Modifiers, Rect, Sense, Ui, pos2, vec2};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct Terminal {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    size: (u16, u16),
    /// Whether the shell has ended.
    ended: Arc<AtomicBool>,
    /// Set when something new is on the screen, to draw it.
    pub changed: Arc<AtomicBool>,
    /// The folder it started in.
    pub folder: std::path::PathBuf,
}

/// The shell to start: PowerShell on Windows, and the person's own elsewhere.
fn shell() -> CommandBuilder {
    if cfg!(windows) {
        let mut command = CommandBuilder::new("powershell.exe");
        command.arg("-NoLogo");
        command
    } else {
        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into());
        CommandBuilder::new(shell)
    }
}

impl Terminal {
    /// Starts a shell in a folder, on a screen of so many rows and columns.
    pub fn start(folder: &Path, rows: u16, cols: u16) -> Result<Terminal, String> {
        Self::start_with(shell(), folder, rows, cols)
    }

    pub fn start_with(mut command: CommandBuilder, folder: &Path, rows: u16, cols: u16) -> Result<Terminal, String> {
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| format!("A terminal couldn't be made: {e}"))?;
        command.cwd(folder);
        command.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(command).map_err(|e| format!("The shell couldn't be started: {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(pair.master.take_writer().map_err(|e| e.to_string())?));
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 2000)));
        let (ended, changed) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(true)));
        let (screen, done, fresh, answer) = (parser.clone(), ended.clone(), changed.clone(), writer.clone());
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let got = &buffer[..n];
                        let mut reply = Vec::new();
                        if let Ok(mut parser) = screen.lock() {
                            parser.process(got);
                            // Where the cursor is, and what kind of terminal this is, when asked: Windows' ConPTY
                            // asks as it starts, and shows nothing until it's answered.
                            if got.windows(4).any(|w| w == b"\x1b[6n") {
                                let (row, col) = parser.screen().cursor_position();
                                reply.extend(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes());
                            }
                            if got.windows(3).any(|w| w == b"\x1b[c") || got.windows(4).any(|w| w == b"\x1b[0c") {
                                reply.extend(b"\x1b[?1;2c");
                            }
                        }
                        if !reply.is_empty()
                            && let Ok(mut writer) = answer.lock()
                        {
                            let _ = writer.write_all(&reply);
                            let _ = writer.flush();
                        }
                        fresh.store(true, Ordering::Relaxed);
                    }
                }
            }
            done.store(true, Ordering::Relaxed);
            fresh.store(true, Ordering::Relaxed);
        });
        Ok(Terminal { parser, writer, master: pair.master, child, size: (rows, cols), ended, changed, folder: folder.to_path_buf() })
    }

    pub fn send(&mut self, bytes: &[u8]) {
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) == self.size || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        if let Ok(mut parser) = self.parser.lock() {
            parser.screen_mut().set_size(rows, cols);
        }
    }

    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }

    /// What's on the screen, as text.
    #[cfg(all(test, unix))]
    pub fn text(&self) -> String {
        self.parser.lock().map(|p| p.screen().contents()).unwrap_or_default()
    }

    /// Whether the program running wants the cursor keys as an application's (vim and the like).
    fn application_cursor(&self) -> bool {
        self.parser.lock().is_ok_and(|p| p.screen().application_cursor())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// What a key sends to the terminal, if anything: Enter a carriage return, the arrows their escape sequences
/// (the application forms when asked for), Ctrl and a letter its control code.
pub fn key_bytes(key: Key, modifiers: Modifiers, application_cursor: bool) -> Option<Vec<u8>> {
    let arrow = |c: char| if application_cursor { format!("\x1bO{c}") } else { format!("\x1b[{c}") };
    let bytes = match key {
        Key::Enter => "\r".to_string(),
        Key::Backspace => "\x7f".to_string(),
        Key::Tab if modifiers.shift => "\x1b[Z".to_string(),
        Key::Tab => "\t".to_string(),
        Key::Escape => "\x1b".to_string(),
        Key::ArrowUp => arrow('A'),
        Key::ArrowDown => arrow('B'),
        Key::ArrowRight => arrow('C'),
        Key::ArrowLeft => arrow('D'),
        Key::Home => "\x1b[H".to_string(),
        Key::End => "\x1b[F".to_string(),
        Key::PageUp => "\x1b[5~".to_string(),
        Key::PageDown => "\x1b[6~".to_string(),
        Key::Delete => "\x1b[3~".to_string(),
        Key::Insert => "\x1b[2~".to_string(),
        _ if modifiers.ctrl && !modifiers.alt => {
            let name = key.name();
            let c = name.chars().next().filter(|_| name.len() == 1)?.to_ascii_lowercase();
            if c.is_ascii_lowercase() {
                return Some(vec![c as u8 - b'a' + 1]);
            }
            return None;
        }
        _ => return None,
    };
    Some(bytes.into_bytes())
}

/// An xterm colour as it's drawn: the 16 named ones, the 6x6x6 cube, the greys, or red, green and blue.
fn colour(c: vt100::Color, default: Color32) -> Color32 {
    const NAMED: [u32; 16] = [0x1E1E1E, 0xCD3131, 0x0DBC79, 0xE5E510, 0x2472C8, 0xBC3FBC, 0x11A8CD, 0xE5E5E5, 0x666666, 0xF14C4C, 0x23D18B, 0xF5F543, 0x3B8EEA, 0xD670D6, 0x29B8DB, 0xFFFFFF];
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) if i < 16 => {
            let v = NAMED[usize::from(i)];
            Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
        }
        vt100::Color::Idx(i) if i < 232 => {
            let n = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        vt100::Color::Idx(i) => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// Draws the terminal to fill the space, and passes it the keyboard while it has the focus. Clicking it gives it
/// the focus.
pub fn draw(ui: &mut Ui, terminal: &mut Terminal, font_size: f32) {
    let font = FontId::monospace(font_size);
    let cell = vec2(ui.fonts_mut(|f| f.glyph_width(&font, 'M')), ui.fonts_mut(|f| f.row_height(&font)));
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let rows = ((area.height() / cell.y).floor() as u16).max(2);
    let cols = ((area.width() / cell.x).floor() as u16).max(10);
    terminal.resize(rows, cols);
    let id = ui.scope_id().with("terminal");
    let response = ui.interact(area, id, Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    let focused = response.has_focus();
    if focused {
        // The terminal takes Tab, the arrows and Escape, as a terminal does.
        ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
        let events = ui.input(|i| i.events.clone());
        let application = terminal.application_cursor();
        for event in events {
            match event {
                egui::Event::Text(text) => terminal.send(text.as_bytes()),
                egui::Event::Paste(text) => terminal.send(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes()),
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(bytes) = key_bytes(key, modifiers, application) {
                        terminal.send(&bytes);
                    }
                }
                _ => {}
            }
        }
    }
    let painter = ui.painter_at(area);
    let background = Color32::from_rgb(0x16, 0x16, 0x16);
    painter.rect_filled(area, 0.0, background);
    let foreground = Color32::from_rgb(0xD4, 0xD4, 0xD4);
    let Ok(parser) = terminal.parser.lock() else {
        return;
    };
    let screen = parser.screen();
    for row in 0..rows {
        for col in 0..cols {
            let Some(c) = screen.cell(row, col) else {
                continue;
            };
            let (mut fg, mut bg) = (colour(c.fgcolor(), foreground), colour(c.bgcolor(), background));
            if c.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            let at = pos2(area.min.x + f32::from(col) * cell.x, area.min.y + f32::from(row) * cell.y);
            if bg != background {
                painter.rect_filled(Rect::from_min_size(at, cell), 0.0, bg);
            }
            if c.has_contents() {
                painter.text(at, egui::Align2::LEFT_TOP, c.contents(), font.clone(), fg);
            }
        }
    }
    if !screen.hide_cursor() {
        let (row, col) = screen.cursor_position();
        let at = pos2(area.min.x + f32::from(col) * cell.x, area.min.y + f32::from(row) * cell.y);
        let shape = Rect::from_min_size(at, cell);
        if focused {
            painter.rect_filled(shape, 0.0, foreground.gamma_multiply(0.7));
        } else {
            painter.rect_stroke(shape, 0.0, egui::Stroke::new(1.0, foreground.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        }
    }
    if terminal.ended() {
        painter.text(pos2(area.min.x + 4.0, area.max.y - cell.y - 2.0), egui::Align2::LEFT_TOP, "The shell has ended. Close the terminal and open it again for another.", font, Color32::from_rgb(0xE5, 0xE5, 0x10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_send_what_a_terminal_sends() {
        assert_eq!(key_bytes(Key::Enter, Modifiers::NONE, false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, Modifiers::NONE, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, Modifiers::NONE, true), Some(b"\x1bOA".to_vec()), "vim's way");
        assert_eq!(key_bytes(Key::C, Modifiers::CTRL, false), Some(vec![3]), "Ctrl+C");
        assert_eq!(key_bytes(Key::A, Modifiers::NONE, false), None, "letters come as text");
        assert_eq!(colour(vt100::Color::Idx(196), Color32::BLACK), Color32::from_rgb(255, 0, 0));
    }

    /// A real shell on a pseudo-terminal: what's typed runs, and its output, colours and all, is on the screen.
    #[cfg(unix)]
    #[test]
    fn a_shell_runs_in_it() {
        let mut command = CommandBuilder::new("/bin/sh");
        command.env("PS1", "$ ");
        let mut terminal = Terminal::start_with(command, &std::env::temp_dir(), 24, 80).unwrap();
        terminal.send(b"printf '\\033[31mred\\033[0m %s\\n' $((6*7))\r");
        let began = std::time::Instant::now();
        while !terminal.text().contains("red 42") && began.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(terminal.text().contains("red 42"), "{}", terminal.text());
        let red = {
            let parser = terminal.parser.lock().unwrap();
            let screen = parser.screen();
            (0..24).find_map(|row| (0..80).find(|col| screen.cell(row, *col).is_some_and(|c| c.contents() == "r" && c.fgcolor() == vt100::Color::Idx(1))))
        };
        assert!(red.is_some(), "red is red");
        terminal.resize(30, 100);
        terminal.send(b"exit\r");
        let began = std::time::Instant::now();
        while !terminal.ended() && began.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(terminal.ended());
    }
}
