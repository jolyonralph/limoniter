//! Amiga disks that boot with no file system: a trackmo's or a game's own way. Code and data are assembled (or
//! taken from files) and written to chosen sectors; a boot block, made from Limoniter's example or the person's
//! own source, reads each piece from its sectors with trackdisk.device and jumps to the one that starts.
//!
//! A disk is 80 cylinders of 2 sides, each track 11 sectors of 512 bytes (22 on a high-density disk): 1760
//! sectors, numbered cylinder by cylinder, side 0 before side 1. Sectors 0 and 1 are the boot block, which
//! Kickstart reads at start-up: "DOS\0", a checksum, a root block number, and then code, run with A1 pointing at
//! trackdisk's IORequest and A6 at ExecBase. All of it has to fit in those 1024 bytes.
//!
//! The plan of a disk is kept in a text file of its own (`.bootdisk`), so it can be built again after any change.

use std::path::{Path, PathBuf};

pub const SECTOR: usize = 512;
/// The boot block's size.
pub const BOOT_BYTES: usize = 1024;

/// The lines in the boot block's source between which Limoniter writes the table of pieces each time.
pub const TABLE_FROM: &str = "; >>>pieces";
pub const TABLE_TO: &str = "; <<<pieces";

/// A disk's size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    /// 880K: 11 sectors a track.
    Double,
    /// 1760K: 22 sectors a track.
    High,
}

impl Density {
    pub fn sectors_a_track(self) -> usize {
        match self {
            Density::Double => 11,
            Density::High => 22,
        }
    }

    pub fn sectors(self) -> usize {
        80 * 2 * self.sectors_a_track()
    }

    pub fn name(self) -> &'static str {
        match self {
            Density::Double => "Double density, 880K",
            Density::High => "High density, 1760K",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Density::Double => "dd",
            Density::High => "hd",
        }
    }
}

/// Where a sector is: its cylinder, side and sector on the track.
pub fn place(density: Density, sector: usize) -> (usize, usize, usize) {
    let per = density.sectors_a_track();
    (sector / (per * 2), (sector / per) % 2, sector % per)
}

/// A piece of the disk: a program or data, put at a sector and loaded by the boot block to an address.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub name: String,
    /// An assembler source (assembled to its address) or any other file (its bytes as they are).
    pub file: PathBuf,
    /// Its first sector.
    pub sector: usize,
    /// Where the boot block loads it. A source with its own ORG is at that address instead.
    pub load: u32,
    /// Whether the boot block jumps to it once everything's loaded.
    pub start: bool,
    /// Whether the boot block loads it at all: data a program reads itself later, from its own loader, isn't.
    pub loaded: bool,
}

/// The plan of a disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub density: Density,
    pub pieces: Vec<Piece>,
    /// The boot block's own source, if it isn't Limoniter's example.
    pub boot: Option<PathBuf>,
}

impl Default for Plan {
    fn default() -> Self {
        Plan { density: Density::Double, pieces: Vec::new(), boot: None }
    }
}

/// Whether a file is assembler source, by its name, rather than bytes to put on the disk as they are.
pub fn is_source(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "s" | "asm" | "a" | "i" | "68k" | "s68" | "a68")
}

impl Plan {
    /// The plan as its file: a line a setting, a piece's fields separated by tabs. Paths are kept relative to
    /// the plan's own folder where they can be.
    pub fn to_text(&self, folder: Option<&Path>) -> String {
        let shown = |path: &Path| folder.and_then(|f| path.strip_prefix(f).ok()).unwrap_or(path).display().to_string();
        let mut out = String::from("# A Limoniter plan of an Amiga disk that boots with no file system.\n");
        out.push_str(&format!("density = {}\n", self.density.id()));
        if let Some(boot) = &self.boot {
            out.push_str(&format!("boot = {}\n", shown(boot)));
        }
        for p in &self.pieces {
            let mut flags = Vec::new();
            if p.start {
                flags.push("start");
            }
            if !p.loaded {
                flags.push("not-loaded");
            }
            out.push_str(&format!("piece = {}\t{}\t{}\t${:X}\t{}\n", p.name, shown(&p.file), p.sector, p.load, flags.join(",")));
        }
        out
    }

