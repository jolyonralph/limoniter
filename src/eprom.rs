//! EPROM, EEPROM and flash chips burned with XGecu's TL866 and T48 programmers, through minipro: the free
//! command-line program for them (https://gitlab.com/DavidGriffith/minipro). Limoniter finds it, asks it for
//! its chips, and runs it to read, write, verify, blank-check and erase, showing what it says.
//!
//! The options used are those in minipro's manual page (man/minipro.1, dated 12 April 2025):
//! `-k` (which programmer is plugged in), `-q` with `-l` (the chips of a programmer: TL866A, TL866II, T48, T56
//! or T76; without `-q` and with nothing plugged in, `-l` stops to ask which list, so it's always given), `-d`
//! (a chip's details, among them "Memory: 32768 Bytes"), `-p` (the chip), `-r`, `-w` and `-m` (read, write and
//! verify a file), `-b` (blank check), `-E` (erase only), `-z` (pin contact check), `-e` (no erase before
//! writing), `-x` (no ID check when reading), `-y` (an ID that doesn't match isn't an error), `-i` (in-circuit,
//! ICSP) and `-f ihex` or `-f srec` for files in those formats. Writing verifies afterwards unless told not to
//! with `-v`, which Limoniter never passes. The TL866II+, T48 and T56 share one list of chips; the TL866A and
//! TL866CS have their own, and the T76 its own.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// Where minipro is found and described.
pub const HOME: &str = "https://gitlab.com/DavidGriffith/minipro";

/// A programmer, for choosing which list of chips to show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Programmer {
    /// Whichever is plugged in; the TL866II+'s list (shared by the T48 and T56) when none is.
    Attached,
    Tl866a,
    Tl866ii,
    T48,
    T56,
    T76,
}

impl Programmer {
    pub const ALL: [Programmer; 6] = [Programmer::Attached, Programmer::Tl866a, Programmer::Tl866ii, Programmer::T48, Programmer::T56, Programmer::T76];

    pub fn label(self) -> &'static str {
        match self {
            Programmer::Attached => "The one plugged in",
            Programmer::Tl866a => "TL866A or TL866CS",
            Programmer::Tl866ii => "TL866II+",
            Programmer::T48 => "T48",
            Programmer::T56 => "T56",
            Programmer::T76 => "T76",
        }
    }

    /// The name minipro's `-q` takes.
    fn option(self) -> Option<&'static str> {
        match self {
            Programmer::Attached => None,
            Programmer::Tl866a => Some("TL866A"),
            Programmer::Tl866ii => Some("TL866II"),
            Programmer::T48 => Some("T48"),
            Programmer::T56 => Some("T56"),
            Programmer::T76 => Some("T76"),
        }
    }

    fn id(self) -> &'static str {
        self.option().unwrap_or("attached")
    }

    /// The programmer named in minipro's "Found TL866II+ 04.2.132 (0x284)".
    pub fn from_found(found: &str) -> Option<Programmer> {
        let model = found.split_whitespace().next()?;
        Some(match model {
            "TL866A" | "TL866CS" => Programmer::Tl866a,
            "TL866II+" => Programmer::Tl866ii,
            "T48" => Programmer::T48,
            "T56" => Programmer::T56,
            "T76" => Programmer::T76,
            _ => return None,
        })
    }
}

/// Where the file goes in a chip bigger than it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Placement {
    /// At the start, the rest filled.
    Start,
    /// At the end, as a 6502's vectors need it.
    End,
    /// Repeated to fill the chip, so it reads the same whatever the high address lines do.
    Repeat,
}

impl Placement {
    pub const ALL: [Placement; 3] = [Placement::Start, Placement::End, Placement::Repeat];

    pub fn label(self) -> &'static str {
        match self {
            Placement::Start => "At the start",
            Placement::End => "At the end",
            Placement::Repeat => "Repeated to fill the chip",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Placement::Start => "start",
            Placement::End => "end",
            Placement::Repeat => "repeat",
        }
    }
}

/// The choices, kept between runs.
#[derive(Clone, PartialEq, Debug)]
pub struct Prefs {
    pub chip: String,
    pub programmer: Programmer,
    pub fill: u8,
    pub placement: Placement,
    /// Carry on when the chip's ID isn't the one expected (or, reading, don't check it).
    pub ignore_id: bool,
    /// Don't erase before writing: for a chip that's blank already, or that can't be erased electrically.
    pub skip_erase: bool,
    /// Programmed in the circuit, through the ICSP lead.
    pub icsp: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { chip: String::new(), programmer: Programmer::Attached, fill: 0xFF, placement: Placement::Start, ignore_id: false, skip_erase: false, icsp: false }
    }
}

