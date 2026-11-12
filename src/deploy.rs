//! Sending a finished file to the machine it's for, as a copy: the document keeps saving where it always does.
//!
//! A device is one of:
//! - a card or stick found by its volume label, wherever the system has put it today (a Gotek's USB stick, an
//!   SD2IEC, a MiSTer's or an Amiga's CF or SD card);
//! - any folder (a shared network folder, an emulator's folder, a TNFS server's folder for FujiNet);
//! - an SFTP server (a MiSTer, a Raspberry Pi, any Unix machine);
//! - an FTP server (the Ultimate 64 and 1541 Ultimate, an Amiga running an FTP server);
//! - a ZX Spectrum Next over Wi-Fi: Limoniter serves the file, and `.sync` on the Next fetches it (NextSync);
//! - an Ultimate 64 or 1541 Ultimate, told to run the program, play the tune or mount the disk at once;
//! - a command line of the person's own, for anything else (an emulator, a serial transfer program).
//!
//! A copy is written under a temporary name, flushed to the card, and only then renamed, so pulling a card out
//! straight afterwards never leaves half a file.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

const TIMEOUT: Duration = Duration::from_secs(15);
const CHUNK: usize = 256 * 1024;
const CREDENTIAL_SERVICE: &str = "Limoniter devices";
pub const NEXTSYNC_PORT: u16 = 2048;
const TEMP_ENDING: &str = ".limoniter-tmp";

// ---------- Devices ----------

/// Where a device is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// A card or stick, found by its volume label, and a folder on it ("" for its top).
    Card { label: String, folder: String },
    Folder { path: PathBuf },
    /// sftp://user@host:port/folder, and a key file to sign in with if there is one.
    Sftp { address: String, key_file: String },
    Ftp { host: String, port: u16, user: String, folder: String },
    /// A folder on the Next's card, such as /dot or /games/mine.
    NextSync { folder: String },
    Ultimate { host: String },
    /// Run with the file written to a folder of its own: {file} is its full path, {name} its name, {folder} its folder.
    Command { line: String },
}

impl Place {
    pub const KINDS: [&'static str; 7] = ["Card or stick", "Folder", "SFTP server", "FTP server", "ZX Spectrum Next (NextSync)", "Ultimate 64 / 1541 Ultimate", "Command line"];

    pub fn kind(&self) -> usize {
        match self {
            Place::Card { .. } => 0,
            Place::Folder { .. } => 1,
            Place::Sftp { .. } => 2,
            Place::Ftp { .. } => 3,
            Place::NextSync { .. } => 4,
            Place::Ultimate { .. } => 5,
            Place::Command { .. } => 6,
        }
    }

    pub fn blank(kind: usize) -> Place {
        match kind {
            0 => Place::Card { label: String::new(), folder: String::new() },
            1 => Place::Folder { path: PathBuf::new() },
            2 => Place::Sftp { address: "sftp://".into(), key_file: String::new() },
            3 => Place::Ftp { host: String::new(), port: 21, user: String::new(), folder: String::new() },
            4 => Place::NextSync { folder: "/".into() },
            5 => Place::Ultimate { host: String::new() },
            _ => Place::Command { line: String::new() },
        }
    }

    /// What it is, in a few words, for menus.
    pub fn describe(&self) -> String {
        match self {
            Place::Card { label, folder } => format!("card \u{201C}{label}\u{201D}{}", if folder.is_empty() { String::new() } else { format!(", {folder}") }),
            Place::Folder { path } => path.display().to_string(),
            Place::Sftp { address, .. } => address.clone(),
            Place::Ftp { host, port, user, folder } => format!("ftp://{}{host}{}/{}", if user.is_empty() { String::new() } else { format!("{user}@") }, if *port == 21 { String::new() } else { format!(":{port}") }, folder.trim_start_matches('/')),
            Place::NextSync { folder } => format!("Next, {folder} (by .sync)"),
            Place::Ultimate { host } => format!("Ultimate at {host}"),
            Place::Command { line } => line.clone(),
        }
    }

    /// The name a stored password is kept under, for the kinds that can have one.
    pub fn secret_key(&self) -> Option<String> {
        match self {
            Place::Ftp { host, port, user, .. } => Some(format!("ftp:{user}@{host}:{port}")),
            Place::Ultimate { host } => Some(format!("ultimate:{host}")),
            _ => None,
        }
    }
}

/// A device kept by name, and what's done to a file on its way there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub place: Place,
    /// A system preset (from the encoding menu) the text is written in, or None to send it as it's saved.
    pub preset: Option<String>,
    /// Eject a card once the file is on it.
    pub eject: bool,
}

const SEP: char = '\u{1}';

impl Device {
    pub fn to_setting(&self) -> String {
        let place: Vec<String> = match &self.place {
            Place::Card { label, folder } => vec!["card".into(), label.clone(), folder.clone()],
            Place::Folder { path } => vec!["folder".into(), path.display().to_string()],
            Place::Sftp { address, key_file } => vec!["sftp".into(), address.clone(), key_file.clone()],
            Place::Ftp { host, port, user, folder } => vec!["ftp".into(), host.clone(), port.to_string(), user.clone(), folder.clone()],
            Place::NextSync { folder } => vec!["nextsync".into(), folder.clone()],
            Place::Ultimate { host } => vec!["ultimate".into(), host.clone()],
            Place::Command { line } => vec!["command".into(), line.clone()],
        };
        let mut parts = vec![self.name.clone(), self.preset.clone().unwrap_or_default(), if self.eject { "eject".into() } else { String::new() }];
        parts.extend(place);
        format!("send_device={}\n", parts.join(&SEP.to_string()))
    }