    pub fn from_text(text: &str, folder: Option<&Path>) -> Result<Plan, String> {
        let full = |path: &str| {
            let path = PathBuf::from(path.trim());
            match folder {
                Some(f) if path.is_relative() => f.join(path),
                _ => path,
            }
        };
        let mut plan = Plan::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim_end();
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(crate::i18n::tf!("Line {} of the plan isn't a setting: {line}", n + 1));
            };
            let value = value.trim_start();
            match key.trim() {
                "density" => plan.density = if value.trim() == "hd" { Density::High } else { Density::Double },
                "boot" => plan.boot = Some(full(value)),
                "piece" => {
                    let fields: Vec<&str> = value.split('\t').collect();
                    let (Some(name), Some(file), Some(sector), Some(load)) = (fields.first(), fields.get(1), fields.get(2).and_then(|s| s.trim().parse().ok()), fields.get(3).and_then(|s| number(s))) else {
                        return Err(crate::i18n::tf!("Line {} of the plan isn't a piece: {line}", n + 1));
                    };
                    let flags = fields.get(4).copied().unwrap_or("");
                    plan.pieces.push(Piece { name: name.to_string(), file: full(file), sector, load, start: flags.split(',').any(|f| f.trim() == "start"), loaded: !flags.split(',').any(|f| f.trim() == "not-loaded") });
                }
                _ => {}
            }
        }
        Ok(plan)
    }

    /// The first run of free sectors long enough for `count`, after the boot block, given what's taken.
    pub fn next_free(&self, taken: &[Owner], count: usize) -> Option<usize> {
        let count = count.max(1);
        (2..=self.density.sectors().saturating_sub(count)).find(|&s| taken[s..s + count].iter().all(|o| *o == Owner::Free))
    }
}

/// A number as the plan or a person writes it: $40000, 0x40000 or 262144.
pub fn number(text: &str) -> Option<u32> {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix('$').or_else(|| t.strip_prefix("0x")).or_else(|| t.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        t.parse().ok()
    }
}

/// What a sector holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Free,
    Boot,
    /// A piece, by its place in the plan.
    Piece(usize),
    /// More than one piece wants it.
    Clash,
}

/// A piece, made: its bytes and where they go.
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    pub bytes: Vec<u8>,
    /// Where it's loaded: its ORG, or the plan's address.
    pub load: u32,
    /// Where it starts, if it's started: its END label or first byte.
    pub entry: u32,
}

impl Made {
    pub fn sectors(&self) -> usize {
        self.bytes.len().div_ceil(SECTOR).max(1)
    }
}

/// Assembles 68000 source to bytes at an address: at its own ORG if it has one, else at `load`.
pub fn assemble(path: &Path, text: &str, load: u32, read: &dyn Fn(&Path) -> Result<Vec<u8>, String>) -> Result<Made, String> {
    let isa = crate::asm::m68k::M68k;
    let mut assembly = crate::asm::assemble(&isa, path, text, read);
    if assembly.ok() && !assembly.absolute {
        assembly = crate::asm::assemble_at(&isa, path, text, read, i64::from(load));
    }
    if let Some(problem) = assembly.problems.first() {
        let more = assembly.problems.len() - 1;
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return Err(if more == 0 { crate::i18n::tf!("{file}, line {}: {}", problem.line + 1, problem.message) } else { crate::i18n::tf!("{file}, line {}: {} (and {more} more problems)", problem.line + 1, problem.message) });
    }
    let (Some(low), Some(high)) = (assembly.lowest(), assembly.highest()) else {
        return Err(crate::i18n::tf!("{} makes no bytes.", path.display()));
    };
    Ok(Made { bytes: assembly.image(low, high), load: low as u32, entry: assembly.start().unwrap_or(low) as u32 })
}