impl Prefs {
    pub fn to_settings(&self) -> String {
        format!(
            "eprom_chip={}\neprom_programmer={}\neprom_fill={:02X}\neprom_placement={}\neprom_ignore_id={}\neprom_skip_erase={}\neprom_icsp={}\n",
            self.chip,
            self.programmer.id(),
            self.fill,
            self.placement.id(),
            self.ignore_id,
            self.skip_erase,
            self.icsp
        )
    }

    pub fn read_setting(&mut self, name: &str, value: &str) {
        let value = value.trim();
        match name {
            "eprom_chip" => self.chip = value.to_string(),
            "eprom_programmer" => self.programmer = Programmer::ALL.into_iter().find(|p| p.id() == value).unwrap_or(Programmer::Attached),
            "eprom_fill" => self.fill = u8::from_str_radix(value, 16).unwrap_or(0xFF),
            "eprom_placement" => self.placement = Placement::ALL.into_iter().find(|p| p.id() == value).unwrap_or(Placement::Start),
            "eprom_ignore_id" => self.ignore_id = value == "true",
            "eprom_skip_erase" => self.skip_erase = value == "true",
            "eprom_icsp" => self.icsp = value == "true",
            _ => {}
        }
    }
}

/// What minipro is asked to do.
#[derive(Clone, PartialEq, Debug)]
pub enum Action {
    /// Which programmer is plugged in.
    Detect,
    /// The chips a programmer knows.
    List(Programmer),
    /// A chip's details: its size, package and so on.
    Info,
    Read(PathBuf),
    /// Written, then verified by minipro as it always does.
    Write(PathBuf, Option<FileFormat>),
    Verify(PathBuf, Option<FileFormat>),
    BlankCheck,
    Erase,
    PinCheck,
}

/// A file that isn't plain bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileFormat {
    IntelHex,
    Srecord,
}

impl FileFormat {
    /// The format a file's name says it's in.
    pub fn of(name: &str) -> Option<FileFormat> {
        let ending = name.rsplit_once('.')?.1.to_ascii_lowercase();
        match ending.as_str() {
            "hex" | "ihx" | "ihex" | "mcs" => Some(FileFormat::IntelHex),
            "srec" | "s19" | "s28" | "s37" | "mot" => Some(FileFormat::Srecord),
            _ => None,
        }
    }

    fn option(self) -> &'static str {
        match self {
            FileFormat::IntelHex => "ihex",
            FileFormat::Srecord => "srec",
        }
    }
}

/// minipro's arguments for an action.
pub fn arguments(prefs: &Prefs, action: &Action) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let chip = |out: &mut Vec<String>| out.extend(["-p".to_string(), prefs.chip.trim().to_string()]);
    let file = |path: &Path| path.display().to_string();
    match action {
        Action::Detect => out.push("-k".into()),
        Action::List(programmer) => {
            // Without -q, and with nothing plugged in, minipro stops to ask which list: so it's always given.
            out.extend(["-q".into(), programmer.option().unwrap_or("TL866II").into(), "-l".into()]);
        }
        Action::Info => {
            if let Some(q) = prefs.programmer.option() {
                out.extend(["-q".into(), q.into()]);
            }
            out.extend(["-d".into(), prefs.chip.trim().to_string()]);
        }
        Action::Read(path) => {
            chip(&mut out);
            out.extend(["-r".into(), file(path)]);
            if prefs.ignore_id {
                out.push("-x".into());
            }
        }
        Action::Write(path, format) | Action::Verify(path, format) => {
            chip(&mut out);
            out.extend([if matches!(action, Action::Write(..)) { "-w" } else { "-m" }.into(), file(path)]);
            if let Some(format) = format {
                out.extend(["-f".into(), format.option().into()]);
            }
            if matches!(action, Action::Write(..)) && prefs.skip_erase {
                out.push("-e".into());
            }
            if prefs.ignore_id {
                out.push("-y".into());
            }
        }
        Action::BlankCheck | Action::Erase | Action::PinCheck => {
            chip(&mut out);
            out.push(
                match action {
                    Action::BlankCheck => "-b",
                    Action::Erase => "-E",
                    _ => "-z",
                }
                .into(),
            );
            if prefs.ignore_id && !matches!(action, Action::PinCheck) {
                out.push("-y".into());
            }
        }
    }
    if prefs.icsp && !matches!(action, Action::Detect | Action::List(_) | Action::Info) {
        out.push("-i".into());
    }
    out
}

