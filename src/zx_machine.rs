//! The ZX Spectrum built into Limoniter: a 48K or 128K, emulated by rustzx-core, running a program in a panel
//! beside the file it came from. It loads tapes (TAP, and TZX made into TAP) and snapshots (SNA, and Z80 made
//! into SNA); the Spectrum ROMs come with rustzx, as Amstrad allows for emulators.

use crate::running::{Format, Machine, Runnable};
use rustzx_core::host::{BufferCursor, FrameBuffer, FrameBufferSource, Host, HostContext, Snapshot, Stopwatch, StubDebugInterface, StubIoExtender, Tape};
use rustzx_core::zx::keys::{CompoundKey, ZXKey};
use rustzx_core::zx::machine::ZXMachine;
use rustzx_core::zx::video::colors::{ZXBrightness, ZXColor};
use rustzx_core::{EmulationMode, Emulator, RustzxSettings};
use std::time::{Duration, Instant};

/// The whole picture, border and all.
pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 240;
/// Where the screen sits inside the border.
const SCREEN_LEFT: usize = 32;
const SCREEN_TOP: usize = 24;
const FRAMES_PER_SECOND: f64 = 50.08;

struct LimoniterHost;

impl Host for LimoniterHost {
    type Context = Context;
    type TapeAsset = BufferCursor<Vec<u8>>;
    type FrameBuffer = Pixels;
    type EmulationStopwatch = Watch;
    type IoExtender = StubIoExtender;
    type DebugInterface = StubDebugInterface;
}

#[derive(Clone)]
struct Context;

impl HostContext<LimoniterHost> for Context {
    fn frame_buffer_context(&self) {}
}

/// A picture the emulator draws into, as red, green, blue and alpha.
struct Pixels {
    width: usize,
    rgba: Vec<u8>,
}

/// The Spectrum's colours, normal and bright, as most emulators show them.
fn colour(colour: ZXColor, brightness: ZXBrightness) -> [u8; 4] {
    let level = if matches!(brightness, ZXBrightness::Bright) { 0xFF } else { 0xD7 };
    let bits = colour as u8;
    let on = |bit: u8| if bits & bit != 0 { level } else { 0 };
    [on(2), on(4), on(1), 0xFF]
}

impl FrameBuffer for Pixels {
    type Context = ();

    fn new(width: usize, height: usize, _source: FrameBufferSource, _context: ()) -> Self {
        Pixels { width, rgba: vec![0; width * height * 4] }
    }

    fn set_color(&mut self, x: usize, y: usize, colour_now: ZXColor, brightness: ZXBrightness) {
        let at = (y * self.width + x) * 4;
        if let Some(pixel) = self.rgba.get_mut(at..at + 4) {
            pixel.copy_from_slice(&colour(colour_now, brightness));
        }
    }
}

struct Watch(Instant);

impl Stopwatch for Watch {
    fn new() -> Self {
        Watch(Instant::now())
    }

    fn measure(&self) -> Duration {
        self.0.elapsed()
    }
}

/// A key held down for a few frames and then let go, for a character typed.
/// Keys clicked on the drawn keyboard: held while the mouse button is, and for a few frames at least, so the ROM
/// sees even a quick click.
struct BoardPress {
    keys: Vec<ZXKey>,
    frames: u32,
    let_go: bool,
}

/// The same key, as ZXKey has no `==`.
fn same(a: ZXKey, b: ZXKey) -> bool {
    std::mem::discriminant(&a) == std::mem::discriminant(&b)
}

struct Tap {
    keys: Vec<ZXKey>,
    frames: u32,
}

/// The built-in Spectrum, running.
pub struct BuiltIn {
    emulator: Emulator<LimoniterHost>,
    /// When frames were last caught up to, and how many are owed since.
    last: Instant,
    owed: f64,
    pub paused: bool,
    taps: std::collections::VecDeque<Tap>,
    held: Vec<ZXKey>,
    /// Keys held down on the keyboard drawn under the screen, apart from those held on the computer's own.
    board: Option<BoardPress>,
    /// Caps Shift and Symbol Shift clicked on that keyboard, waiting to go with the next key clicked.
    pub caps_latched: bool,
    pub symbol_latched: bool,
    /// The picture, made afresh each frame.
    picture: Vec<u8>,
    /// Where its sound goes, if anywhere, and whether it's muted.
    speaker: Option<crate::speaker::Speaker>,
    pub muted: bool,
    /// How loud the loudest sound made has been, for the tests.
    #[cfg(test)]
    loudest: f32,
}

