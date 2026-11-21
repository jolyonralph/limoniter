//! Programs for the Sinclair QL: QDOS's file header, the two ways QL emulators keep a header on a file of another
//! computer's (the "XTcc" trailer and Q-emuLator's header), the job header a program starts with, and the starter
//! File, New as, QL program (68008) opens.
//!
//! Sources, each checked against what's written here:
//!
//! - The QL Technical Guide (Tony Tebby and David Karlin, Sinclair Research, 1985): section 3.1 for a job's header
//!   (a jump to its start, $4AFB at 6, then its name as a word count and the letters), section 7.0 for the 64-byte
//!   file header (length at 0, access at 4, type at 5, type-dependent information at 6, the name at $E, dates at
//!   $34, $38 and $3C; type 1 an executable program whose first long of information is its data space), and
//!   section 18.14 for the traps' keys used in the starter.
//! - sQLux's uxfile.c (github.com/SinclairQL/sQLux): `FillXHXtcc` reads a file's last 8 bytes, and when they start
//!   "XTcc" makes it an executable whose data space is the long after them; `FillQemulator` reads Q-emuLator's
//!   header, "]!QDOS File Header" then a reserved byte, the header's length in words (15), the access byte, the
//!   type and the data space, the file's own bytes starting after it. `QGetHeaderFromFile` gives a file's length as
//!   its bytes plus the 64 of the header, as QDOS counts it.

/// How long a QDOS file header is: the length QDOS counts in a file's length, as the logical start of the file.
pub const HEADER_LEN: usize = 64;

/// The file type of an executable program, which EXEC loads and starts as a job.
pub const EXECUTABLE: u8 = 1;

/// The data space a program is given when its source doesn't say (; data space: 4096), enough for a stack and a
/// little room besides.
pub const DATA_SPACE: u32 = 4096;

/// The mark a job has six bytes in, after the jump to its start, before its name.
pub const JOB_MARK: u16 = 0x4AFB;

/// A QDOS file header, as the Technical Guide's section 7.0 sets it out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    /// The file's length, counting these 64 bytes, as QDOS keeps it.
    pub length: u32,
    pub access: u8,
    /// 0 for anything, 1 for an executable program, 2 for a relocatable object file, 255 for a directory.
    pub kind: u8,
    /// For an executable, the data space it's given when it's started.
    pub data_space: u32,
    /// The other four bytes of type-dependent information.
    pub extra: u32,
    /// Up to 36 characters.
    pub name: String,
    pub update: u32,
    pub reference: u32,
    pub backup: u32,
}

fn long(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

impl Header {
    /// A header for a file of `size` bytes (not counting the header).
    pub fn new(name: &str, size: usize, kind: u8, data_space: u32) -> Self {
        Self { length: (size + HEADER_LEN) as u32, kind, data_space: if kind == EXECUTABLE { data_space } else { 0 }, name: name.chars().take(36).collect(), ..Self::default() }
    }

    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[0..4].copy_from_slice(&self.length.to_be_bytes());
        out[4] = self.access;
        out[5] = self.kind;
        out[6..10].copy_from_slice(&self.data_space.to_be_bytes());
        out[10..14].copy_from_slice(&self.extra.to_be_bytes());
        let name: Vec<u8> = self.name.chars().map(|c| if c.is_ascii() { c as u8 } else { b'_' }).take(36).collect();
        out[14..16].copy_from_slice(&(name.len() as u16).to_be_bytes());
        out[16..16 + name.len()].copy_from_slice(&name);
        out[52..56].copy_from_slice(&self.update.to_be_bytes());
        out[56..60].copy_from_slice(&self.reference.to_be_bytes());
        out[60..64].copy_from_slice(&self.backup.to_be_bytes());
        out
    }

    pub fn read(bytes: &[u8]) -> Option<Self> {
        let bytes = bytes.get(..HEADER_LEN)?;
        let name_len = usize::from(u16::from_be_bytes([bytes[14], bytes[15]])).min(36);
        Some(Self {
            length: long(bytes, 0),
            access: bytes[4],
            kind: bytes[5],
            data_space: long(bytes, 6),
            extra: long(bytes, 10),
            name: bytes[16..16 + name_len].iter().map(|b| *b as char).collect(),
            update: long(bytes, 52),
            reference: long(bytes, 56),
            backup: long(bytes, 60),
        })
    }
}