/// minipro, looked for in these folders.
pub fn find_in(folders: &[PathBuf]) -> Option<PathBuf> {
    let name = if cfg!(windows) { "minipro.exe" } else { "minipro" };
    folders.iter().map(|f| f.join(name)).find(|p| p.is_file())
}

/// minipro, on the PATH or where Homebrew and `make install` put it.
pub fn find() -> Option<PathBuf> {
    let mut folders: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    folders.extend(["/usr/local/bin", "/opt/homebrew/bin", "/opt/local/bin", "/usr/bin"].map(PathBuf::from));
    find_in(&folders)
}

/// How to get minipro on this computer.
pub fn how_to_get() -> String {
    if cfg!(target_os = "macos") {
        crate::i18n::tf!("minipro isn't installed. With Homebrew: brew install minipro. Or build it from {HOME}.")
    } else if cfg!(windows) {
        crate::i18n::tf!("minipro isn't installed. It's made for Linux, BSD and macOS; on Windows, it can be built with MSYS2 from {HOME}, or XGecu's own Xgpro program used instead.")
    } else {
        crate::i18n::tf!("minipro isn't installed. Most Linux distributions have it as a package called minipro; or build it from {HOME}. Its udev rules let it use the programmer without being root.")
    }
}

/// The chips in minipro's list, one name to a line (such as AT28C256 or W27C512@DIP28), leaving out anything
/// else it says.
pub fn read_list(output: &str) -> Vec<String> {
    output.lines().map(str::trim).filter(|l| !l.is_empty() && !l.contains(char::is_whitespace) && !l.ends_with(':') && !l.ends_with('.') && !l.ends_with('!')).map(str::to_string).collect()
}

/// The programmer minipro found: the text after "Found ", such as "TL866II+ 04.2.132 (0x284)".
pub fn found(output: &str) -> Option<String> {
    output.lines().find_map(|l| l.trim().strip_prefix("Found ")).map(|s| s.trim().to_string())
}

/// A chip's size in bytes, from minipro's details: "Memory: 32768 Bytes" (or Words, of two bytes each, or Bits).
pub fn size_from_info(output: &str) -> Option<u64> {
    let line = output.lines().find_map(|l| l.trim().strip_prefix("Memory:"))?;
    let mut words = line.split_whitespace();
    let count: u64 = words.next()?.parse().ok()?;
    match words.next()? {
        "Bytes" => Some(count),
        "Words" => Some(count * 2),
        "Bits" => Some(count / 8),
        _ => None,
    }
}

/// A chip's size guessed from its name, for when minipro's details can't be had: a 27C256 holds 256 kilobits.
/// The 27 (EPROM), 28 (EEPROM), 29 and 39 (flash) families are known, with or without a maker's letters.
pub fn size_from_name(name: &str) -> Option<u64> {
    let name = name.split('@').next()?.to_ascii_uppercase();
    for family in ["27", "28", "29", "39"] {
        let mut from = 0;
        while let Some(at) = name[from..].find(family) {
            let at = from + at;
            from = at + 1;
            // The family's number is at the start, or just after letters (W27C512, AT28C256, SST39SF040).
            if at > 0 && !name.as_bytes()[at - 1].is_ascii_alphabetic() {
                continue;
            }
            let rest = &name[at + 2..];
            let rest = rest.trim_start_matches(|c: char| c.is_ascii_alphabetic());
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            let kilobits: u64 = match digits.as_str() {
                "16" => 16,
                "32" => 32,
                "64" => 64,
                "128" => 128,
                "256" => 256,
                "512" => 512,
                "010" | "1001" => 1024,
                "020" | "2001" => 2048,
                "040" | "4001" => 4096,
                "080" | "801" => 8192,
                "160" => 16_384,
                _ => continue,
            };
            return Some(kilobits * 1024 / 8);
        }
    }
    None
}

/// The file made to fit the chip: placed as chosen, the rest filled. A file too big for the chip is refused.
pub fn fit(data: &[u8], size: usize, fill: u8, placement: Placement) -> Result<Vec<u8>, String> {
    if data.len() > size {
        let (have, room) = (data.len(), size);
        return Err(crate::i18n::tf!("The file is {have} bytes, but the chip holds only {room}."));
    }
    let mut out = vec![fill; size];
    match placement {
        Placement::Start => out[..data.len()].copy_from_slice(data),
        Placement::End => out[size - data.len()..].copy_from_slice(data),
        Placement::Repeat if !data.is_empty() => {
            for chunk in out.chunks_mut(data.len()) {
                chunk.copy_from_slice(&data[..chunk.len()]);
            }
        }
        Placement::Repeat => {}
    }
    Ok(out)
}