    pub fn from_setting(value: &str) -> Option<Device> {
        let parts: Vec<&str> = value.split(SEP).collect();
        let [name, preset, eject, kind, rest @ ..] = parts.as_slice() else {
            return None;
        };
        let text = |i: usize| rest.get(i).map(|s| (*s).to_string()).unwrap_or_default();
        let place = match *kind {
            "card" => Place::Card { label: text(0), folder: text(1) },
            "folder" => Place::Folder { path: PathBuf::from(text(0)) },
            "sftp" => Place::Sftp { address: text(0), key_file: text(1) },
            "ftp" => Place::Ftp { host: text(0), port: text(1).parse().unwrap_or(21), user: text(2), folder: text(3) },
            "nextsync" => Place::NextSync { folder: text(0) },
            "ultimate" => Place::Ultimate { host: text(0) },
            "command" => Place::Command { line: text(0) },
            _ => return None,
        };
        Some(Device { name: (*name).to_string(), place, preset: (!preset.is_empty()).then(|| (*preset).to_string()), eject: *eject == "eject" })
    }

    /// What stands in the way of using it as it's filled in, if anything.
    pub fn problem(&self) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("Give it a name.".into());
        }
        match &self.place {
            Place::Card { label, .. } if label.trim().is_empty() => Some("Say which card: its volume label.".into()),
            Place::Folder { path } if path.as_os_str().is_empty() => Some("Choose the folder.".into()),
            Place::Sftp { address, .. } => crate::remote::parse_url(address).err(),
            Place::Ftp { host, .. } | Place::Ultimate { host } if host.trim().is_empty() => Some("Say which machine: its name or address.".into()),
            Place::Command { line } if !line.contains("{file}") => Some("The command line needs {file} where the file goes.".into()),
            _ => None,
        }
    }
}

/// A file name that's safe to make on any of the cards and servers sent to.
pub fn name_problem(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." {
        return Some("Give the file a name.".into());
    }
    if let Some(c) = name.chars().find(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()) {
        return Some(format!("A file name can't have \u{201C}{}\u{201D} in it.", c.escape_default()));
    }
    None
}

// ---------- Cards and sticks ----------

/// A drive the system has, as found now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    pub root: PathBuf,
    pub label: String,
    pub removable: bool,
    /// What it's mounted from, where that's needed to eject it.
    pub device: Option<String>,
}

#[cfg(windows)]
pub fn drives() -> Vec<Drive> {
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    use windows::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetThreadErrorMode};
    use windows::core::PCWSTR;
    let mut out = Vec::new();
    // An empty card reader mustn't put up "There is no disk in the drive".
    let mut old = Default::default();
    unsafe {
        let _ = SetThreadErrorMode(SEM_FAILCRITICALERRORS, Some(&mut old));
    }
    let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_uppercase();
    let mask = unsafe { GetLogicalDrives() };
    for n in 0..26u8 {
        if mask & (1 << n) == 0 {
            continue;
        }
        let letter = (b'A' + n) as char;
        let root = format!("{letter}:\\");
        let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
        let kind = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
        // 2 removable, and 3 fixed, which card readers and USB disks often say they are. Network drives are left
        // out: one asleep can take many seconds to answer, and a shared folder is a Folder device anyway.
        if !matches!(kind, 2 | 3) || format!("{letter}:") == system {
            continue;
        }
        let mut label = [0u16; 261];
        let ok = unsafe { GetVolumeInformationW(PCWSTR(wide.as_ptr()), Some(&mut label), None, None, None, None) };
        if ok.is_err() {
            continue;
        }
        let end = label.iter().position(|&c| c == 0).unwrap_or(label.len());
        out.push(Drive { root: PathBuf::from(&root), label: String::from_utf16_lossy(&label[..end]), removable: kind == 2, device: None });
    }
    unsafe {
        let _ = SetThreadErrorMode(old, None);
    }
    out
}

#[cfg(target_os = "macos")]
pub fn drives() -> Vec<Drive> {
    let Ok(entries) = std::fs::read_dir("/Volumes") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| !std::fs::read_link(entry.path()).is_ok_and(|to| to == Path::new("/")))
        .map(|entry| Drive { root: entry.path(), label: entry.file_name().to_string_lossy().into_owned(), removable: true, device: None })
        .collect()
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn drives() -> Vec<Drive> {
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    linux_drives(&mounts, &labels_by_device())
}

/// Labels the system knows for each device, from /dev/disk/by-label.
#[cfg(not(any(windows, target_os = "macos")))]
fn labels_by_device() -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir("/dev/disk/by-label") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let device = std::fs::canonicalize(entry.path()).ok()?;
            Some((device.display().to_string(), unescape_mount(&entry.file_name().to_string_lossy())))
        })
        .collect()
}