impl BuiltIn {
    /// Whether it can run this; if not, what it can't do.
    pub fn takes(run: &Runnable) -> Result<(), &'static str> {
        if !matches!(run.machine, Machine::Spectrum48 | Machine::Spectrum128) {
            return Err("is a 48K and 128K; this is for another machine.");
        }
        match run.format {
            Format::Tap | Format::Tzx | Format::Sna | Format::Z80 => Ok(()),
            _ => Err("runs tapes and snapshots (.tap, .tzx, .sna, .z80)."),
        }
    }

    /// Starts a Spectrum running a program, silently.
    #[cfg(test)]
    pub fn start(run: &Runnable) -> Result<BuiltIn, String> {
        Self::start_inner(run, None, None)
    }

    /// Starts a Spectrum running a program, its sound going to a speaker if one's given.
    pub fn start_with(run: &Runnable, speaker: Option<crate::speaker::Speaker>) -> Result<BuiltIn, String> {
        let rate = speaker.as_ref().map(|s| s.rate);
        Self::start_inner(run, rate, speaker)
    }

    /// Starts it, making sound at `rate` if there's a rate, to go to the speaker if there's one.
    fn start_inner(run: &Runnable, rate: Option<u32>, speaker: Option<crate::speaker::Speaker>) -> Result<BuiltIn, String> {
        Self::takes(run).map_err(|why| format!("The built-in Spectrum {why}"))?;
        // A snapshot says which Spectrum it's from; a tape loads on a 128K only if it says so.
        let (machine, snapshot) = match run.format {
            Format::Sna => (if run.bytes.len() == 49179 { Machine::Spectrum48 } else { Machine::Spectrum128 }, Some(run.bytes.to_vec())),
            Format::Z80 => {
                let sna = z80_to_sna(&run.bytes)?;
                (if sna.len() == 49179 { Machine::Spectrum48 } else { Machine::Spectrum128 }, Some(sna))
            }
            _ => (run.machine, None),
        };
        let settings = RustzxSettings {
            machine: if machine == Machine::Spectrum128 { ZXMachine::Sinclair128K } else { ZXMachine::Sinclair48K },
            emulation_mode: EmulationMode::FrameCount(1),
            tape_fastload_enabled: true,
            kempston_enabled: false,
            mouse_enabled: false,
            load_default_rom: true,
            autoload_enabled: true,
            // The beeper, and the 128K's AY chip in stereo, at the speaker's own rate.
            ay_mode: rustzx_core::zx::sound::ay::ZXAYMode::ABC,
            ay_enabled: true,
            beeper_enabled: true,
            sound_enabled: rate.is_some(),
            sound_volume: 100,
            sound_sample_rate: rate.unwrap_or(44100) as usize,
        };
        let mut emulator = Emulator::<LimoniterHost>::new(settings, Context).map_err(|e| format!("The built-in Spectrum couldn't start: {e:?}"))?;
        match snapshot {
            Some(sna) => emulator.load_snapshot(Snapshot::Sna(BufferCursor::new(sna))).map_err(|e| format!("The snapshot couldn't be loaded: {e:?}"))?,
            None => {
                let tap = if run.format == Format::Tzx { tzx_to_tap(&run.bytes)? } else { run.bytes.to_vec() };
                let basic_first = tap.get(2..4) == Some(&[0, 0][..]);
                emulator.load_tape(Tape::Tap(BufferCursor::new(tap))).map_err(|e| format!("The tape couldn't be loaded: {e:?}"))?;
                emulator.play_tape();
                let mut built = BuiltIn { emulator, last: Instant::now(), owed: 0.0, paused: false, taps: Default::default(), held: Vec::new(), board: None, caps_latched: false, symbol_latched: false, picture: vec![0; WIDTH * HEIGHT * 4], speaker: None, muted: false, #[cfg(test)] loudest: 0.0 };
                // A tape that starts with a BASIC program loads that much at once, rather than at the tape's speed:
                // it's loaded when the program starts running, and PPC, the line being run, is a line at last.
                if basic_first {
                    // Loading at once makes no sound worth hearing.
                    let ppc = |m: &BuiltIn| u16::from_le_bytes([m.peek(23621), m.peek(23622)]);
                    for _ in 0..50 * 90 {
                        built.frame()?;
                        if ppc(&built) < 0xFFFE {
                            break;
                        }
                    }
                }
                built.speaker = speaker;
                return Ok(built);
            }
        }
        Ok(BuiltIn { emulator, last: Instant::now(), owed: 0.0, paused: false, taps: Default::default(), held: Vec::new(), board: None, caps_latched: false, symbol_latched: false, picture: vec![0; WIDTH * HEIGHT * 4], speaker, muted: false, #[cfg(test)] loudest: 0.0 })
    }

    /// Runs the frames due since last time: at the Spectrum's own speed, catching up at most a few frames, so a
    /// window that wasn't drawn for a while doesn't race ahead.
    pub fn catch_up(&mut self) -> Result<(), String> {
        let now = Instant::now();
        if self.paused {
            self.last = now;
            if let Some(speaker) = &self.speaker {
                speaker.clear();
            }
            return Ok(());
        }
        self.owed = (self.owed + now.duration_since(self.last).as_secs_f64() * FRAMES_PER_SECOND).min(5.0);
        self.last = now;
        while self.owed >= 1.0 {
            self.owed -= 1.0;
            self.frame()?;
        }
        Ok(())
    }

    /// Runs one frame.
    pub fn frame(&mut self) -> Result<(), String> {
        self.press_taps();
        self.board_tick();
        self.emulator.emulate_frames(Duration::from_millis(500)).map_err(|e| format!("The built-in Spectrum stopped: {e:?}"))?;
        // The frame's sound, to the speaker, or let go of if there isn't one or it's muted.
        while let Some(sample) = self.emulator.next_audio_sample() {
            #[cfg(test)]
            {
                self.loudest = self.loudest.max(sample.left.abs()).max(sample.right.abs());
            }
            if let Some(speaker) = self.speaker.as_ref().filter(|_| !self.muted) {
                speaker.push(sample.left, sample.right);
            }
        }
        Ok(())
    }

    /// Whether it makes sound at all.
    pub fn has_speaker(&self) -> bool {
        self.speaker.is_some()
    }

    /// Mutes or unmutes it; muting lets go of the sound waiting.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(speaker) = &self.speaker {
            speaker.clear();
        }
    }

    /// The picture, border and all, as red, green, blue and alpha, `WIDTH` by `HEIGHT`.
    pub fn picture(&mut self) -> &[u8] {
        let border = &self.emulator.border_buffer().rgba;
        self.picture.copy_from_slice(border);
        let screen = &self.emulator.screen_buffer().rgba;
        for row in 0..192 {
            let to = ((SCREEN_TOP + row) * WIDTH + SCREEN_LEFT) * 4;
            self.picture[to..to + 256 * 4].copy_from_slice(&screen[row * 256 * 4..(row + 1) * 256 * 4]);
        }
        &self.picture
    }

    pub fn peek(&self, address: u16) -> u8 {
        self.emulator.peek(address)
    }

    /// Writes to memory; writes to the ROM are let go, as on a real Spectrum.
    pub fn poke(&mut self, address: u16, values: &[u8]) {
        let actions: Vec<rustzx_core::poke::PokeAction> = values.iter().enumerate().map(|(i, value)| (address.wrapping_add(i as u16), *value)).filter(|(at, _)| *at >= 0x4000).map(|(at, value)| rustzx_core::poke::PokeAction::mem(at, value)).collect();
        struct Many(Vec<rustzx_core::poke::PokeAction>);
        impl rustzx_core::poke::Poke for Many {
            fn actions(&self) -> &[rustzx_core::poke::PokeAction] {
                &self.0
            }
        }
        self.emulator.execute_poke(Many(actions));
    }

    /// The screen as text, each character cell matched against the ROM's font, as printed or inverse. A cell
    /// that isn't a character (a graphic, a user-defined one) is a full stop, unless it's empty.
    #[cfg(test)]
    pub fn screen_text(&self) -> Vec<String> {
        screen_text(|address| self.peek(address))
    }

    /// A key on the Spectrum's own keyboard pressed or let go, for game keys and the like.
    pub fn key(&mut self, key: ZXKey, pressed: bool) {
        if pressed {
            self.emulator.send_key(key, true);
            self.held.push(key);
        } else {
            self.held.retain(|k| !same(*k, key));
            if !self.on_board(key) {
                self.emulator.send_key(key, false);
            }
        }
    }

    fn on_board(&self, key: ZXKey) -> bool {
        self.board.as_ref().is_some_and(|b| b.keys.iter().any(|k| same(*k, key)))
    }

    /// Keys pressed together on the drawn keyboard, held until `board_up`. Any held there before are let go first.
    pub fn board_down(&mut self, keys: Vec<ZXKey>) {
        self.board_release();
        for key in &keys {
            self.emulator.send_key(*key, true);
        }
        self.board = Some(BoardPress { keys, frames: 0, let_go: false });
    }

    /// The mouse button let go on the drawn keyboard: its keys go up once they've been down long enough to be seen.
    pub fn board_up(&mut self) {
        if let Some(press) = &mut self.board {
            press.let_go = true;
        }
    }

    /// Whether the drawn keyboard has a key down.
    pub fn board_held(&self) -> bool {
        self.board.is_some()
    }

    fn board_tick(&mut self) {
        let Some(press) = &mut self.board else {
            return;
        };
        press.frames += 1;
        if press.let_go && press.frames >= 4 {
            self.board_release();
        }
    }

    fn board_release(&mut self) {
        for key in self.board.take().map(|b| b.keys).unwrap_or_default() {
            if !self.held.iter().any(|k| same(*k, key)) {
                self.emulator.send_key(key, false);
            }
        }
    }

    pub fn compound(&mut self, key: CompoundKey, pressed: bool) {
        self.emulator.send_compound_key(key, pressed);
    }

    /// Types a character, with Caps Shift or Symbol Shift as the Spectrum needs for it.
    pub fn type_char(&mut self, c: char) -> bool {
        let Some(keys) = keys_for(c) else {
            return false;
        };
        self.taps.push_back(Tap { keys, frames: 0 });
        true
    }

    /// Typing is a few frames of each key down, then a few up, so the ROM sees each one.
    fn press_taps(&mut self) {
        let Some(tap) = self.taps.front_mut() else {
            return;
        };
        tap.frames += 1;
        match tap.frames {
            1 => {
                for key in tap.keys.clone() {
                    self.emulator.send_key(key, true);
                }
            }
            4 => {
                for key in tap.keys.clone() {
                    self.emulator.send_key(key, false);
                }
            }
            7.. => {
                self.taps.pop_front();
            }
            _ => {}
        }
    }

    /// Lets go of every key, as when the panel loses the keyboard.
    pub fn release_all(&mut self) {
        for key in std::mem::take(&mut self.held) {
            self.emulator.send_key(key, false);
        }
    }

    /// Whether characters are still being typed.
    #[cfg(test)]
    pub fn typing(&self) -> bool {
        !self.taps.is_empty()
    }
}