/// Where a verify found the chip different: the address, and what the file and the chip hold there. From
/// "Verification failed at address 0x0010: File=0x41, Device=0xFF".
pub fn mismatch(output: &str) -> Option<(u32, u32, u32)> {
    let line = output.lines().find_map(|l| l.trim().strip_prefix("Verification failed at address "))?;
    let hex = |s: &str| u32::from_str_radix(s.trim().trim_start_matches("0x").trim_start_matches("0X"), 16).ok();
    let (address, rest) = line.split_once(':')?;
    let file = rest.split_once("File=")?.1.split(',').next()?;
    let device = rest.split_once("Device=")?.1;
    Some((hex(address)?, hex(file)?, hex(device)?))
}

/// One line saying how an action went, from minipro's answer and whether it succeeded.
pub fn summary(action: &Action, succeeded: bool, output: &str) -> String {
    if output.lines().any(|l| l.trim() == "No programmer found." || l.trim() == "[No programmer found]") {
        return crate::i18n::tr("No programmer was found. Is it plugged in (and, on Linux, are minipro's udev rules installed)?").into();
    }
    if let Some(device) = output.lines().find_map(|l| l.trim().strip_prefix("Device ").and_then(|r| r.strip_suffix(" not found!"))) {
        return crate::i18n::tf!("minipro doesn't know a chip called {device}. Choose one from its list.");
    }
    if let Some((address, file, chip)) = mismatch(output) {
        return if *action == Action::BlankCheck {
            crate::i18n::tf!("The chip isn't blank: ${address:04X} holds ${chip:02X}.")
        } else {
            crate::i18n::tf!("The chip is different from the file at ${address:04X}: the file has ${file:02X}, the chip ${chip:02X}.")
        };
    }
    if !succeeded {
        let last = output.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
        return crate::i18n::tf!("minipro reported a problem: {last}");
    }
    match action {
        Action::Detect => found(output).map_or_else(|| crate::i18n::tr("minipro answered, but didn't say which programmer it found.").into(), |f| crate::i18n::tf!("Found {f}.")),
        Action::List(_) => {
            let count = read_list(output).len();
            crate::i18n::tf!("{count} chips listed.")
        }
        Action::Info => size_from_info(output).map_or_else(|| crate::i18n::tr("The chip's details are below.").into(), |size| crate::i18n::tf!("The chip holds {size} bytes.")),
        Action::Read(_) => crate::i18n::tr("Read. The chip's contents are open in a new tab.").into(),
        Action::Write(..) if output.contains("Verification OK") => crate::i18n::tr("Written, and verified: the chip matches the file.").into(),
        Action::Write(..) => crate::i18n::tr("Written.").into(),
        Action::Verify(..) => crate::i18n::tr("Verified: the chip matches the file.").into(),
        Action::BlankCheck => crate::i18n::tr("The chip is blank.").into(),
        Action::Erase => crate::i18n::tr("Erased.").into(),
        Action::PinCheck => crate::i18n::tr("Every pin is making contact.").into(),
    }
}

/// minipro's output so far, with its progress lines (rewritten in place with carriage returns) as they end up.
#[derive(Default)]
pub struct Output {
    pub text: String,
    line_start: usize,
    after_cr: bool,
}

impl Output {
    pub fn push(&mut self, bytes: &[u8]) {
        for c in String::from_utf8_lossy(bytes).chars() {
            match c {
                // The next characters overwrite this line, as on a terminal; a CR LF just ends it.
                '\r' => self.after_cr = true,
                '\n' => {
                    self.after_cr = false;
                    self.text.push('\n');
                    self.line_start = self.text.len();
                }
                _ => {
                    if std::mem::take(&mut self.after_cr) {
                        self.text.truncate(self.line_start);
                    }
                    self.text.push(c);
                }
            }
        }
    }
}

/// minipro running in the background.
pub struct Job {
    pub action: Action,
    pub output: Arc<Mutex<Output>>,
    /// Whether it succeeded, once it's finished; or why it couldn't be started.
    pub finished: Arc<Mutex<Option<Result<bool, String>>>>,
    child: Arc<Mutex<Option<std::process::Child>>>,
}