/// /proc/mounts and /dev/disk/by-label write some characters as \ and three octal digits.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn unescape_mount(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1..i + 4].iter().all(|b| (b'0'..=b'7').contains(b)) {
            out.push((bytes[i + 1] - b'0') * 64 + (bytes[i + 2] - b'0') * 8 + (bytes[i + 3] - b'0'));
            i += 4;
        } else if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'x') && i + 3 < bytes.len() {
            match u8::from_str_radix(&text[i + 2..i + 4], 16) {
                Ok(b) => {
                    out.push(b);
                    i += 4;
                }
                Err(_) => {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The drives in a /proc/mounts listing that a person plugs in: those under /media, /run/media and /mnt.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn linux_drives(mounts: &str, labels: &[(String, String)]) -> Vec<Drive> {
    let mut out = Vec::new();
    for line in mounts.lines() {
        let mut fields = line.split(' ');
        let (Some(device), Some(point)) = (fields.next(), fields.next()) else {
            continue;
        };
        let point = unescape_mount(point);
        let plugged = point.starts_with("/media/") || point.starts_with("/run/media/") || (point.starts_with("/mnt/") && !point.starts_with("/mnt/wsl"));
        if !plugged {
            continue;
        }
        let label = labels.iter().find(|(dev, _)| dev == device).map_or_else(|| point.rsplit('/').next().unwrap_or_default().to_string(), |(_, label)| label.clone());
        out.push(Drive { root: PathBuf::from(&point), label, removable: true, device: device.starts_with("/dev/").then(|| device.to_string()) });
    }
    out
}

/// The drive with this label, if it's plugged in now. Labels are matched without regard to case, as FAT keeps
/// them in capitals whatever was typed.
pub fn find_card(label: &str) -> Option<Drive> {
    let label = label.trim();
    drives().into_iter().find(|drive| drive.label.eq_ignore_ascii_case(label))
}

/// Asks the system to eject a card so it can be pulled out.
pub fn eject(drive: &Drive) -> Result<(), String> {
    #[cfg(windows)]
    let result = {
        use std::os::windows::process::CommandExt;
        let letter = drive.root.display().to_string().trim_end_matches('\\').to_string();
        let script = format!("$d=(New-Object -ComObject Shell.Application).Namespace(17).ParseName('{letter}'); if ($d) {{ $d.InvokeVerb('Eject') }} else {{ exit 1 }}");
        std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).creation_flags(0x0800_0000).output()
    };
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("diskutil").arg("eject").arg(&drive.root).output();
    #[cfg(not(any(windows, target_os = "macos")))]
    let result = match &drive.device {
        Some(device) => std::process::Command::new("udisksctl").args(["unmount", "-b", device]).output().inspect(|out| {
            if out.status.success() {
                // Powering it off is a nicety; the card is safe once it's unmounted.
                let _ = std::process::Command::new("udisksctl").args(["power-off", "-b", device]).output();
            }
        }),
        None => std::process::Command::new("umount").arg(&drive.root).output(),
    };
    match result {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

// ---------- What's sent ----------

/// The file's bytes, or a file on disk too large to hold, which is copied as it is.
#[derive(Clone, Debug)]
pub enum Payload {
    Bytes(Arc<Vec<u8>>),
    File(PathBuf),
}

impl Payload {
    pub fn len(&self) -> u64 {
        match self {
            Payload::Bytes(bytes) => bytes.len() as u64,
            Payload::File(path) => std::fs::metadata(path).map_or(0, |m| m.len()),
        }
    }

    fn reader(&self) -> std::io::Result<Box<dyn Read + Send>> {
        Ok(match self {
            Payload::Bytes(bytes) => Box::new(std::io::Cursor::new(SharedBytes(bytes.clone()))),
            Payload::File(path) => Box::new(std::fs::File::open(path)?),
        })
    }

    fn all(&self) -> std::io::Result<Arc<Vec<u8>>> {
        match self {
            Payload::Bytes(bytes) => Ok(bytes.clone()),
            Payload::File(path) => std::fs::read(path).map(Arc::new),
        }
    }
}

struct SharedBytes(Arc<Vec<u8>>);

impl AsRef<[u8]> for SharedBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Copies, counting as it goes, and stopping if asked.
fn pump(from: &mut dyn Read, to: &mut dyn Write, done: &AtomicU64, stop: &AtomicBool) -> Result<(), String> {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err("Stopped.".into());
        }
        let n = from.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        to.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        done.fetch_add(n as u64, Ordering::Relaxed);
    }
}

/// What a file on a card was like when last sent, to notice something else changing it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub modified: u64,
}

pub fn stamp_of_file(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    Some(Stamp { size: meta.len(), modified })
}

/// Writes into a folder under a temporary name, flushes it to the card, then renames it into place.
/// `expected` is the file there when this was last sent, if it was; a different one is left alone unless `force`.
pub fn write_into(folder: &Path, name: &str, payload: &Payload, expected: Option<Option<Stamp>>, force: bool, done: &AtomicU64, stop: &AtomicBool) -> Result<Stamp, Failure> {
    std::fs::create_dir_all(folder).map_err(|e| Failure::Failed(format!("Couldn't make the folder {}.\n\n{e}", folder.display())))?;
    let target = folder.join(name);
    if !force {
        let there = stamp_of_file(&target);
        match expected {
            // Sent before: it should still be as it was left.
            Some(was) if there.is_some() && there != was => return Err(Failure::Changed),
            // Never sent there: something of the same name is in the way.
            None if there.is_some() => return Err(Failure::AlreadyThere),
            _ => {}
        }
    }
    let temp = folder.join(format!("{name}{TEMP_ENDING}"));
    let result = (|| {
        let mut file = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
        pump(&mut *payload.reader().map_err(|e| e.to_string())?, &mut file, done, stop)?;
        // On a card, nothing is certain to be there until it's flushed.
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&temp, &target).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Ok(dir) = std::fs::File::open(folder) {
            let _ = dir.sync_all();
        }
        Ok::<(), String>(())
    })();
    if let Err(problem) = result {
        let _ = std::fs::remove_file(&temp);
        return Err(Failure::Failed(format!("Couldn't write {}.\n\n{problem}", target.display())));
    }
    stamp_of_file(&target).ok_or_else(|| Failure::Failed(format!("{} isn't there after writing it.", target.display())))
}