/// A Spectrum's screen as text, read through `peek`: 24 rows of 32 cells, each matched against the font in the
/// ROM at 0x3D00, the right way round or inverse.
pub fn screen_text(peek: impl Fn(u16) -> u8) -> Vec<String> {
    let font: Vec<[u8; 8]> = (0..96u16).map(|c| std::array::from_fn(|line| peek(0x3D00 + c * 8 + line as u16))).collect();
    (0..24)
        .map(|row| {
            let text: String = (0..32)
                .map(|column| {
                    let cell: [u8; 8] = std::array::from_fn(|line| {
                        let y = row * 8 + line;
                        peek((0x4000 + ((y & 0xC0) << 5) + ((y & 7) << 8) + ((y & 0x38) << 2) + column) as u16)
                    });
                    let inverse = cell.map(|b| !b);
                    match font.iter().position(|glyph| *glyph == cell || *glyph == inverse) {
                        Some(index) => char::from(0x20 + index as u8),
                        None if cell == [0; 8] || inverse == [0; 8] => ' ',
                        None => '.',
                    }
                })
                .collect();
            text.trim_end().to_string()
        })
        .collect()
}

/// The Spectrum keys for a character: a letter's own key (with Caps Shift for a capital), a digit's, or a symbol's
/// key with Symbol Shift.
pub fn keys_for(c: char) -> Option<Vec<ZXKey>> {
    use ZXKey::*;
    let letter = |c: char| -> Option<ZXKey> {
        Some(match c.to_ascii_lowercase() {
            'a' => A,
            'b' => B,
            'c' => C,
            'd' => D,
            'e' => E,
            'f' => F,
            'g' => G,
            'h' => H,
            'i' => I,
            'j' => J,
            'k' => K,
            'l' => L,
            'm' => M,
            'n' => N,
            'o' => O,
            'p' => P,
            'q' => Q,
            'r' => R,
            's' => S,
            't' => T,
            'u' => U,
            'v' => V,
            'w' => W,
            'x' => X,
            'y' => Y,
            'z' => Z,
            '0' => N0,
            '1' => N1,
            '2' => N2,
            '3' => N3,
            '4' => N4,
            '5' => N5,
            '6' => N6,
            '7' => N7,
            '8' => N8,
            '9' => N9,
            _ => return None,
        })
    };
    if c.is_ascii_lowercase() || c.is_ascii_digit() {
        return letter(c).map(|key| vec![key]);
    }
    if c.is_ascii_uppercase() {
        return letter(c).map(|key| vec![Shift, key]);
    }
    let symbol = match c {
        ' ' => return Some(vec![Space]),
        '\n' | '\r' => return Some(vec![Enter]),
        '!' => N1,
        '@' => N2,
        '#' => N3,
        '$' => N4,
        '%' => N5,
        '&' => N6,
        '\'' => N7,
        '(' => N8,
        ')' => N9,
        '_' => N0,
        '<' => R,
        '>' => T,
        ';' => O,
        '"' => P,
        '^' => H,
        '-' => J,
        '+' => K,
        '=' => L,
        ':' => Z,
        '?' => C,
        '/' => V,
        '*' => B,
        ',' => N,
        '.' => M,
        _ => return None,
    };
    Some(vec![SymShift, symbol])
}