/// A piece made from its file: assembled, or read as it is.
pub fn make_piece(piece: &Piece, read: &dyn Fn(&Path) -> Result<Vec<u8>, String>) -> Result<Made, String> {
    let bytes = read(&piece.file)?;
    if is_source(&piece.file) {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        assemble(&piece.file, &text, piece.load, read)
    } else if bytes.is_empty() {
        Err(crate::i18n::tf!("{} is empty.", piece.file.display()))
    } else {
        Ok(Made { bytes, load: piece.load, entry: piece.load })
    }
}

/// Which sectors are whose: the boot block's, and each made piece's from its first sector on.
pub fn map(plan: &Plan, made: &[Option<Made>]) -> Vec<Owner> {
    let mut owners = vec![Owner::Free; plan.density.sectors()];
    owners[0] = Owner::Boot;
    owners[1] = Owner::Boot;
    for (i, piece) in plan.pieces.iter().enumerate() {
        let count = made.get(i).and_then(Option::as_ref).map_or(1, Made::sectors);
        for s in piece.sector..(piece.sector + count).min(owners.len()) {
            owners[s] = if owners[s] == Owner::Free { Owner::Piece(i) } else { Owner::Clash };
        }
    }
    owners
}

/// What's wrong with the plan's places, piece by piece: a piece on the boot block, past the end of the disk, or
/// over another.
pub fn place_problems(plan: &Plan, made: &[Option<Made>]) -> Vec<String> {
    let mut problems = Vec::new();
    let total = plan.density.sectors();
    for (i, piece) in plan.pieces.iter().enumerate() {
        let count = made.get(i).and_then(Option::as_ref).map_or(1, Made::sectors);
        if piece.sector < 2 {
            problems.push(crate::i18n::tf!("{} starts at sector {}, in the boot block (sectors 0 and 1).", piece.name, piece.sector));
        } else if piece.sector + count > total {
            problems.push(crate::i18n::tf!("{} needs sectors {} to {}, past the disk's last sector, {}.", piece.name, piece.sector, piece.sector + count - 1, total - 1));
        }
        for (j, other) in plan.pieces.iter().enumerate().skip(i + 1) {
            let other_count = made.get(j).and_then(Option::as_ref).map_or(1, Made::sectors);
            if piece.sector < other.sector + other_count && other.sector < piece.sector + count {
                problems.push(crate::i18n::tf!("{} and {} both want sectors from {}.", piece.name, other.name, piece.sector.max(other.sector)));
            }
        }
    }
    let starts = plan.pieces.iter().filter(|p| p.start).count();
    if starts == 0 && !plan.pieces.is_empty() {
        problems.push(crate::i18n::tr("No piece is marked to start, so the boot block wouldn't know where to jump.").into());
    } else if starts > 1 {
        problems.push(crate::i18n::tr("More than one piece is marked to start: mark only one.").into());
    }
    if let Some(p) = plan.pieces.iter().find(|p| p.start && !p.loaded) {
        problems.push(crate::i18n::tf!("{} is marked to start but isn't loaded by the boot block.", p.name));
    }
    problems
}

/// The table the boot block reads: where to jump, then each loaded piece's place on the disk (in bytes), its
/// length (in whole sectors' bytes) and its address, ending with 0.
pub fn table_source(plan: &Plan, made: &[Option<Made>]) -> String {
    let start = plan.pieces.iter().zip(made).find(|(p, _)| p.start).and_then(|(_, m)| m.as_ref()).map_or(0, |m| m.entry);
    let mut out = format!("{TABLE_FROM}\nstart:  dc.l    ${start:X}\npieces:\n");
    for (piece, made) in plan.pieces.iter().zip(made) {
        let Some(made) = made.as_ref().filter(|_| piece.loaded) else { continue };
        out.push_str(&format!("        dc.l    {},{},${:X}\t\t; {}\n", piece.sector * SECTOR, made.sectors() * SECTOR, made.load, piece.name.replace(';', ",")));
    }
    out.push_str(&format!("        dc.l    0\n{TABLE_TO}\n"));
    out
}