/// Why a send didn't happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The file there has changed since it was last sent.
    Changed,
    /// A file of that name is already there, not one sent from here.
    AlreadyThere,
    /// An SFTP server refused, as remote files describe.
    Refused(crate::remote::Refused),
    Failed(String),
}

// ---------- FTP ----------

struct Ftp {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    peer: std::net::IpAddr,
}

impl Ftp {
    fn connect(host: &str, port: u16) -> Result<Ftp, String> {
        let stream = connect_tcp(host, port)?;
        let peer = stream.peer_addr().map_err(|e| e.to_string())?.ip();
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        let mut ftp = Ftp { reader: BufReader::new(stream), writer, peer };
        ftp.expect(&[220])?;
        Ok(ftp)
    }

    /// Reads one reply, of one line or several.
    fn reply(&mut self) -> Result<(u16, String), String> {
        let mut line = String::new();
        let mut text = String::new();
        let mut code = None;
        loop {
            line.clear();
            if self.reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("The server closed the connection.".into());
            }
            text += &line;
            let bytes = line.as_bytes();
            if bytes.len() >= 4 && bytes[..3].iter().all(u8::is_ascii_digit) {
                let this: u16 = line[..3].parse().unwrap_or(0);
                match code {
                    None if bytes[3] == b'-' => code = Some(this),
                    None => return Ok((this, text.trim_end().to_string())),
                    Some(first) if first == this && bytes[3] == b' ' => return Ok((this, text.trim_end().to_string())),
                    _ => {}
                }
            } else if code.is_none() {
                return Err(format!("The server said something unexpected: {}", line.trim_end()));
            }
        }
    }

    fn expect(&mut self, codes: &[u16]) -> Result<String, String> {
        let (code, text) = self.reply()?;
        if codes.contains(&code) { Ok(text) } else { Err(text) }
    }

    fn command(&mut self, line: &str, codes: &[u16]) -> Result<String, String> {
        self.writer.write_all(format!("{line}\r\n").as_bytes()).map_err(|e| e.to_string())?;
        self.expect(codes)
    }

    fn sign_in(&mut self, user: &str, password: &str) -> Result<(), String> {
        let user = if user.is_empty() { "anonymous" } else { user };
        self.writer.write_all(format!("USER {user}\r\n").as_bytes()).map_err(|e| e.to_string())?;
        match self.reply()? {
            (230, _) => Ok(()),
            (331 | 332, _) => self.command(&format!("PASS {password}"), &[230, 202]).map(|_| ()).map_err(|text| format!("The server didn't accept the sign-in.\n\n{text}")),
            (_, text) => Err(format!("The server didn't accept the sign-in.\n\n{text}")),
        }
    }

    /// Goes to a folder, making any part that isn't there.
    fn go_to(&mut self, folder: &str) -> Result<(), String> {
        let folder = folder.trim();
        if folder.is_empty() || self.command(&format!("CWD {folder}"), &[250]).is_ok() {
            return Ok(());
        }
        if folder.starts_with('/') {
            self.command("CWD /", &[250])?;
        }
        for part in folder.split('/').filter(|p| !p.is_empty()) {
            if self.command(&format!("CWD {part}"), &[250]).is_err() {
                self.command(&format!("MKD {part}"), &[257]).map_err(|text| format!("Couldn't make the folder {part}.\n\n{text}"))?;
                self.command(&format!("CWD {part}"), &[250])?;
            }
        }
        Ok(())
    }

    /// Opens a passive data connection. The address the server gives is ignored in favour of the one connected
    /// to, as servers behind a router often give their inside address.
    fn data(&mut self) -> Result<TcpStream, String> {
        let port = match self.command("EPSV", &[229]) {
            Ok(text) => {
                let inside = text.split('(').nth(1).and_then(|rest| rest.split(')').next()).ok_or("The server's passive reply couldn't be read.")?;
                inside.trim_matches('|').parse::<u16>().map_err(|_| "The server's passive reply couldn't be read.".to_string())?
            }
            Err(_) => {
                let text = self.command("PASV", &[227])?;
                let inside = text.split_once('(').map_or(text.get(3..).unwrap_or_default(), |(_, rest)| rest);
                let numbers: Vec<u16> = inside.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).take(6).collect();
                if numbers.len() < 6 {
                    return Err("The server's passive reply couldn't be read.".into());
                }
                numbers[4] * 256 + numbers[5]
            }
        };
        let stream = TcpStream::connect_timeout(&(self.peer, port).into(), TIMEOUT).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(TIMEOUT)).ok();
        Ok(stream)
    }

    fn store(&mut self, name: &str, payload: &Payload, done: &AtomicU64, stop: &AtomicBool) -> Result<(), String> {
        self.command("TYPE I", &[200])?;
        let mut data = self.data()?;
        self.command(&format!("STOR {name}"), &[125, 150])?;
        let sent = pump(&mut *payload.reader().map_err(|e| e.to_string())?, &mut data, done, stop);
        let _ = data.shutdown(Shutdown::Write);
        drop(data);
        sent?;
        self.expect(&[226, 250]).map(|_| ())
    }
}

fn connect_tcp(host: &str, port: u16) -> Result<TcpStream, String> {
    let addresses: Vec<_> = (host.trim(), port).to_socket_addrs().map_err(|e| format!("Couldn't find {host}.\n\n{e}"))?.collect();
    let mut last = format!("Couldn't find {host}.");
    for address in addresses {
        match TcpStream::connect_timeout(&address, TIMEOUT) {
            Ok(stream) => {
                stream.set_read_timeout(Some(TIMEOUT)).ok();
                stream.set_write_timeout(Some(TIMEOUT)).ok();
                return Ok(stream);
            }
            Err(e) => last = format!("Couldn't connect to {host}.\n\n{e}"),
        }
    }
    Err(last)
}