/// A Z80 snapshot as an SNA snapshot, which rustzx loads. Every version of Z80 is read, compressed or not, for the
/// 48K and the 128K.
pub fn z80_to_sna(z80: &[u8]) -> Result<Vec<u8>, String> {
    let short = || "This Z80 snapshot is cut short.".to_string();
    let header = z80.get(..30).ok_or_else(short)?;
    let word = |at: usize| u16::from_le_bytes([header[at], header[at + 1]]);
    let flags1 = if header[12] == 0xFF { 1 } else { header[12] };
    let mut pc = word(6);
    // 48K memory from 0x4000, or the 128K's eight banks.
    let mut ram48 = vec![0u8; 0xC000];
    let mut banks: Vec<Option<Vec<u8>>> = vec![None; 8];
    let mut port_7ffd = 0u8;
    let mut is_128 = false;
    if pc != 0 {
        let data = &z80[30..];
        let memory = if flags1 & 0x20 != 0 { decompress(data, Some(0xC000))? } else { data.to_vec() };
        let n = memory.len().min(0xC000);
        ram48[..n].copy_from_slice(&memory[..n]);
    } else {
        let extra = usize::from(u16::from_le_bytes([*z80.get(30).ok_or_else(short)?, *z80.get(31).ok_or_else(short)?]));
        let ext = z80.get(32..32 + extra).ok_or_else(short)?;
        pc = u16::from_le_bytes([ext[0], ext[1]]);
        let hardware = ext[2];
        is_128 = if extra == 23 { matches!(hardware, 3 | 4 | 12) } else { matches!(hardware, 4..=6 | 12) };
        port_7ffd = ext[3];
        let mut at = 32 + extra;
        while at + 3 <= z80.len() {
            let length = usize::from(u16::from_le_bytes([z80[at], z80[at + 1]]));
            let page = z80[at + 2];
            at += 3;
            let (data, used) = if length == 0xFFFF { (z80.get(at..at + 0x4000).ok_or_else(short)?.to_vec(), 0x4000) } else { (decompress(z80.get(at..at + length).ok_or_else(short)?, None)?, length) };
            at += used;
            if is_128 {
                if (3..=10).contains(&page) {
                    banks[usize::from(page - 3)] = Some(data);
                }
            } else {
                let offset = match page {
                    8 => 0x0000,
                    4 => 0x4000,
                    5 => 0x8000,
                    _ => continue,
                };
                let n = data.len().min(0x4000);
                ram48[offset..offset + n].copy_from_slice(&data[..n]);
            }
        }
    }
    // The SNA header: registers in its own order.
    let r = (header[11] & 0x7F) | ((flags1 & 1) << 7);
    let mut sna = vec![
        header[10], // I
        header[19], header[20], // HL'
        header[17], header[18], // DE'
        header[15], header[16], // BC'
        header[22], header[21], // F', A'
        header[4], header[5], // HL
        header[13], header[14], // DE
        header[2], header[3], // BC
        header[23], header[24], // IY
        header[25], header[26], // IX
        if header[28] != 0 { 4 } else { 0 }, // IFF2
        r,
        header[1], header[0], // F, A
    ];
    let mut sp = word(8);
    let im = header[29] & 3;
    let border = (flags1 >> 1) & 7;
    if !is_128 {
        // A 48K SNA has PC on the stack.
        sp = sp.wrapping_sub(2);
        if sp >= 0x4000 {
            let at = usize::from(sp - 0x4000);
            ram48[at] = (pc & 0xFF) as u8;
            if at + 1 < ram48.len() {
                ram48[at + 1] = (pc >> 8) as u8;
            }
        }
        sna.extend_from_slice(&sp.to_le_bytes());
        sna.extend_from_slice(&[im, border]);
        sna.extend_from_slice(&ram48);
        return Ok(sna);
    }
    sna.extend_from_slice(&sp.to_le_bytes());
    sna.extend_from_slice(&[im, border]);
    let paged = usize::from(port_7ffd & 7);
    let bank = |n: usize| banks[n].clone().unwrap_or_else(|| vec![0; 0x4000]);
    sna.extend(bank(5));
    sna.extend(bank(2));
    sna.extend(bank(paged));
    sna.extend_from_slice(&pc.to_le_bytes());
    sna.extend_from_slice(&[port_7ffd, 0]);
    for n in [0, 1, 3, 4, 6, 7] {
        if n != paged {
            sna.extend(bank(n));
        }
    }
    Ok(sna)
}