/// Limoniter's example boot block: it reads each piece in the table to its address and jumps to the start.
pub fn example_boot_source() -> String {
    format!(
        "; The boot block of a disk with no file system, made by Limoniter. Kickstart reads these 1024 bytes when\n\
         ; the disk boots and runs the code after the header, with A1 pointing at trackdisk.device's IORequest and A6\n\
         ; at ExecBase. This reads each piece from its sectors to its address, turns the drive's motor off, and jumps\n\
         ; to the piece that starts, with A6 still ExecBase.\n\
         ; Everything here, the header and the table included, has to fit in 1024 bytes: Limoniter says if it doesn't.\n\
         ; Limoniter writes the table between the two marked lines each time the disk is written, from the disk's map.\n\
         \n\
         io_Command\tequ\t28\n\
         io_Error\tequ\t31\n\
         io_Length\tequ\t36\n\
         io_Data\t\tequ\t40\n\
         io_Offset\tequ\t44\n\
         CMD_READ\tequ\t2\n\
         TD_MOTOR\tequ\t9\n\
         _LVODoIO\tequ\t-456\n\
         \n\
         \t\torg\t0\n\
         \t\tdc.b\t'DOS',0\t\t\t; the disk boots\n\
         \t\tdc.l\t0\t\t\t; the checksum, worked out when the disk is written\n\
         \t\tdc.l\t880\t\t\t; the root block: not used, as there's no file system\n\
         \n\
         boot:\t\tmove.l\ta1,a5\t\t\t; trackdisk's IORequest, for each read\n\
         \t\tlea\tpieces(pc),a4\n\
         .read:\t\tmove.l\t(a4)+,d0\t\t; where the piece is on the disk, in bytes: 0 ends the table\n\
         \t\tbeq.s\t.loaded\n\
         \t\tmove.l\ta5,a1\n\
         \t\tmove.w\t#CMD_READ,io_Command(a1)\n\
         \t\tmove.l\td0,io_Offset(a1)\n\
         \t\tmove.l\t(a4)+,io_Length(a1)\t; whole sectors\n\
         \t\tmove.l\t(a4)+,io_Data(a1)\t; where it goes\n\
         \t\tjsr\t_LVODoIO(a6)\n\
         \t\ttst.b\tio_Error(a5)\n\
         \t\tbeq.s\t.read\n\
         .failed:\tmove.w\t#$0f00,$dff180\t\t; a read failed: the screen goes red\n\
         \t\tbra.s\t.failed\n\
         .loaded:\tmove.l\ta5,a1\n\
         \t\tmove.w\t#TD_MOTOR,io_Command(a1)\n\
         \t\tclr.l\tio_Length(a1)\t\t; the motor off\n\
         \t\tjsr\t_LVODoIO(a6)\n\
         \t\tmove.l\tstart(pc),a0\n\
         \t\tjmp\t(a0)\n\
         \n\
         \t\tcnop\t0,4\n\
         {TABLE_FROM}\n\
         start:\t\tdc.l\t0\n\
         pieces:\t\tdc.l\t0\n\
         {TABLE_TO}\n"
    )
}

/// A boot block's source with the plan's table put between its marked lines (or, without them, added at the end).
pub fn with_table(source: &str, table: &str) -> String {
    match (source.find(TABLE_FROM), source.find(TABLE_TO)) {
        (Some(from), Some(to)) if to > from => {
            let end = source[to..].find('\n').map_or(source.len(), |n| to + n + 1);
            format!("{}{table}{}", &source[..from], &source[end..])
        }
        _ => format!("{}\n\t\tcnop\t0,4\n{table}", source.trim_end()),
    }
}

/// The boot block's checksum, as Kickstart checks it: every long word added with its carry put back in, the
/// result inverted, so that the whole block adds up to $FFFFFFFF.
pub fn checksum(block: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    for (i, long) in block[..BOOT_BYTES].chunks(4).enumerate() {
        let value = if i == 1 { 0 } else { u32::from_be_bytes([long[0], long[1], long[2], long[3]]) };
        let (added, carry) = sum.overflowing_add(value);
        sum = added + u32::from(carry);
    }
    !sum
}