pub fn ftp_send(host: &str, port: u16, user: &str, password: &str, folder: &str, name: &str, payload: &Payload, done: &AtomicU64, stop: &AtomicBool) -> Result<(), String> {
    let mut ftp = Ftp::connect(host, port)?;
    ftp.sign_in(user, password)?;
    ftp.go_to(folder)?;
    ftp.store(name, payload, done, stop)?;
    let _ = ftp.command("QUIT", &[221]);
    Ok(())
}

// ---------- Ultimate 64 and 1541 Ultimate ----------

/// What the Ultimate is asked to do with a file, going by its ending.
pub fn ultimate_route(name: &str) -> Option<(&'static str, bool)> {
    let ending = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ending.as_str() {
        "prg" => ("/v1/runners:run_prg", false),
        "crt" => ("/v1/runners:run_crt", false),
        "sid" => ("/v1/runners:sidplay", false),
        "mod" => ("/v1/runners:modplay", false),
        "d64" | "g64" | "d71" | "g71" | "d81" => ("/v1/drives/a:mount?mode=readwrite", true),
        _ => return None,
    })
}

/// Sends a file to an Ultimate's REST interface (firmware 3.11 and later): a program to run, a tune to play, a
/// cartridge to start, or a disk to put in drive A.
pub fn ultimate_send(host: &str, password: &str, name: &str, payload: &Payload, done: &AtomicU64) -> Result<String, String> {
    let (route, as_form) = ultimate_route(name).ok_or_else(|| format!("An Ultimate can run .prg and .crt files, play .sid and .mod files, and mount .d64, .g64, .d71, .g71 and .d81 disks. It wouldn't know what to do with {name}."))?;
    let data = payload.all().map_err(|e| e.to_string())?;
    let (host_only, port) = match host.trim().rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() && !h.contains(':') => (h, p.parse().unwrap_or(80)),
        _ => (host.trim(), 80),
    };
    let boundary = "limoniter-6d0b8c1f2a";
    let (content_type, head, tail): (String, Vec<u8>, Vec<u8>) = if as_form {
        let safe: String = name.chars().filter(|c| *c != '"' && !c.is_control()).collect();
        (
            format!("multipart/form-data; boundary={boundary}"),
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{safe}\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes(),
            format!("\r\n--{boundary}--\r\n").into_bytes(),
        )
    } else {
        ("application/octet-stream".into(), Vec::new(), Vec::new())
    };
    let length = head.len() + data.len() + tail.len();
    let mut request = format!("POST {route} HTTP/1.1\r\nHost: {host_only}\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\nConnection: close\r\n");
    if !password.is_empty() {
        request += &format!("X-Password: {password}\r\n");
    }
    request += "\r\n";
    let mut stream = connect_tcp(host_only, port)?;
    stream.set_read_timeout(Some(Duration::from_secs(60))).ok();
    let mut send = || -> std::io::Result<()> {
        stream.write_all(request.as_bytes())?;
        stream.write_all(&head)?;
        for chunk in data.chunks(CHUNK) {
            stream.write_all(chunk)?;
            done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        }
        stream.write_all(&tail)?;
        stream.flush()
    };
    send().map_err(|e| format!("Couldn't send it to {host}.\n\n{e}"))?;
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    let answer = String::from_utf8_lossy(&answer);
    let status: u16 = answer.split(' ').nth(1).and_then(|code| code.parse().ok()).ok_or_else(|| format!("{host} didn't answer as an Ultimate does."))?;
    let body = answer.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    let errors: Vec<String> = serde_json::from_str::<serde_json::Value>(body.trim()).ok().and_then(|json| json.get("errors").and_then(|e| e.as_array()).map(|list| list.iter().filter_map(|e| e.as_str().map(str::to_string)).collect())).unwrap_or_default();
    match status {
        200..=299 if errors.is_empty() => Ok(if as_form { format!("{name} is in drive A.") } else { format!("{name} has started.") }),
        403 => Err(format!("{host} wants its network password. Edit the device to give it.")),
        404 => Err(format!("{host} doesn't know this request. Its firmware may be older than 3.11, which added it.")),
        _ => Err(format!("{host} didn't do it.\n\n{}", if errors.is_empty() { answer.lines().next().unwrap_or_default().to_string() } else { errors.join("\n") })),
    }
}

// ---------- ZX Spectrum Next, by NextSync ----------
//
// NextSync is Jari Komppa's: `.sync` on the Next connects to port 2048 on this computer and asks for files one at a
// time. Each packet is a big-endian length (payload + 5), the payload, two checksums and a packet number. With
// "Sync4" the data is run-length packed.

/// A file waiting for the Next to fetch it.
#[derive(Clone, Debug)]
pub struct Waiting {
    /// Where it goes on the Next, as /dot/foo.
    pub path: String,
    pub data: Arc<Vec<u8>>,
    /// Which send this is, so a newer one of the same file isn't taken as fetched.
    pub number: u64,
}

#[derive(Default)]
pub struct NextShared {
    pub waiting: VecDeque<Waiting>,
    /// What's happened, for the status bar and to report.
    pub status: String,
    /// Files fetched since the window last looked.
    pub fetched: Vec<String>,
    pub problem: Option<String>,
    pub connected: bool,
}