impl Job {
    pub fn start(program: &Path, prefs: &Prefs, action: Action) -> Job {
        let output = Arc::new(Mutex::new(Output::default()));
        let finished = Arc::new(Mutex::new(None));
        let child_slot = Arc::new(Mutex::new(None));
        let arguments = arguments(prefs, &action);
        {
            let mut shown = lock(&output);
            shown.push(format!("$ minipro {}\n", arguments.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ")).as_bytes());
        }
        // Nothing to type into: a minipro that stops to ask a question ends instead of waiting for ever.
        let spawned = Command::new(program).args(&arguments).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        match spawned {
            Err(e) => *lock(&finished) = Some(Err(crate::i18n::tf!("minipro couldn't be started: {e}"))),
            Ok(mut child) => {
                let readers: Vec<_> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
                    .into_iter()
                    .flatten()
                    .map(|mut stream| {
                        let output = output.clone();
                        std::thread::spawn(move || {
                            let mut buffer = [0u8; 1024];
                            while let Ok(n) = stream.read(&mut buffer) {
                                if n == 0 {
                                    break;
                                }
                                lock(&output).push(&buffer[..n]);
                            }
                        })
                    })
                    .collect();
                *lock(&child_slot) = Some(child);
                let (slot, done) = (child_slot.clone(), finished.clone());
                std::thread::spawn(move || {
                    for reader in readers {
                        let _ = reader.join();
                    }
                    // Waited for a little at a time, so that Stop can take the child to end it meanwhile.
                    let result = loop {
                        let status = lock(&slot).as_mut().map(|c| c.try_wait());
                        match status {
                            Some(Ok(Some(status))) => break Ok(status.success()),
                            Some(Ok(None)) => std::thread::sleep(std::time::Duration::from_millis(20)),
                            Some(Err(e)) => break Err(e.to_string()),
                            None => break Ok(false),
                        }
                    };
                    *lock(&done) = Some(result);
                });
            }
        }
        Job { action, output, finished, child: child_slot }
    }

    pub fn done(&self) -> Option<Result<bool, String>> {
        lock(&self.finished).clone()
    }

    pub fn text(&self) -> String {
        lock(&self.output).text.clone()
    }

    /// Stops minipro; what it was doing to the chip is left unfinished.
    pub fn stop(&self) {
        if let Some(child) = lock(&self.child).as_mut() {
            let _ = child.kill();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    /// A stand-in for minipro, since no programmer is attached here: a shell script that writes down its
    /// arguments and answers as minipro does, its messages taken from minipro's source (src/main.c,
    /// src/minipro.c, src/usb_nix.c and src/database.c). The chip is a file beside it, 32K of 0xFF to begin
    /// with, as an erased AT28C256 is.
    #[cfg(unix)]
    pub(crate) fn fake_minipro(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).unwrap();
        let script = r#"#!/bin/sh
here=$(dirname "$0")
echo "$@" >> "$here/arguments"
chip="$here/chip.bin"
[ -f "$chip" ] || head -c 32768 /dev/zero | tr '\000' '\377' > "$chip"
if [ -f "$here/unplugged" ]; then echo "No programmer found." >&2; exit 1; fi
found() {
    echo "Found T48 01.1.31 (0x11f)" >&2
    echo "Warning: T48 support is not yet complete!" >&2
    echo "Device code: 46A16257" >&2
    echo "Serial code: HSSCVO9LARFMOYKYOMVE5123" >&2
    echo "USB speed: 480Mbps (USB 2.0)" >&2
    echo "Supply voltage: 5.08 V (USB)" >&2
}
mode=""; device=""; file=""
while [ $# -gt 0 ]; do
    case "$1" in
        -k) found; exit 0 ;;
        -l) mode=list ;;
        -q) shift ;;
        -d) shift; device="$1"; mode=info ;;
        -p) shift; device="$1" ;;
        -r) shift; file="$1"; mode=read ;;
        -w) shift; file="$1"; mode=write ;;
        -m) shift; file="$1"; mode=verify ;;
        -b) mode=blank ;;
        -E) mode=erase ;;
        -z) mode=pins ;;
    esac
    shift
done
case "$mode" in
    list) printf 'AM2764A@DIP28\nAT28C16\nAT28C256\nAT28C256@PLCC32\nM27C256B@DIP28\nW27C512@DIP28\nSST39SF040@PLCC32\n'; echo "Devices: 7" >&2; exit 0 ;;
esac
case "$device" in
    AT28C256|AT28C256@PLCC32) ;;
    *) printf '\nDevice %s not found!\n' "$device" >&2; exit 1 ;;