/// How a QDOS header was kept on a file away from the QL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keeping {
    /// "XTcc" and the data space, as the last 8 bytes.
    Xtcc,
    /// Q-emuLator's header before the file's bytes.
    Qemulator,
}

/// A program as it was kept away from the QL, with its header in one of those ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kept<'a> {
    pub code: &'a [u8],
    pub kind: u8,
    pub data_space: u32,
    pub keeping: Keeping,
}

/// What Q-emuLator's header starts with.
pub const QEMULATOR_MARK: &[u8; 18] = b"]!QDOS File Header";

/// The bytes of a program with the XTcc trailer after them, as sQLux reads an executable's header from a file.
pub fn with_xtcc(code: &[u8], data_space: u32) -> Vec<u8> {
    let mut out = code.to_vec();
    out.extend_from_slice(b"XTcc");
    out.extend_from_slice(&data_space.to_be_bytes());
    out
}

/// The bytes of a file with Q-emuLator's short header before them: 30 bytes, 15 words, as sQLux writes it.
pub fn with_qemulator_header(code: &[u8], kind: u8, data_space: u32) -> Vec<u8> {
    let mut out = QEMULATOR_MARK.to_vec();
    out.extend_from_slice(&[0, 15, 0, kind]);
    out.extend_from_slice(&data_space.to_be_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(code);
    out
}

/// A file's QDOS header, if it was kept with one: Q-emuLator's at its start, or XTcc at its end.
pub fn kept(bytes: &[u8]) -> Option<Kept<'_>> {
    if bytes.starts_with(QEMULATOR_MARK) && bytes.len() >= 30 {
        let header_len = usize::from(bytes[19]) * 2;
        if (30..=bytes.len()).contains(&header_len) {
            return Some(Kept { code: &bytes[header_len..], kind: bytes[21], data_space: long(bytes, 22), keeping: Keeping::Qemulator });
        }
    }
    let at = bytes.len().checked_sub(8)?;
    (&bytes[at..at + 4] == b"XTcc").then(|| Kept { code: &bytes[..at], kind: EXECUTABLE, data_space: long(bytes, at + 4), keeping: Keeping::Xtcc })
}

/// Whether a file is a QL program kept with its header, so it can be run as one.
pub fn is_program(bytes: &[u8]) -> bool {
    kept(bytes).is_some_and(|k| k.kind == EXECUTABLE && !k.code.is_empty())
}

/// The name a job gives itself in its header (section 3.1), if it has one.
pub fn job_name(code: &[u8]) -> Option<String> {
    if code.get(6..8)? != JOB_MARK.to_be_bytes() {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([*code.get(8)?, *code.get(9)?]));
    let name = code.get(10..10 + len)?;
    Some(name.iter().map(|b| *b as char).collect())
}

/// A name as QDOS keeps them, from a file's name elsewhere: letters, digits and underscores, the ending (if any)
/// after an underscore as the QL has it (hello.exe is hello_exe), and no more than 36 characters.
pub fn qdos_name(name: &str) -> String {
    let tidy: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let tidy = tidy.trim_matches('_');
    let tidy = if tidy.is_empty() { "program" } else { tidy };
    tidy.chars().take(36).collect()
}

/// The name an executable is given from its source's: the stem, with _exe after it, as QL programs are named.
pub fn program_name(stem: &str) -> String {
    let stem: String = qdos_name(stem).chars().take(32).collect();
    format!("{stem}_exe")
}

/// The text after a `; name:` comment near the top of the source, in lower case.
fn comment_value<'a>(lines: impl IntoIterator<Item = &'a str>, key: &str) -> Option<String> {
    lines.into_iter().take(50).find_map(|line| {
        let line = line.trim_start();
        let comment = line.strip_prefix(';').or_else(|| line.strip_prefix('*')).or_else(|| line.strip_prefix("//"))?;
        let comment = comment.trim_start_matches([';', '/', '*', ' ', '\t']).to_ascii_lowercase();
        Some(comment.strip_prefix(key)?.trim_start().strip_prefix(':')?.trim().to_string())
    })
}