pub struct NextSync {
    pub shared: Arc<Mutex<NextShared>>,
    /// The port listened on, which a test asks the system to choose.
    #[cfg_attr(not(test), allow(dead_code))]
    pub port: u16,
    stop: Arc<AtomicBool>,
}

impl NextSync {
    /// Starts listening for the Next. Fails if something else, such as NextSync's own server, has the port.
    pub fn start(port: u16) -> Result<NextSync, String> {
        let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("Couldn't listen for the Next on port {port}. Is NextSync's own server running?\n\n{e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let shared = Arc::new(Mutex::new(NextShared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (worker_shared, worker_stop) = (shared.clone(), stop.clone());
        let port = listener.local_addr().map_or(port, |a| a.port());
        std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        lock(&worker_shared).connected = true;
                        let outcome = serve_next(stream, &worker_shared, &worker_stop);
                        let mut shared = lock(&worker_shared);
                        shared.connected = false;
                        if let Err(problem) = outcome {
                            shared.problem = Some(problem);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
                    Err(_) => std::thread::sleep(Duration::from_millis(250)),
                }
            }
        });
        Ok(NextSync { shared, port, stop })
    }

    /// Puts a file in line for the Next, in place of an older send of the same file.
    pub fn offer(&self, waiting: Waiting) {
        let mut shared = lock(&self.shared);
        shared.waiting.retain(|w| !w.path.eq_ignore_ascii_case(&waiting.path));
        shared.waiting.push_back(waiting);
    }
}

impl Drop for NextSync {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn next_packet(payload: &[u8], number: u8) -> Vec<u8> {
    let (mut sum0, mut sum1) = (0u8, 0u8);
    for &b in payload {
        sum0 ^= b;
        sum1 = sum1.wrapping_add(sum0);
    }
    let mut packet = ((payload.len() + 5) as u16).to_be_bytes().to_vec();
    packet.extend_from_slice(payload);
    packet.extend_from_slice(&[sum0, sum1, number]);
    packet
}

/// Packs as NextSync 4 does: blocks that alternate between bytes as they are and a run of one byte, each starting
/// with its length (one byte, or 0xFF and two more for 255 and over). It starts with bytes as they are.
pub fn next_pack(data: &[u8]) -> Vec<u8> {
    fn length(out: &mut Vec<u8>, n: usize) {
        if n < 0xFF {
            out.push(n as u8);
        } else {
            out.extend_from_slice(&[0xFF, (n >> 8) as u8, n as u8]);
        }
    }
    let mut out = Vec::with_capacity(data.len() + data.len() / 64 + 8);
    let mut i = 0;
    while i < data.len() {
        // Bytes as they are, up to the next run of three or more, or 8192 of them.
        let start = i;
        let mut run_at = None;
        while i < data.len() && i - start < 8192 {
            let b = data[i];
            if i + 2 < data.len() && data[i + 1] == b && data[i + 2] == b {
                run_at = Some(i);
                break;
            }
            i += 1;
        }
        length(&mut out, i - start);
        out.extend_from_slice(&data[start..i]);
        if i >= data.len() {
            break;
        }
        // A run follows a stretch as it is: a real one, or one byte long when the stretch was cut at 8192.
        let b = data[i];
        let mut n = 1;
        if run_at.is_some() {
            while i + n < data.len() && data[i + n] == b && n < 8192 {
                n += 1;
            }
        }
        length(&mut out, n);
        out.push(b);
        i += n;
    }
    out
}

/// Unpacks what next_pack packs, as `.sync` on the Next does; for checking.
#[cfg(test)]
pub fn next_unpack(packed: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut as_is = true;
    while i < packed.len() {
        let mut n = packed[i] as usize;
        i += 1;
        if n == 0xFF {
            n = ((*packed.get(i)? as usize) << 8) | *packed.get(i + 1)? as usize;
            i += 2;
        }
        if as_is {
            out.extend_from_slice(packed.get(i..i + n)?);
            i += n;
        } else {
            out.extend(std::iter::repeat_n(*packed.get(i)?, n));
            i += 1;
        }
        as_is = !as_is;
    }
    Some(out)
}

const MAX_NEXT_PAYLOAD: usize = 1024;

fn serve_next(mut stream: TcpStream, shared: &Mutex<NextShared>, stop: &AtomicBool) -> Result<(), String> {
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    stream.set_nodelay(true).ok();
    let files: Vec<Waiting> = lock(shared).waiting.iter().cloned().collect();
    let mut next_file = 0;
    let mut packed = false;
    let mut data: Vec<u8> = Vec::new();
    let mut offset = 0;
    let mut number: u8 = 0;
    let mut last = Vec::new();
    let mut fetched: Vec<(String, u64)> = Vec::new();
    let mut buffer = [0u8; 1024];
    lock(shared).status = "The Next is fetching\u{2026}".into();
    while !stop.load(Ordering::Relaxed) {
        let n = match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(format!("The connection to the Next was lost.\n\n{e}")),
        };
        let packet = match &buffer[..n] {
            b"Sync3" => {
                packed = false;
                next_packet(b"NextSync3", 0)
            }
            b"Sync4" => {
                packed = true;
                next_packet(b"NextSync4", 0)
            }
            b"Sync" | b"Sync1" | b"Sync2" => next_packet(b"Nextsync 0.8 or later needed", 0),
            // "Neex" and "Gee" are how the Next's Wi-Fi module often garbles "Next" and "Get".
            b"Next" | b"Neex" => match files.get(next_file) {
                None => {
                    // All fetched: what was sent is no longer waiting, unless it's been sent again since.
                    let mut shared = lock(shared);
                    shared.waiting.retain(|w| !fetched.iter().any(|(path, number)| *path == w.path && *number == w.number));
                    shared.fetched.extend(fetched.drain(..).map(|(path, _)| path));
                    next_packet(&[0, 0, 0, 0, 0], 0)
                }
                Some(file) => {
                    let path = file.path.as_bytes();
                    let mut header = (file.data.len() as u32).to_be_bytes().to_vec();
                    header.push(path.len().min(255) as u8);
                    header.extend_from_slice(&path[..path.len().min(255)]);
                    data = if packed { next_pack(&file.data) } else { file.data.to_vec() };
                    offset = 0;
                    number = 0;
                    fetched.push((file.path.clone(), file.number));
                    next_file += 1;
                    lock(shared).status = format!("The Next is fetching {}", file.path);
                    next_packet(&header, 0)
                }
            },
            b"Get" | b"Gee" => {
                let end = (offset + MAX_NEXT_PAYLOAD).min(data.len());
                let packet = next_packet(&data[offset..end], number);
                offset = end;
                number = number.wrapping_add(1);
                packet
            }
            b"Retry" => {
                stream.write_all(&last).map_err(|e| e.to_string())?;
                continue;
            }
            b"Restart" => {
                offset = 0;
                number = 0;
                next_packet(b"Back", 0)
            }
            b"Bye" => {
                let _ = stream.write_all(&next_packet(b"Later", 0));
                break;
            }
            _ => next_packet(b"Error", 0),
        };
        stream.write_all(&packet).map_err(|e| format!("The connection to the Next was lost.\n\n{e}"))?;
        last = packet;
    }
    let mut shared = lock(shared);
    shared.status = if shared.waiting.is_empty() { String::new() } else { format!("{} file(s) waiting for .sync on the Next", shared.waiting.len()) };
    Ok(())
}