esac
case "$mode" in
    info) printf -- '---------------Chip Info----------------\nName: AT28C256\nAvailable on: TL866A/CS\nTL866II\nT48\nT56\nMemory: 32768 Bytes\nPackage: DIP28\nProtocol: 0x07\nRead buffer size: 128 Bytes\nWrite buffer size: 64 Bytes\n' ;;
    read) found; printf 'Chip ID: 0x1E08  OK\nReading Code...  0.43Sec  OK\n' >&2; cp "$chip" "$file" ;;
    write)
        found
        size=$(wc -c < "$file")
        if [ "$size" -ne 32768 ]; then printf 'Incorrect file size: %s (needed 32768, use -s/S to ignore)\n' "$size" >&2; exit 1; fi
        printf 'Chip ID: 0x1E08  OK\nErasing... 0.02Sec  OK\n' >&2
        printf 'Writing Code...  1%%\rWriting Code...  50%%\rWriting Code...  2.85Sec  OK\n' >&2
        cp "$file" "$chip"
        printf 'Reading Code...  0.45Sec  OK\nVerification OK\n' >&2 ;;
    verify)
        found
        printf 'Reading Code...  0.45Sec  OK\n' >&2
        if cmp -s "$file" "$chip"; then echo "Verification OK" >&2; else
            at=$(cmp "$file" "$chip" | sed 's/.*byte \([0-9]*\).*/\1/')
            at=$((at - 1))
            f=$(od -An -tx1 -j "$at" -N1 "$file" | tr -d ' ')
            d=$(od -An -tx1 -j "$at" -N1 "$chip" | tr -d ' ')
            printf 'Verification failed at address 0x%04X: File=0x%s, Device=0x%s\n' "$at" "$(echo $f | tr a-f A-F)" "$(echo $d | tr a-f A-F)" >&2
            exit 1
        fi ;;
    blank)
        found
        if [ -z "$(tr -d '\377' < "$chip" | head -c 1)" ]; then echo "Code memory section is blank." >&2; else
            first=$(od -An -tx1 -v "$chip" | tr -s ' ' '\n' | grep -v '^$' | grep -n -v '^ff$' | head -1)
            at=$(( ${first%%:*} - 1 )); d=$(echo ${first#*:} | tr a-f A-F)
            printf 'Verification failed at address 0x%04X: File=0xFF, Device=0x%s\n' "$at" "$d" >&2
            exit 1
        fi ;;
    erase) found; printf 'Erasing... 0.10Sec  OK\n' >&2; head -c 32768 /dev/zero | tr '\000' '\377' > "$chip" ;;
    pins) found; echo "Pin test passed." >&2 ;;