/// Whether the source says it's for the QL near its top: ; machine: ql (or qdos, or sinclair ql).
pub fn says_ql<'a>(lines: impl IntoIterator<Item = &'a str>) -> bool {
    comment_value(lines, "machine").is_some_and(|v| matches!(v.replace(' ', "").as_str(), "ql" | "qdos" | "sinclairql" | "smsq" | "smsq/e"))
}

/// The data space the source asks for near its top (; data space: 8192, or $2000), if it says.
pub fn data_space_said<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<u32> {
    let value = comment_value(lines, "data space")?;
    let value = value.split_whitespace().next()?;
    match value.strip_prefix('$').or_else(|| value.strip_prefix("0x")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => value.replace(',', "").parse().ok(),
    }
}

/// The SuperBASIC BOOT file that starts a program from a drive as the QL boots: EXEC loads it as a job and starts
/// it (the QL User Guide's Keywords, EXEC), and EX is Toolkit II's word for the same.
pub fn boot_program(device: &str, program: &str) -> String {
    format!("10 EXEC {device}{program}\n")
}

/// The source File, New as, QL program (68008) starts with: a job that opens a console window, writes in it, waits
/// for a key and removes itself, with the traps it calls named by their QDOS names.
pub const STARTER: &str = "\
; machine: ql
; data space: 4096

        machine 68008           ; the QL's processor, for its cycle counts

; A QDOS job: F9 makes it an executable program (type 1, with its data space) and starts it with EXEC in sQLux,
; Q-emuLator or MAME's QL. It opens a console window, writes a message in it, waits for a key and removes itself.
; Each trap is the QL Technical Guide's (section 18.14): the key in d0, the channel in a0.
; A job can be loaded anywhere, so the code only uses addresses relative to the PC (lea name(pc),a1).

mt.frjob equ    $05             ; trap #1: remove a job (-1 is this one)
io.open  equ    $01             ; trap #2: open a channel
io.fbyte equ    $01             ; trap #3: fetch a byte
io.sstrg equ    $07             ; trap #3: send a string of bytes
sd.bordr equ    $0C             ; trap #3: set the window's border
sd.clear equ    $20             ; trap #3: clear the whole window

; The job's header: a jump to its start, $4AFB six bytes in, and its name (Technical Guide, section 3.1).
start   bra.s   begin
        dc.l    0
        dc.w    $4AFB
        dc.w    5
        dc.b    'Hello'
        dc.b    0

begin   moveq   #io.open,d0
        moveq   #-1,d1          ; for this job
        moveq   #0,d3           ; an old device
        lea     console(pc),a0
        trap    #2
        tst.l   d0
        bne.s   done            ; it couldn't be opened

        moveq   #sd.bordr,d0
        moveq   #4,d1           ; green
        moveq   #2,d2           ; two pixels wide
        moveq   #-1,d3          ; wait as long as it takes
        trap    #3

        moveq   #sd.clear,d0
        moveq   #-1,d3
        trap    #3

        moveq   #io.sstrg,d0
        moveq   #message_end-message,d2
        moveq   #-1,d3
        lea     message(pc),a1
        trap    #3

        moveq   #io.fbyte,d0    ; wait for a key
        moveq   #-1,d3
        trap    #3

done    moveq   #mt.frjob,d0
        moveq   #-1,d1          ; this job
        moveq   #0,d3           ; no error
        trap    #1

console dc.w    console_end-console-2
        dc.b    'con_400x60a56x80'
console_end

message dc.b    'Hello from Limoniter, on the QL.',10
        dc.b    'Press a key.'
message_end
";

#[cfg(test)]
mod tests {
    use super::*;