/// The boot block, assembled from its source with the plan's table, and checked to fit.
pub fn make_boot(source: &str, plan: &Plan, made: &[Option<Made>]) -> Result<Vec<u8>, String> {
    let source = with_table(source, &table_source(plan, made));
    let isa = crate::asm::m68k::M68k;
    let assembly = crate::asm::assemble(&isa, Path::new("bootblock.s"), &source, &|p| Err(crate::i18n::tf!("The boot block can't include {}.", p.display())));
    if let Some(problem) = assembly.problems.first() {
        return Err(crate::i18n::tf!("The boot block, line {}: {}", problem.line + 1, problem.message));
    }
    let high = assembly.highest().unwrap_or(0).max(0) as usize;
    if assembly.lowest().unwrap_or(0) != 0 {
        return Err(crate::i18n::tr("The boot block's source has to start at ORG 0, with \"DOS\",0 first.").into());
    }
    if high > BOOT_BYTES {
        return Err(crate::i18n::tf!("The boot block is {high} bytes, {} more than the 1024 it has. Make its code or its table smaller.", high - BOOT_BYTES));
    }
    let mut block = assembly.image(0, BOOT_BYTES as i64);
    if &block[..3] != b"DOS" {
        return Err(crate::i18n::tr("The boot block has to start with \"DOS\" (then a byte, 0 for the old file system), or Kickstart won't boot it.").into());
    }
    let sum = checksum(&block);
    block[4..8].copy_from_slice(&sum.to_be_bytes());
    Ok(block)
}

/// How many bytes the boot block uses of its 1024, from its assembled size.
pub fn boot_size(source: &str, plan: &Plan, made: &[Option<Made>]) -> Option<usize> {
    let source = with_table(source, &table_source(plan, made));
    let assembly = crate::asm::assemble(&crate::asm::m68k::M68k, Path::new("bootblock.s"), &source, &|_| Err(String::new()));
    assembly.ok().then(|| assembly.highest().unwrap_or(0).max(0) as usize)
}

/// A disk made: the image, and what was said along the way.
pub struct Built {
    pub image: Vec<u8>,
    pub boot_size: usize,
    pub made: Vec<Option<Made>>,
}

/// Makes the whole disk: every piece, the boot block with their table, and the image with each at its sectors.
/// `base` is an image to write over (sectors it already uses are kept, unless a piece is put there).
pub fn build(plan: &Plan, boot_source: &str, base: Option<&[u8]>, read: &dyn Fn(&Path) -> Result<Vec<u8>, String>) -> Result<Built, String> {
    let made: Vec<Option<Made>> = plan.pieces.iter().map(|p| make_piece(p, read).map(Some)).collect::<Result<_, _>>()?;
    if let Some(problem) = place_problems(plan, &made).into_iter().next() {
        return Err(problem);
    }
    let boot = make_boot(boot_source, plan, &made)?;
    let size = plan.density.sectors() * SECTOR;
    let mut image = match base {
        Some(base) if base.len() == size => base.to_vec(),
        Some(base) => return Err(crate::i18n::tf!("The disk to write over is {} bytes, but a {} disk is {size}.", base.len(), crate::i18n::tr(plan.density.name()))),
        None => vec![0; size],
    };
    image[..BOOT_BYTES].copy_from_slice(&boot);
    for (piece, made) in plan.pieces.iter().zip(&made) {
        let Some(made) = made else { continue };
        let at = piece.sector * SECTOR;
        image[at..at + made.bytes.len()].copy_from_slice(&made.bytes);
        // The rest of its last sector, cleared.
        image[at + made.bytes.len()..at + made.sectors() * SECTOR].fill(0);
    }
    let boot_size = boot_size(boot_source, plan, &made).unwrap_or(BOOT_BYTES);
    Ok(Built { image, boot_size, made })
}

/// The sectors of an image that hold anything: where a disk to write over already has something.
pub fn used_in(image: &[u8]) -> Vec<bool> {
    image.chunks(SECTOR).map(|s| s.iter().any(|&b| b != 0)).collect()
}

#[cfg(test)]
mod tests;