/// Undoes the Z80 format's run-length coding: ED ED, a count and a byte stand for that many of the byte.
fn decompress(data: &[u8], most: Option<usize>) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(most.unwrap_or(0x4000));
    let mut i = 0;
    while i < data.len() {
        if most.is_some_and(|most| out.len() >= most) {
            break;
        }
        if data[i] == 0xED && data.get(i + 1) == Some(&0xED) {
            let count = *data.get(i + 2).ok_or("This Z80 snapshot is cut short.")?;
            let value = *data.get(i + 3).ok_or("This Z80 snapshot is cut short.")?;
            out.extend(std::iter::repeat_n(value, usize::from(count)));
            i += 4;
        } else if most.is_some() && data[i..].starts_with(&[0x00, 0xED, 0xED, 0x00]) {
            break;
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    Ok(out)
}

/// A TZX tape's data blocks as a TAP tape. Standard blocks carry over exactly; turbo and pure data blocks keep
/// their bytes, which loads them if they're in the ROM's own format. Other blocks, such as pauses and text, are
/// left out.
pub fn tzx_to_tap(tzx: &[u8]) -> Result<Vec<u8>, String> {
    if !tzx.starts_with(b"ZXTape!\x1A") {
        return Err("This isn't a TZX tape.".into());
    }
    let short = || "This TZX tape is cut short.".to_string();
    let n16 = |at: usize| -> Result<usize, String> { Ok(usize::from(u16::from_le_bytes([*tzx.get(at).ok_or_else(short)?, *tzx.get(at + 1).ok_or_else(short)?]))) };
    let n24 = |at: usize| -> Result<usize, String> { Ok(n16(at)? | usize::from(*tzx.get(at + 2).ok_or_else(short)?) << 16) };
    let n32 = |at: usize| -> Result<usize, String> { Ok(n16(at)? | n16(at + 2)? << 16) };
    let mut tap = Vec::new();
    let mut push = |data: &[u8]| {
        tap.extend_from_slice(&(data.len() as u16).to_le_bytes());
        tap.extend_from_slice(data);
    };
    let mut at = 10;
    while at < tzx.len() {
        let id = tzx[at];
        at += 1;
        let skip = match id {
            0x10 => {
                let length = n16(at + 2)?;
                push(tzx.get(at + 4..at + 4 + length).ok_or_else(short)?);
                4 + length
            }
            0x11 => {
                let length = n24(at + 15)?;
                push(tzx.get(at + 18..at + 18 + length).ok_or_else(short)?);
                18 + length
            }
            0x14 => {
                let length = n24(at + 7)?;
                push(tzx.get(at + 10..at + 10 + length).ok_or_else(short)?);
                10 + length
            }
            0x12 => 4,
            0x13 => 1 + 2 * usize::from(*tzx.get(at).ok_or_else(short)?),
            0x15 => 8 + n24(at + 5)?,
            0x18 | 0x19 => 4 + n32(at)?,
            0x20 | 0x23 | 0x24 => 2,
            0x21 | 0x30 => 1 + usize::from(*tzx.get(at).ok_or_else(short)?),
            0x22 | 0x25 | 0x27 => 0,
            0x26 => 2 + 2 * n16(at)?,
            0x28 | 0x32 => 2 + n16(at)?,
            0x2A => 4,
            0x2B => 5,
            0x31 => 2 + usize::from(*tzx.get(at + 1).ok_or_else(short)?),
            0x33 => 1 + 3 * usize::from(*tzx.get(at).ok_or_else(short)?),
            0x35 => 20 + n32(at + 16)?,
            0x5A => 9,
            other => return Err(format!("This TZX tape has a block (ID {other:#04X}) the built-in Spectrum can't read.")),
        };
        at += skip;
    }
    if tap.is_empty() {
        return Err("This TZX tape has nothing on it the built-in Spectrum can load.".into());
    }
    Ok(tap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn tape_of(lines: &[&str]) -> Runnable {
        Runnable::from_listing(lines, crate::basic::Machine::Spectrum, "test").unwrap()
    }

    /// Runs frames until the screen says something, or gives up.
    fn run_until(machine: &mut BuiltIn, frames: usize, mut done: impl FnMut(&BuiltIn) -> bool) -> bool {
        for _ in 0..frames {
            machine.frame().unwrap();
            if done(machine) {
                return true;
            }
        }
        false
    }

    /// The character cell's 8 bytes of the Spectrum screen, row by row, at a text position.
    fn cell(machine: &BuiltIn, row: usize, column: usize) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        for (line, byte) in bytes.iter_mut().enumerate() {
            let y = row * 8 + line;
            let address = 0x4000 + ((y & 0xC0) << 5) + ((y & 7) << 8) + ((y & 0x38) << 2) + column;
            *byte = machine.peek(address as u16);
        }
        bytes
    }

    /// The ROM's own picture of a character, from its font at 0x3D00.
    fn glyph(machine: &BuiltIn, c: char) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        for (line, byte) in bytes.iter_mut().enumerate() {
            *byte = machine.peek(0x3D00 + (c as u16 - 0x20) * 8 + line as u16);
        }
        bytes
    }

    /// Whether some text is on the screen, anywhere, in the ROM's font.
    fn on_screen(machine: &BuiltIn, text: &str) -> bool {
        let glyphs: Vec<[u8; 8]> = text.chars().map(|c| glyph(machine, c)).collect();
        (0..24).any(|row| (0..=32 - glyphs.len()).any(|column| glyphs.iter().enumerate().all(|(i, g)| cell(machine, row, column + i) == *g)))
    }

    #[test]
    fn a_listing_runs_on_the_built_in_spectrum_and_prints() {
        let run = tape_of(&["10 PRINT \"HELLO\"", "20 STOP"]);
        let mut machine = BuiltIn::start(&run).unwrap();
        // It loads by itself from the tape, at once, and runs: HELLO is there within a second.
        let printed = run_until(&mut machine, 50, |m| on_screen(m, "HELLO"));
        assert!(printed, "HELLO was printed");
        assert_eq!(machine.picture().len(), WIDTH * HEIGHT * 4);
    }

    #[test]
    fn typing_reaches_the_spectrum() {
        let run = tape_of(&["10 PRINT \"READY\"", "20 INPUT a$", "30 PRINT a$"]);
        let mut machine = BuiltIn::start(&run).unwrap();
        // Wait for the program to be running, and a moment more for INPUT.
        assert!(run_until(&mut machine, 50 * 30, |m| on_screen(m, "READY")));
        run_until(&mut machine, 25, |_| false);
        for c in "Hi!\n".chars() {
            assert!(machine.type_char(c));
        }
        let printed = run_until(&mut machine, 50 * 5, |m| !m.typing() && on_screen(m, "Hi!"));
        assert!(printed, "what was typed was printed, capital and symbol too");
    }

    #[test]
    fn keys_clicked_on_the_drawn_keyboard_reach_the_spectrum() {
        let run = tape_of(&["10 PRINT \"READY\"", "20 INPUT a$", "30 PRINT a$"]);
        let mut machine = BuiltIn::start(&run).unwrap();
        assert!(run_until(&mut machine, 50 * 30, |m| on_screen(m, "READY")));
        run_until(&mut machine, 25, |_| false);
        // Quick clicks, down and up before a frame has run: Caps Shift and H, then Symbol Shift and 1, then Enter.
        for keys in [vec![ZXKey::Shift, ZXKey::H], vec![ZXKey::SymShift, ZXKey::N1], vec![ZXKey::Enter]] {
            machine.board_down(keys);
            // The computer's own keyboard letting go of Symbol Shift doesn't let go of one clicked.
            machine.key(ZXKey::SymShift, false);
            machine.board_up();
            run_until(&mut machine, 10, |_| false);
            assert!(!machine.board_held(), "let go once seen");
        }
        assert!(run_until(&mut machine, 50 * 2, |m| on_screen(m, "H!")), "clicked keys typed H!");
    }

    #[test]
    fn memory_can_be_read_and_changed() {
        let mut machine = BuiltIn::start(&tape_of(&["10 STOP"])).unwrap();
        machine.frame().unwrap();
        machine.poke(0x8000, &[1, 2, 3]);
        assert_eq!((machine.peek(0x8000), machine.peek(0x8002)), (1, 3));
        let rom = machine.peek(0);
        machine.poke(0, &[rom ^ 0xFF]);
        assert_eq!(machine.peek(0), rom, "the ROM isn't written");
    }

    #[test]
    fn a_z80_snapshot_becomes_an_sna_of_the_same_machine() {
        // A version 1 snapshot, uncompressed: PC 0x8000, a JR to itself there.
        let mut z80 = vec![0u8; 30];
        z80[6] = 0x00;
        z80[7] = 0x80;
        z80[8] = 0x00;
        z80[9] = 0xFF;
        z80[12] = 2 << 1;
        let mut ram = vec![0u8; 0xC000];
        ram[0x4000] = 0x18;
        ram[0x4001] = 0xFE;
        z80.extend(ram);
        let sna = z80_to_sna(&z80).unwrap();
        assert_eq!(sna.len(), 49179);
        assert_eq!(sna[26], 2, "the border colour carries over");
        // PC is on the stack, just below where SP was.
        assert_eq!(&sna[27 + 0xFF00 - 2 - 0x4000..][..2], &[0x00, 0x80]);
        let run = Runnable { machine: Machine::Spectrum48, format: Format::Z80, name: "loop.z80".into(), bytes: Arc::new(z80) };
        let mut machine = BuiltIn::start(&run).unwrap();
        machine.frame().unwrap();
        assert_eq!(machine.peek(0x8000), 0x18);
    }

    #[test]
    fn compressed_z80_memory_is_expanded() {
        assert_eq!(decompress(&[1, 0xED, 0xED, 5, 9, 2], None).unwrap(), [1, 9, 9, 9, 9, 9, 2]);
        assert_eq!(decompress(&[1, 2, 0x00, 0xED, 0xED, 0x00], Some(100)).unwrap(), [1, 2], "the end marker ends it");
    }

    #[test]
    fn a_tzx_tape_keeps_its_standard_blocks() {
        let mut tzx = b"ZXTape!\x1A\x01\x14".to_vec();
        tzx.extend([0x30, 4, b'h', b'i', b'!', b'!']); // text, left out
        tzx.extend([0x10, 0xE8, 0x03, 3, 0, 0, 1, 2]); // a standard block of 3 bytes
        let tap = tzx_to_tap(&tzx).unwrap();
        assert_eq!(tap, [3, 0, 0, 1, 2]);
        assert!(tzx_to_tap(b"not a tape").is_err());
    }

    #[test]
    fn the_screen_reads_back_as_text() {
        let mut machine = BuiltIn::start(&tape_of(&["10 PRINT \"HELLO WORLD\"", "20 PRINT INVERSE 1; \"OK\""])).unwrap();
        run_until(&mut machine, 50, |m| on_screen(m, "OK"));
        let screen = machine.screen_text();
        assert!(screen.iter().any(|row| row == "HELLO WORLD"), "{screen:?}");
        assert!(screen.iter().any(|row| row == "OK"), "inverse text reads too");
    }

    #[test]
    fn a_beep_makes_sound() {
        let mut machine = BuiltIn::start_inner(&tape_of(&["10 BEEP 1,0", "20 GO TO 10"]), Some(44100), None).unwrap();
        run_until(&mut machine, 50, |_| false);
        assert!(machine.loudest > 0.05, "the beeper was heard: {}", machine.loudest);
        let mut quiet = BuiltIn::start_inner(&tape_of(&["10 PAUSE 0"]), Some(44100), None).unwrap();
        run_until(&mut quiet, 50, |_| false);
        assert!(quiet.loudest < machine.loudest, "and a program that makes none is quieter");
    }

    #[test]
    fn characters_are_typed_with_the_shifts_they_need() {
        assert_eq!(keys_for('a').map(|k| k.len()), Some(1));
        assert_eq!(keys_for('A').map(|k| k.len()), Some(2));
        assert!(matches!(keys_for('"').as_deref(), Some([ZXKey::SymShift, ZXKey::P])));
        assert!(keys_for('\u{e9}').is_none());
    }
}