    /// The header as the Technical Guide lays it out, byte by byte: an executable of 100 bytes with 4K of data
    /// space, named hello_exe.
    #[test]
    fn a_header_has_each_field_where_the_technical_guide_puts_it() {
        let header = Header::new("hello_exe", 100, EXECUTABLE, 4096);
        let bytes = header.to_bytes();
        assert_eq!(&bytes[0..4], &[0, 0, 0, 164], "the length counts the header");
        assert_eq!((bytes[4], bytes[5]), (0, 1), "access, then the type: executable");
        assert_eq!(&bytes[6..10], &[0, 0, 0x10, 0], "the data space");
        assert_eq!(&bytes[10..14], &[0; 4]);
        assert_eq!(&bytes[14..16], &[0, 9], "the name's length");
        assert_eq!(&bytes[16..25], b"hello_exe");
        assert!(bytes[25..].iter().all(|b| *b == 0));
        assert_eq!(Header::read(&bytes), Some(header.clone()));
        // A data file has no data space.
        assert_eq!(Header::new("notes", 10, 0, 4096).data_space, 0);
    }

    /// The XTcc trailer and Q-emuLator's header, against a file as sQLux's own distribution has it (mdv2/rotate_code,
    /// whose first 30 bytes are 5D 21 51 44 4F 53 ... 00 0F 00 01 00 00 01 F4 00 00 00 00: type 1, 500 bytes of data
    /// space).
    #[test]
    fn headers_kept_by_emulators_are_read_and_written() {
        let code = [0x60, 0x00, 0x00, 0x72, 0x4E, 0x75];
        let qemulator = with_qemulator_header(&code, EXECUTABLE, 500);
        let mut expected = b"]!QDOS File Header".to_vec();
        expected.extend([0x00, 0x0F, 0x00, 0x01, 0x00, 0x00, 0x01, 0xF4, 0, 0, 0, 0]);
        assert_eq!(&qemulator[..30], expected.as_slice());
        assert_eq!(kept(&qemulator), Some(Kept { code: &code, kind: 1, data_space: 500, keeping: Keeping::Qemulator }));
        let xtcc = with_xtcc(&code, 4096);
        assert_eq!(&xtcc[6..], b"XTcc\x00\x00\x10\x00");
        assert_eq!(kept(&xtcc), Some(Kept { code: &code, kind: 1, data_space: 4096, keeping: Keeping::Xtcc }));
        assert!(is_program(&xtcc) && is_program(&qemulator));
        assert!(!is_program(&code) && kept(b"XTc").is_none());
        // A data file with Q-emuLator's header isn't a program.
        assert!(!is_program(&with_qemulator_header(b"text", 0, 0)));
    }

    #[test]
    fn names_follow_the_ql_s_ways() {
        assert_eq!(program_name("hello"), "hello_exe");
        assert_eq!(qdos_name("my game.bas"), "my_game_bas");
        assert_eq!(qdos_name("..."), "program");
        assert_eq!(qdos_name(&"x".repeat(50)).len(), 36);
        assert_eq!(boot_program("mdv1_", "hello_exe"), "10 EXEC mdv1_hello_exe\n");
    }

    #[test]
    fn the_source_says_it_is_for_the_ql_and_how_much_data_space() {
        assert!(says_ql(STARTER.lines()));
        assert!(says_ql(["  ; Machine: Sinclair QL"]));
        assert!(!says_ql(["; machine: c64"]));
        assert_eq!(data_space_said(STARTER.lines()), Some(4096));
        assert_eq!(data_space_said(["* data space: $2000"]), Some(0x2000));
        assert_eq!(data_space_said(["; data space: 10,000 bytes"]), Some(10_000));
        assert_eq!(data_space_said(["; nothing"]), None);
    }

    #[test]
    fn a_job_s_header_gives_its_name() {
        let code = [0x60, 0x0C, 0, 0, 0, 0, 0x4A, 0xFB, 0, 5, b'H', b'e', b'l', b'l', b'o'];
        assert_eq!(job_name(&code).as_deref(), Some("Hello"));
        assert_eq!(job_name(&code[..8]), None);
        assert_eq!(job_name(&[0x4E, 0x75]), None);
    }
}