/// This computer's address on the local network, for typing into `.sync` on the Next.
pub fn local_address() -> Option<String> {
    // Connecting a UDP socket sends nothing; it only picks the way out.
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

/// Where a file goes on the Next: its folder, then its name.
pub fn next_path(folder: &str, name: &str) -> String {
    let folder = folder.trim().replace('\\', "/");
    let folder = folder.trim_end_matches('/');
    if folder.is_empty() {
        format!("/{name}")
    } else if folder.starts_with('/') || folder.contains(':') {
        format!("{folder}/{name}")
    } else {
        format!("/{folder}/{name}")
    }
}

// ---------- Command lines ----------

/// Fills in a device's command line. Paths are quoted, as they may have spaces.
pub fn fill_command(line: &str, file: &Path) -> String {
    let quote = |text: String| if cfg!(windows) { format!("\"{text}\"") } else { format!("'{}'", text.replace('\'', "'\\''")) };
    let name = file.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let folder = file.parent().map_or_else(String::new, |f| f.display().to_string());
    line.replace("{file}", &quote(file.display().to_string())).replace("{name}", &quote(name)).replace("{folder}", &quote(folder))
}

/// Runs a device's command line. A program still running after a few seconds, such as an emulator, is left to
/// run; one that ends at once with a failure says why.
fn run_command(line: &str, file: &Path) -> Result<String, String> {
    let filled = fill_command(line, file);
    let log = file.with_extension("limoniter-log");
    let out = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut child = crate::platform::shell_command(&filled).stdout(out).stderr(err).stdin(std::process::Stdio::null()).spawn().map_err(|e| format!("Couldn't run it.\n\n{e}"))?;
    let began = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            let said = std::fs::read_to_string(&log).unwrap_or_default();
            let said: String = said.trim().chars().rev().take(2000).collect::<Vec<_>>().into_iter().rev().collect();
            return if status.success() { Ok(said) } else { Err(format!("The command line failed ({status}).\n\n{said}")) };
        }
        if began.elapsed() > Duration::from_secs(5) {
            return Ok("It's still running.".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ---------- Stored passwords ----------

pub fn stored_password(key: &str) -> Option<String> {
    keyring::Entry::new(CREDENTIAL_SERVICE, key).ok()?.get_password().ok()
}

pub fn store_password(key: &str, password: &str) -> Result<(), String> {
    keyring::Entry::new(CREDENTIAL_SERVICE, key).and_then(|entry| entry.set_password(password)).map_err(|e| e.to_string())
}

pub fn forget_password(key: &str) {
    if let Ok(entry) = keyring::Entry::new(CREDENTIAL_SERVICE, key) {
        let _ = entry.delete_credential();
    }
}

// ---------- Sending in the background ----------

/// Everything a send needs, worked out before it starts.
#[derive(Clone, Debug)]
pub struct Sending {
    pub device: Device,
    pub name: String,
    pub payload: Payload,
    /// The file there as last sent from here, for a card, folder or SFTP server; None if never sent there.
    pub expected: Option<Option<Stamp>>,
    pub force: bool,
    /// A password typed or kept, for FTP, SFTP and the Ultimate.
    pub password: String,
    pub sftp_secret: Option<crate::remote::Secret>,
    pub known_hosts: crate::remote::KnownHosts,
    /// Where a command line's copy is written.
    pub scratch: PathBuf,
}

#[derive(Clone, Debug)]
pub enum Outcome {
    /// Sent. The stamp of the file there, where one can be had, and anything to tell.
    Sent { stamp: Option<Stamp>, note: String, place: String },
    Failed(Failure),
}

pub struct Job {
    pub doc_id: u64,
    pub sending: Sending,
    pub done: Arc<AtomicU64>,
    pub total: u64,
    stop: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Outcome>>>,
}

impl Job {
    pub fn start(doc_id: u64, sending: Sending) -> Job {
        let total = sending.payload.len();
        let (done, stop, result) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
        let (worker, worker_done, worker_stop, worker_result) = (sending.clone(), done.clone(), stop.clone(), result.clone());
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| perform(&worker, &worker_done, &worker_stop))).unwrap_or_else(|_| Outcome::Failed(Failure::Failed("It failed unexpectedly.".into())));
            *lock(&worker_result) = Some(outcome);
        });
        Job { doc_id, sending, done, total, stop, result }
    }

    pub fn take(&self) -> Option<Outcome> {
        lock(&self.result).take()
    }

    pub fn share(&self) -> f32 {
        self.done.load(Ordering::Relaxed) as f32 / self.total.max(1) as f32
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

fn perform(s: &Sending, done: &AtomicU64, stop: &AtomicBool) -> Outcome {
    let sent = |stamp: Option<Stamp>, note: String, place: String| Outcome::Sent { stamp, note, place };
    let failed = |text: String| Outcome::Failed(Failure::Failed(text));
    match &s.device.place {
        Place::Card { label, folder } => {
            let Some(drive) = find_card(label) else {
                return failed(format!("The card \u{201C}{label}\u{201D} isn't plugged in."));
            };
            let into = drive.root.join(folder.trim_start_matches(['/', '\\']));
            match write_into(&into, &s.name, &s.payload, s.expected, s.force, done, stop) {
                Ok(stamp) => {
                    let place = into.join(&s.name).display().to_string();
                    let note = if s.device.eject {
                        match eject(&drive) {
                            Ok(()) => format!("The card {label} can be taken out."),
                            Err(problem) => format!("The file is written, but the card couldn't be ejected: {problem}"),
                        }
                    } else {
                        String::new()
                    };
                    sent(Some(stamp), note, place)
                }
                Err(failure) => Outcome::Failed(failure),
            }
        }
        Place::Folder { path } => match write_into(path, &s.name, &s.payload, s.expected, s.force, done, stop) {
            Ok(stamp) => sent(Some(stamp), String::new(), path.join(&s.name).display().to_string()),
            Err(failure) => Outcome::Failed(failure),
        },
        Place::Sftp { address, key_file } => {
            let (mut site, folder) = match crate::remote::parse_url(address) {
                Ok(found) => found,
                Err(problem) => return failed(problem),
            };
            site.key_file = (!key_file.trim().is_empty()).then(|| PathBuf::from(key_file.trim()));
            let secret = s.sftp_secret.clone().unwrap_or(crate::remote::Secret::KeyPassphrase(None));
            let connection = match crate::remote::connect(&site, &secret, &s.known_hosts) {
                Ok(connection) => connection,
                Err(refused) => return Outcome::Failed(Failure::Refused(refused)),
            };
            let remote_path = crate::remote::join(&folder, &s.name);
            let local = match &s.payload {
                Payload::File(path) => path.clone(),
                Payload::Bytes(bytes) => {
                    let local = s.scratch.join(&s.name);
                    if let Err(e) = std::fs::create_dir_all(&s.scratch).and_then(|()| std::fs::write(&local, bytes.as_slice())) {
                        return failed(format!("Couldn't make the copy to send.\n\n{e}"));
                    }
                    local
                }
            };
            let there = connection.stamp(&remote_path).ok().map(|st| Stamp { size: st.size, modified: st.modified });
            if !s.force {
                match s.expected {
                    Some(was) if there.is_some() && there != was => return Outcome::Failed(Failure::Changed),
                    None if there.is_some() => return Outcome::Failed(Failure::AlreadyThere),
                    _ => {}
                }
            }
            match connection.upload(&local, &remote_path, None, true, done, stop) {
                Ok(stamp) => sent(Some(Stamp { size: stamp.size, modified: stamp.modified }), String::new(), site.url(&remote_path)),
                Err(crate::remote::UploadError::ChangedOnServer) => Outcome::Failed(Failure::Changed),
                Err(crate::remote::UploadError::Failed(problem)) => failed(problem),
            }
        }
        Place::Ftp { host, port, user, folder } => match ftp_send(host, *port, user, &s.password, folder, &s.name, &s.payload, done, stop) {
            Ok(()) => sent(None, String::new(), format!("{host}, {}", next_path(folder, &s.name))),
            Err(problem) => failed(format!("Couldn't send it to {host} by FTP.\n\n{problem}")),
        },
        Place::Ultimate { host } => match ultimate_send(host, &s.password, &s.name, &s.payload, done) {
            Ok(note) => sent(None, note, host.clone()),
            Err(problem) => failed(problem),
        },
        Place::NextSync { .. } => failed("The Next fetches files itself; they aren't sent from here.".into()),
        Place::Command { line } => {
            let file = s.scratch.join(&s.name);
            let written = std::fs::create_dir_all(&s.scratch).and_then(|()| match &s.payload {
                Payload::Bytes(bytes) => std::fs::write(&file, bytes.as_slice()),
                Payload::File(path) => std::fs::copy(path, &file).map(|_| ()),
            });
            if let Err(e) = written {
                return failed(format!("Couldn't make the copy for the command line.\n\n{e}"));
            }
            done.store(s.payload.len(), Ordering::Relaxed);
            match run_command(line, &file) {
                Ok(said) => sent(None, said, file.display().to_string()),
                Err(problem) => failed(problem),
            }
        }
    }
}

#[cfg(test)]
#[path = "deploy_tests.rs"]
mod tests;