esac
exit 0
"#;
        let path = dir.join("minipro");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    // Used by the tests with the stand-in minipro, a shell script, so on Unix only.
    #[cfg(unix)]
    pub(crate) fn finish(job: &Job) -> bool {
        let began = Instant::now();
        loop {
            if let Some(done) = job.done() {
                return done.unwrap();
            }
            assert!(began.elapsed() < Duration::from_secs(20), "minipro never finished");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // Used by the tests with the stand-in minipro, a shell script, so on Unix only.
    #[cfg(unix)]
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-eprom-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn arguments_are_minipro_s() {
        let prefs = Prefs { chip: "AT28C256".into(), ..Prefs::default() };
        let file = PathBuf::from("/tmp/rom.bin");
        let args = |action: Action, prefs: &Prefs| arguments(prefs, &action).join(" ");
        assert_eq!(args(Action::Detect, &prefs), "-k");
        assert_eq!(args(Action::List(Programmer::Attached), &prefs), "-q TL866II -l");
        assert_eq!(args(Action::List(Programmer::Tl866a), &prefs), "-q TL866A -l");
        assert_eq!(args(Action::List(Programmer::T48), &prefs), "-q T48 -l");
        assert_eq!(args(Action::Info, &prefs), "-d AT28C256");
        assert_eq!(args(Action::Read(file.clone()), &prefs), "-p AT28C256 -r /tmp/rom.bin");
        assert_eq!(args(Action::Write(file.clone(), None), &prefs), "-p AT28C256 -w /tmp/rom.bin");
        assert_eq!(args(Action::Verify(file.clone(), Some(FileFormat::IntelHex)), &prefs), "-p AT28C256 -m /tmp/rom.bin -f ihex");
        assert_eq!(args(Action::BlankCheck, &prefs), "-p AT28C256 -b");
        assert_eq!(args(Action::Erase, &prefs), "-p AT28C256 -E");
        assert_eq!(args(Action::PinCheck, &prefs), "-p AT28C256 -z");
        // The options: ID checks (-x reading, -y otherwise), no erase, and in-circuit.
        let careless = Prefs { chip: "W27C512@DIP28".into(), programmer: Programmer::T56, ignore_id: true, skip_erase: true, icsp: true, ..Prefs::default() };
        assert_eq!(args(Action::Read(file.clone()), &careless), "-p W27C512@DIP28 -r /tmp/rom.bin -x -i");
        assert_eq!(args(Action::Write(file.clone(), Some(FileFormat::Srecord)), &careless), "-p W27C512@DIP28 -w /tmp/rom.bin -f srec -e -y -i");
        assert_eq!(args(Action::Verify(file, None), &careless), "-p W27C512@DIP28 -m /tmp/rom.bin -y -i");
        assert_eq!(args(Action::Info, &careless), "-q T56 -d W27C512@DIP28");
    }

    #[test]
    fn settings_come_back_as_they_went() {
        let prefs = Prefs { chip: "M27C256B@DIP28".into(), programmer: Programmer::T48, fill: 0x00, placement: Placement::Repeat, ignore_id: true, skip_erase: true, icsp: false };
        let mut read = Prefs::default();
        for line in prefs.to_settings().lines() {
            let (name, value) = line.split_once('=').unwrap();
            read.read_setting(name, value);
        }
        assert_eq!(read, prefs);
    }

    #[test]
    fn minipro_s_answers_are_read() {
        // As minipro prints them (format strings from its source).
        assert_eq!(found("Found TL866II+ 04.2.132 (0x284)\nWarning: Firmware is out of date.\n"), Some("TL866II+ 04.2.132 (0x284)".into()));
        assert_eq!(Programmer::from_found("TL866II+ 04.2.132 (0x284)"), Some(Programmer::Tl866ii));
        assert_eq!(Programmer::from_found("T48 01.1.31 (0x11f)"), Some(Programmer::T48));
        assert_eq!(Programmer::from_found("TL866CS in bootloader mode."), Some(Programmer::Tl866a));
        assert_eq!(size_from_info("---------------Chip Info----------------\nName: AT28C256\nMemory: 32768 Bytes\nPackage: DIP28\n"), Some(32768));
        assert_eq!(size_from_info("Name: PIC16F84A\nMemory: 1024 Words + 64 Bytes\n"), Some(2048));
        assert_eq!(size_from_info("Name: GAL16V8\nMemory: 2194 Bits\n"), Some(274));
        assert_eq!(mismatch("Reading Code...  0.45Sec  OK\nVerification failed at address 0x0010: File=0x41, Device=0xFF\n"), Some((0x10, 0x41, 0xFF)));
        assert_eq!(read_list("AT28C256\nW27C512@DIP28\n\nNo programmer found.\nDevices: 2\n"), ["AT28C256", "W27C512@DIP28"]);
        let summary_of = |action: Action, ok: bool, text: &str| summary(&action, ok, text);
        assert!(summary_of(Action::Detect, false, "No programmer found.\n").contains("No programmer was found"));
        assert!(summary_of(Action::Info, false, "\nDevice FOO not found!\n").contains("FOO"));
        assert!(summary_of(Action::Write(PathBuf::new(), None), true, "Writing Code...  2.85Sec  OK\nReading Code...  0.45Sec  OK\nVerification OK\n").contains("verified"));
        assert!(summary_of(Action::BlankCheck, false, "Verification failed at address 0x0100: File=0xFF, Device=0x12\n").contains("$0100 holds $12"));
        assert!(summary_of(Action::Write(PathBuf::new(), None), false, "Incorrect file size: 100 (needed 32768, use -s/S to ignore)\n").contains("Incorrect file size"));
    }

    #[test]
    fn sizes_are_guessed_from_names() {
        for (name, size) in [
            ("2716", 2048),
            ("AM2764A@DIP28", 8192),
            ("27C256", 32768),
            ("M27C256B@DIP28", 32768),
            ("W27C512@DIP28", 65536),
            ("AT27C010", 131_072),
            ("27C4001", 524_288),
            ("M27C801", 1_048_576),
            ("AT28C16", 2048),
            ("AT28C64B", 8192),
            ("AT28C256@PLCC32", 32768),
            ("AM29F040B@PLCC32", 524_288),
            ("SST39SF010A", 131_072),
        ] {
            assert_eq!(size_from_name(name), Some(size), "{name}");
        }
        assert_eq!(size_from_name("ATMEGA328P"), None);
        assert_eq!(size_from_name("PIC16F84A"), None);
    }

    #[test]
    fn files_are_fitted_to_the_chip() {
        assert_eq!(fit(b"AB", 6, 0xFF, Placement::Start).unwrap(), [b'A', b'B', 0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(fit(b"AB", 6, 0x00, Placement::End).unwrap(), [0, 0, 0, 0, b'A', b'B']);
        assert_eq!(fit(b"ABC", 8, 0xFF, Placement::Repeat).unwrap(), b"ABCABCAB");
        assert_eq!(fit(b"", 2, 0xEA, Placement::Repeat).unwrap(), [0xEA, 0xEA]);
        assert!(fit(&[0; 9], 8, 0xFF, Placement::Start).unwrap_err().contains("only 8"));
        assert_eq!(FileFormat::of("ROM.HEX"), Some(FileFormat::IntelHex));
        assert_eq!(FileFormat::of("kernal.s19"), Some(FileFormat::Srecord));
        assert_eq!(FileFormat::of("kernal.bin"), None);
    }

    #[test]
    fn progress_lines_end_up_as_they_finish() {
        let mut output = Output::default();
        output.push(b"Writing Code...  1%\rWriting Code...  5");
        output.push(b"0%\rWriting Code...  2.85Sec  OK\r\nVerification OK\n");
        assert_eq!(output.text, "Writing Code...  2.85Sec  OK\nVerification OK\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_rom_is_written_verified_read_and_checked_with_a_stand_in_minipro() {
        let dir = scratch("fake");
        let program = fake_minipro(&dir.join("bin"));
        assert_eq!(find_in(&[dir.clone(), dir.join("bin")]), Some(program.clone()));
        assert_eq!(find_in(std::slice::from_ref(&dir)), None);
        let run = |prefs: &Prefs, action: Action| {
            let job = Job::start(&program, prefs, action.clone());
            let ok = finish(&job);
            let text = job.text();
            (ok, summary(&action, ok, &text), text)
        };
        let mut prefs = Prefs::default();
        let (ok, said, _) = run(&prefs, Action::Detect);
        assert!(ok && said.contains("T48 01.1.31"), "{said}");
        let (ok, _, text) = run(&prefs, Action::List(Programmer::Attached));
        assert!(ok);
        let chips = read_list(&text);
        assert!(chips.contains(&"AT28C256".to_string()) && chips.len() == 7, "{chips:?}");
        prefs.chip = "AT28C256".into();
        let (_, _, text) = run(&prefs, Action::Info);
        assert_eq!(size_from_info(&text), Some(32768));
        // A blank chip, written with a ROM fitted to it, verified, then found different when the file changes.
        assert!(run(&prefs, Action::BlankCheck).1.contains("blank"));
        let rom = fit(b"\xA9\x00\x8D\x20\xD0\x60", 32768, 0xFF, Placement::End).unwrap();
        let image = dir.join("rom.bin");
        std::fs::write(&image, &rom).unwrap();
        let (ok, said, text) = run(&prefs, Action::Write(image.clone(), None));
        assert!(ok && said.contains("verified"), "{said}\n{text}");
        // The progress line, rewritten in place, ended up as minipro left it.
        assert!(text.contains("Writing Code...  2.85Sec  OK\n") && !text.contains("50%"), "{text}");
        assert!(run(&prefs, Action::Verify(image.clone(), None)).0);
        let mut changed = rom.clone();
        changed[0x7FFA] = 0x41;
        std::fs::write(&image, &changed).unwrap();
        let (ok, said, _) = run(&prefs, Action::Verify(image.clone(), None));
        assert!(!ok && said.contains("$7FFA") && said.contains("$41") && said.contains("$A9"), "{said}");
        assert!(run(&prefs, Action::BlankCheck).1.contains("isn't blank: $7FFA holds $A9"));
        // Read back, it's what was written.
        let back = dir.join("back.bin");
        assert!(run(&prefs, Action::Read(back.clone())).0);
        assert_eq!(std::fs::read(&back).unwrap(), rom);
        // Erased, it's blank again; a file of the wrong size is refused by minipro, and said so.
        assert!(run(&prefs, Action::Erase).0);
        assert!(run(&prefs, Action::BlankCheck).0);
        std::fs::write(&image, b"short").unwrap();
        let (ok, said, _) = run(&prefs, Action::Write(image.clone(), None));
        assert!(!ok && said.contains("Incorrect file size: 5"), "{said}");
        // A chip it doesn't know, and no programmer plugged in.
        prefs.chip = "NOPE".into();
        assert!(run(&prefs, Action::BlankCheck).1.contains("NOPE"));
        std::fs::write(dir.join("bin").join("unplugged"), "").unwrap();
        assert!(run(&prefs, Action::Detect).1.contains("No programmer was found"));
        // Each run's arguments, as minipro was given them.
        let log = std::fs::read_to_string(dir.join("bin").join("arguments")).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines[0], "-k");
        assert_eq!(lines[1], "-q TL866II -l");
        assert_eq!(lines[2], "-d AT28C256");
        assert_eq!(lines[4], format!("-p AT28C256 -w {}", image.display()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_program_is_said_so() {
        let job = Job::start(Path::new("/nonexistent/minipro"), &Prefs::default(), Action::Detect);
        assert!(job.done().unwrap().unwrap_err().contains("couldn't be started"));
    }
}
