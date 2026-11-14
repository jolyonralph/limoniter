//! Flux images of floppy disks: what a Greaseweazle, SuperCard Pro or KryoFlux board saw the disk's magnetic field
//! do, rather than the sectors a disk controller would have read. They keep everything, copy protection and
//! unreadable sectors too, which makes them the archivist's format; to see what's on the disk, the flux has to be
//! turned back into bit cells (pll.rs), the cells into sectors in whatever encoding the disk was written in
//! (mfm.rs for IBM FM and MFM and the Amiga's MFM, gcr.rs for Commodore's and Apple's GCR), and the sectors into a
//! plain image of the disk, which then opens as the file system it holds, as an ImageDisk or Teledisk image does.
//!
//! Most images hold several revolutions of each track, so a sector that doesn't read cleanly on one may on another:
//! the first good copy of each is kept, and those that never read cleanly are listed and left as zeros. The plain
//! image is offered as a file too, so it can be saved for an emulator or another tool.
//!
//! The containers: SuperCard Pro (`.scp`, also what the Greaseweazle writes), HxC (`.hfe`, bit cells rather than
//! flux) and KryoFlux stream files (`trackNN.S.raw`, a file a track, so a whole folder or ZIP of them is one disk).

mod gcr;
mod hfe;
mod kryoflux;
mod mfm;
mod pll;
mod scp;

use super::{Entry, InfoSection, Source, Volume};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A track's flux: the times between changes, in nanoseconds, every revolution read one after another.
pub struct Flux {
    pub gaps: Vec<f32>,
    pub revolutions: usize,
}

/// A sector as decoded from a track.
#[derive(Clone, Debug)]
pub struct Found {
    /// The track its own header gives (an IBM ID field's cylinder, an Amiga or 1541 track number).
    pub cylinder: u8,
    pub head: u8,
    pub id: u8,
    pub size: usize,
    /// None when its data didn't pass its check.
    pub data: Option<Vec<u8>>,
}

/// The flux image formats.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Scp,
    Hfe,
    KryoFlux,
}

/// A flux image's format, from its signature: "SCP", HxC's "HXCPICFE" or "HXCHFEV3", or a KryoFlux stream's
/// out-of-band block with a `.raw` ending.
pub fn detect(head: &[u8], ending: &str) -> Option<Kind> {
    if scp::looks_like(head) {
        Some(Kind::Scp)
    } else if hfe::looks_like(head) {
        Some(Kind::Hfe)
    } else if ending == "raw" && kryoflux::looks_like(head) {
        Some(Kind::KryoFlux)
    } else {
        None
    }
}

enum Container {
    Scp(scp::Scp),
    Hfe(hfe::Hfe),
    KryoFlux(kryoflux::Files),
}

impl Container {
    fn places(&self) -> Vec<(u8, u8)> {
        match self {
            Container::Scp(scp) => scp.places(),
            Container::Hfe(hfe) => hfe.places(),
            Container::KryoFlux(files) => files.places(),
        }
    }

    /// A track's flux, and anything its reader reported going wrong.
    fn flux(&self, place: (u8, u8)) -> Result<(Flux, Option<String>), String> {
        match self {
            Container::Scp(scp) => scp.flux(place).map(|f| (f, None)),
            Container::Hfe(hfe) => hfe.flux(place).map(|f| (f, None)),
            Container::KryoFlux(files) => kryoflux::decode(&files.read(place)?),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Container::Scp(_) => "SuperCard Pro (SCP) flux",
            Container::Hfe(_) => "HxC (HFE) bit cells",
            Container::KryoFlux(kryoflux::Files::Single(..)) => "KryoFlux stream (one track)",
            Container::KryoFlux(_) => "KryoFlux stream files",
        }
    }

    fn about(&self) -> Vec<(String, String)> {
        let mut rows = vec![("Format".to_string(), self.name().to_string())];
        match self {
            Container::Scp(scp) => {
                rows.push(("Version".into(), format!("{}.{}", scp.version >> 4, scp.version & 15)));
                rows.push(("Disk type".into(), format!("&{:02X}", scp.disk_type)));
                rows.push(("Revolutions a track".into(), scp.revolutions().to_string()));
                rows.push(("Timing resolution".into(), format!("{} ns", scp.tick())));
            }
            Container::Hfe(hfe) => {
                rows.push(("Version".into(), hfe.version().to_string()));
                rows.push(("Bit rate".into(), format!("{} kbit/s", hfe.bit_rate)));
                rows.push(("Encoding it gives".into(), format!("&{:02X}", hfe.encoding)));
                rows.push(("Interface mode".into(), format!("&{:02X}", hfe.interface)));
            }
            Container::KryoFlux(_) => {}
        }
        rows.push(("Tracks in the image".into(), self.places().len().to_string()));
        rows
    }
}

/// The encodings a disk can be decoded as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    Ibm,
    Amiga,
    Commodore,
    Apple,
}

/// What came of one track.
#[derive(Default)]
struct TrackRead {
    found: Vec<Found>,
    /// Nanoseconds a cell, as the loop started.
    cell: Option<f64>,
    fm: bool,
    revolutions: usize,
    trouble: Option<String>,
}

/// Decodes a track's flux in one family of encodings, the loop starting from the track's own shortest gap, times
/// `nudge` (1 for the first try).
fn decode(flux: &Flux, family: Family, pull: pll::Loop, nudge: f64) -> TrackRead {
    let mut read = TrackRead { revolutions: flux.revolutions, ..TrackRead::default() };
    let Some(peak) = pll::shortest_peak(&flux.gaps).map(|p| p * nudge) else {
        return read;
    };
    let good = |found: &[Found]| found.iter().filter(|f| f.data.is_some()).count();
    match family {
        Family::Ibm => {
            // MFM's shortest gap is two cells; FM's one. A track with no good MFM sectors may be FM, as the TRS-80's
            // first track is on an otherwise double density disk.
            let mfm = mfm::ibm_mfm(&pll::cells(&flux.gaps, peak / 2.0, pull));
            if good(&mfm) > 0 {
                (read.found, read.cell) = (mfm, Some(peak / 2.0));
                return read;
            }
            let fm = mfm::ibm_fm(&pll::cells(&flux.gaps, peak, pull));
            if good(&fm) > 0 || fm.len() > mfm.len() {
                (read.found, read.cell, read.fm) = (fm, Some(peak), true);
            } else {
                (read.found, read.cell) = (mfm, Some(peak / 2.0));
            }
        }
        Family::Amiga => (read.found, read.cell) = (mfm::amiga(&pll::cells(&flux.gaps, peak / 2.0, pull)), Some(peak / 2.0)),
        Family::Commodore => {
            (read.found, read.cell) = (gcr::commodore(&pll::cells(&flux.gaps, peak, pull)), Some(peak));
            // Noisy 1541 tracks read better with spurious short gaps merged away, as Greaseweazle also does.
            if good(&read.found) == 0 {
                let filtered = pll::low_pass(&flux.gaps, (peak * 0.77) as f32);
                let again = gcr::commodore(&pll::cells(&filtered, peak, pull));
                if good(&again) > 0 {
                    read.found = again;
                }
            }
        }
        Family::Apple => (read.found, read.cell) = (gcr::apple(&pll::cells(&flux.gaps, peak, pull)), Some(peak)),
    }
    read
}

/// Runs `work` on each item on all the computer's cores, keeping the results in order.
fn in_parallel<T: Sync, R: Send + Default>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 16).min(items.len().max(1));
    let mut results: Vec<R> = (0..items.len()).map(|_| R::default()).collect();
    let parts: Vec<Vec<(usize, R)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        done.push((i, work(item)));
                    }
                    done
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    for (i, result) in parts.into_iter().flatten() {
        results[i] = result;
    }
    results
}

/// A list of places, shortened past eight.
fn list(places: &[String]) -> String {
    if places.len() > 8 { format!("{} and {} more", places[..8].join("; "), places.len() - 8) } else { places.join("; ") }
}

/// A disk decoded from its flux: its file system if Limoniter knows it, and the plain image of its sectors.
pub struct FluxDisk {
    what: String,
    inner: Option<Box<dyn Volume>>,
    plain: Arc<Vec<u8>>,
    plain_name: String,
    notes: Vec<String>,
    info: Vec<InfoSection>,
}

/// An entry's `extra` for the plain image of the whole disk.
const WHOLE_DISK: u64 = u64::MAX;

/// Opens a flux image as the disk it holds.
pub fn open(source: Arc<dyn Source>, kind: Kind, name: &str) -> Result<Box<dyn Volume>, String> {
    let container = match kind {
        Kind::Scp => Container::Scp(scp::Scp::open(source)?),
        Kind::Hfe => Container::Hfe(hfe::Hfe::open(source)?),
        Kind::KryoFlux => {
            let place = kryoflux::place_of(name).map_or((0, 0), |(_, place)| place);
            Container::KryoFlux(kryoflux::Files::Single(place, source))
        }
    };
    Ok(Box::new(read_disk(container, Vec::new())))
}

/// Opens a KryoFlux stream file on disk as the whole set it belongs to, or None when it isn't one.
pub fn open_stream_folder(path: &Path) -> Option<Result<Box<dyn Volume>, String>> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    kryoflux::place_of(&name)?;
    let mut head = [0u8; 4];
    std::io::Read::read_exact(&mut std::fs::File::open(path).ok()?, &mut head).ok()?;
    if !kryoflux::looks_like(&head) {
        return None;
    }
    let files = kryoflux::folder_set(path)?;
    Some(Ok(Box::new(read_disk(Container::KryoFlux(files), Vec::new())) as Box<dyn Volume>))
}

/// Opens a ZIP archive holding one disk's KryoFlux stream files as that disk, or None when it doesn't hold one.
pub fn open_zip_set(source: Arc<dyn Source>) -> Option<Result<Box<dyn Volume>, String>> {
    let files = kryoflux::zip_set(source)?;
    let note = "The KryoFlux stream files were read from a ZIP archive.".to_string();
    Some(Ok(Box::new(read_disk(Container::KryoFlux(files), vec![note])) as Box<dyn Volume>))
}

/// Reads every track, works out the encoding, lays out the sectors and opens the disk inside.
fn read_disk(container: Container, mut notes: Vec<String>) -> FluxDisk {
    let places = container.places();
    let mut info = vec![InfoSection { title: "Flux image".into(), rows: container.about() }];
    // The encoding is chosen on a few tracks spread over the disk, by which gives the most good sector bytes.
    let mut probes: Vec<(u8, u8)> = [0, 1, places.len() / 4, places.len() / 2, places.len() * 3 / 4, places.len().saturating_sub(4)].iter().filter_map(|i| places.get(*i).copied()).collect();
    probes.dedup();
    let probe_flux = in_parallel(&probes, |place| container.flux(*place).ok().map(|(flux, _)| flux));
    let families = [Family::Ibm, Family::Amiga, Family::Commodore, Family::Apple];
    let scores: Vec<usize> = families
        .iter()
        .map(|family| {
            let reads = in_parallel(&probe_flux, |flux| flux.as_ref().map(|f| decode(f, *family, pll::QUICK, 1.0).found.iter().filter(|s| s.data.is_some()).map(|s| s.size).sum::<usize>()).unwrap_or(0));
            reads.iter().sum()
        })
        .collect();
    drop(probe_flux);
    let best = (0..families.len()).max_by_key(|&i| (scores[i], std::cmp::Reverse(i))).filter(|&i| scores[i] > 0).map(|i| families[i]);
    let Some(family) = best else {
        notes.push("No sectors in an encoding Limoniter knows (IBM FM or MFM, Amiga MFM, Commodore 1541 GCR, Apple II 6-and-2 GCR) could be decoded from the flux.".into());
        return FluxDisk { what: container.name().to_string(), inner: None, plain: Arc::new(Vec::new()), plain_name: String::new(), notes, info };
    };
    // Every track, with the quick loop; then any track with a bad sector, or fewer good ones than most tracks have,
    // again with the steady loop, keeping whatever it adds.
    let mut reads: Vec<TrackRead> = in_parallel(&places, |place| match container.flux(*place) {
        Ok((flux, trouble)) => TrackRead { trouble, ..decode(&flux, family, pll::QUICK, 1.0) },
        Err(problem) => TrackRead { trouble: Some(problem), ..TrackRead::default() },
    });
    // Sectors are counted once each, however many revolutions read them.
    let good = |read: &TrackRead| read.found.iter().filter(|f| f.data.is_some()).map(|f| (f.cylinder, f.head, f.id)).collect::<std::collections::BTreeSet<_>>().len();
    let unread = |read: &TrackRead| read.found.iter().any(|f| f.data.is_none() && !read.found.iter().any(|g| g.data.is_some() && (g.cylinder, g.head, g.id) == (f.cylinder, f.head, f.id)));
    let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
    for read in &reads {
        *counts.entry(good(read)).or_default() += 1;
    }
    let usual = counts.iter().filter(|(n, _)| **n > 0).max_by_key(|(_, times)| **times).map_or(0, |(n, _)| *n);
    // Tracks short of sectors are tried again: with the steady loop, then with the clock started a little slow
    // and a little fast, in case the shortest gap misled it.
    for (pull, nudge) in [(pll::STEADY, 1.0), (pll::QUICK, 0.96), (pll::QUICK, 1.04), (pll::STEADY, 0.96), (pll::STEADY, 1.04)] {
        let again: Vec<usize> = (0..reads.len()).filter(|&i| reads[i].cell.is_some() && (good(&reads[i]) < usual || unread(&reads[i]))).collect();
        let more = in_parallel(&again, |&i| container.flux(places[i]).map(|(flux, _)| decode(&flux, family, pull, nudge)).unwrap_or_default());
        for (&i, extra) in again.iter().zip(more) {
            reads[i].found.extend(extra.found);
        }
    }
    for (place, read) in places.iter().zip(&reads) {
        if let Some(trouble) = &read.trouble {
            notes.push(format!("Track {} side {}: {trouble}.", place.0, place.1));
        }
    }
    let mut cells: Vec<f64> = reads.iter().filter(|r| good(r) > 0).filter_map(|r| r.cell).collect();
    cells.sort_by(f64::total_cmp);
    let revolutions = reads.iter().map(|r| r.revolutions).max().unwrap_or(0);
    let fm_tracks = reads.iter().filter(|r| r.fm && good(r) > 0).count();
    let mfm_tracks = reads.iter().filter(|r| !r.fm && good(r) > 0).count();
    let encoding = match family {
        Family::Ibm if fm_tracks > 0 && mfm_tracks > 0 => "IBM MFM, with FM tracks",
        Family::Ibm if fm_tracks > 0 => "IBM FM",
        Family::Ibm => "IBM MFM",
        Family::Amiga => "Amiga MFM",
        Family::Commodore => "Commodore 1541 GCR",
        Family::Apple => "Apple II 6-and-2 GCR",
    };
    let found: Vec<((u8, u8), Found)> = places.iter().zip(reads).flat_map(|(place, read)| read.found.into_iter().map(move |f| (*place, f))).collect();
    let mut laid = match family {
        Family::Ibm => lay_out_ibm(&found),
        Family::Amiga => lay_out_amiga(&found),
        Family::Commodore => lay_out_commodore(&found),
        Family::Apple => lay_out_apple(&found),
    };
    let mut rows = vec![("Encoding".to_string(), encoding.to_string())];
    if let Some(cell) = cells.get(cells.len() / 2) {
        rows.push(("Bit cell".into(), format!("{:.2} \u{B5}s", cell / 1000.0)));
    }
    if revolutions > 0 {
        rows.push(("Revolutions read".into(), revolutions.to_string()));
    }
    // The disk inside, from the whole layout; failing that, for an IBM disk whose sides were written differently
    // (a single-sided BBC disk whose other side still holds an old PC format), from either side alone.
    let mut opened = laid.as_ref().ok().map(|laid| laid.open(Arc::new(laid.bytes.clone())));
    if family == Family::Ibm && matches!(opened, Some(Err(_))) {
        for head in 0..2 {
            let side: Vec<((u8, u8), Found)> = found.iter().filter(|((_, h), _)| *h == head).cloned().collect();
            if side.len() == found.len() {
                continue;
            }
            if let Ok(alone) = lay_out_ibm(&side)
                && let Ok(volume) = alone.open(Arc::new(alone.bytes.clone()))
            {
                notes.push(format!("Only side {head} holds a file system Limoniter knows, so the disk is laid out from that side alone."));
                (laid, opened) = (Ok(alone), Some(Ok(volume)));
                break;
            }
        }
    }
    match laid {
        Ok(laid) => {
            rows.push(("Layout".into(), laid.shape.clone()));
            rows.push(("Sectors read cleanly".into(), format!("{} of {}", laid.good, laid.expected)));
            if !laid.bad.is_empty() {
                rows.push(("Never read cleanly".into(), list(&laid.bad)));
            }
            info.push(InfoSection { title: "Decoded".into(), rows });
            notes.extend(laid.notes.iter().cloned());
            let inner = match opened.unwrap_or_else(|| Err(String::new())) {
                Ok(volume) => Some(volume),
                Err(problem) => {
                    notes.push(format!("{problem} The decoded sectors are offered as a plain image to look at in hex or save."));
                    None
                }
            };
            FluxDisk { what: format!("{} \u{B7} {encoding} \u{B7} {}", container.name(), laid.shape), inner, plain: Arc::new(laid.bytes), plain_name: format!("Decoded disk.{}", laid.ending), notes, info }
        }
        Err(problem) => {
            info.push(InfoSection { title: "Decoded".into(), rows });
            notes.push(problem);
            FluxDisk { what: format!("{} \u{B7} {encoding}", container.name()), inner: None, plain: Arc::new(Vec::new()), plain_name: String::new(), notes, info }
        }
    }
}

/// A decoded disk laid out as a plain image.
struct Laid {
    bytes: Vec<u8>,
    shape: String,
    notes: Vec<String>,
    good: usize,
    expected: usize,
    /// Where the sectors that never read cleanly are, for File properties.
    bad: Vec<String>,
    ending: &'static str,
    /// The tracks' shape, for the IBM disks whose file system needs telling it.
    tracks: Option<super::cpm::Shape>,
}

impl Laid {
    fn open(&self, disk: Arc<dyn Source>) -> Result<Box<dyn Volume>, String> {
        match (self.ending, self.tracks) {
            ("img", Some(tracks)) => super::sectors::open_inner(disk, tracks),
            ("adf", _) => Ok(Box::new(super::amiga::open_adf(disk)?)),
            ("d64", _) => Ok(Box::new(super::cbm::Disk::open(disk, super::cbm::Kind::D64)?)),
            (ending, _) => super::open(disk, &format!("disk.{ending}")),
        }
    }
}

/// The note for sectors that never read cleanly, listed by place.
fn missing_note(missing: &[String]) -> Option<String> {
    (!missing.is_empty()).then(|| format!("Sectors that never decoded cleanly from the flux are zeros here: {}.", list(missing)))
}

/// IBM sectors, by the track their ID fields give when the disk's IDs agree with where they were read (or with half
/// of it, for a 40-track disk read in an 80-track drive), else by where they were read; then laid out as an ImageDisk
/// image's are.
fn lay_out_ibm(found: &[((u8, u8), Found)]) -> Result<Laid, String> {
    let good: Vec<&((u8, u8), Found)> = found.iter().filter(|(_, f)| f.data.is_some()).collect();
    let agreeing = good.iter().filter(|((cylinder, _), f)| f.cylinder == *cylinder || f.cylinder == cylinder / 2).count();
    let by_id = agreeing * 10 >= good.len() * 8;
    let mut tracks: BTreeMap<(u8, u8), BTreeMap<(u8, usize), Option<Vec<u8>>>> = BTreeMap::new();
    for ((cylinder, head), sector) in found {
        let key = (if by_id { sector.cylinder } else { *cylinder }, *head);
        let slot = tracks.entry(key).or_default().entry((sector.id, sector.size)).or_default();
        if slot.is_none() {
            slot.clone_from(&sector.data);
        }
    }
    // A sector number and size whose data never read anywhere on the disk is taken for a copy protection's
    // decoy, not part of the disk, so it doesn't change the layout.
    let readable: std::collections::BTreeSet<(u8, usize)> = good.iter().map(|(_, f)| (f.id, f.size)).collect();
    let mut decoys = 0;
    for sectors in tracks.values_mut() {
        let before = sectors.len();
        sectors.retain(|key, data| data.is_some() || readable.contains(key));
        decoys += before - sectors.len();
    }
    tracks.retain(|_, sectors| !sectors.is_empty());
    let bad: Vec<String> = tracks.iter().flat_map(|((c, h), sectors)| sectors.iter().filter(|(_, d)| d.is_none()).map(move |((id, _), _)| format!("track {c} side {h} sector {id}"))).collect();
    let sectors: super::sectors::Tracks = tracks
        .into_iter()
        .map(|(key, sectors)| (key, sectors.into_iter().map(|((id, size), data)| super::sectors::Sector { id, size, data, error: false }).collect()))
        .collect();
    let laid = super::sectors::lay_out(&sectors)?;
    let expected = laid.bytes.len() / laid.tracks.sector_size.max(1) as usize;
    let mut notes: Vec<String> = laid.notes.iter().map(|n| n.replace("Sectors that couldn't be read when the image was made", "Sectors that never decoded cleanly from the flux")).collect();
    if decoys > 0 {
        notes.push(format!("{decoys} sector ID(s) whose data never read on any track, as copy protection writes, aren't part of the disk here."));
    }
    let good = expected.saturating_sub(count_missing(&sectors, &laid.tracks));
    Ok(Laid { shape: laid.shape, notes, good, expected, bad, ending: "img", tracks: Some(laid.tracks), bytes: laid.bytes })
}

/// How many of the laid-out sectors had no good copy: the slots of the usual geometry with no data. The first
/// sector number is found as the layout finds it, from the run of numbers most tracks have.
fn count_missing(tracks: &super::sectors::Tracks, shape: &super::cpm::Shape) -> usize {
    let size = shape.sector_size as usize;
    let mut runs: BTreeMap<(u8, usize), usize> = BTreeMap::new();
    for sectors in tracks.values() {
        let mut ids: Vec<u8> = sectors.iter().filter(|s| s.size == size).map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        if let Some(&first) = ids.first() {
            let length = ids.iter().enumerate().take_while(|(i, id)| usize::from(**id) == usize::from(first) + i).count();
            *runs.entry((first, length)).or_default() += 1;
        }
    }
    let first = runs.iter().filter(|((_, length), _)| *length as u64 == shape.per_track).max_by_key(|(_, count)| **count).map_or(0, |((first, _), _)| *first);
    let heads: std::collections::BTreeSet<u8> = tracks.keys().map(|(_, h)| *h).collect();
    let cylinders = tracks.iter().filter(|(_, s)| !s.is_empty()).map(|((c, _), _)| *c).max().map_or(0, |c| usize::from(c) + 1);
    let mut missing = 0;
    for cylinder in 0..cylinders {
        for head in &heads {
            let sectors = tracks.get(&(cylinder as u8, *head)).map(Vec::as_slice).unwrap_or_default();
            for n in 0..shape.per_track as usize {
                let id = usize::from(first) + n;
                if !sectors.iter().any(|s| usize::from(s.id) == id && s.size == size && s.data.is_some()) {
                    missing += 1;
                }
            }
        }
    }
    missing
}

/// Amiga sectors into an ADF: 80 cylinders of two sides, 11 sectors a track (22 for a high density disk).
fn lay_out_amiga(found: &[((u8, u8), Found)]) -> Result<Laid, String> {
    let per_track = if found.iter().any(|(_, f)| f.data.is_some() && f.id >= 11) { 22 } else { 11 };
    let mut bytes = vec![0u8; 160 * per_track * 512];
    let mut have = vec![false; 160 * per_track];
    let mut seen_bad = std::collections::BTreeSet::new();
    let mut beyond = 0;
    for (_, sector) in found {
        let track = usize::from(sector.cylinder) * 2 + usize::from(sector.head);
        if usize::from(sector.id) >= per_track {
            continue;
        }
        if track >= 160 {
            beyond += usize::from(sector.data.is_some());
            continue;
        }
        let slot = track * per_track + usize::from(sector.id);
        match &sector.data {
            Some(data) if !have[slot] => {
                bytes[slot * 512..slot * 512 + 512].copy_from_slice(data);
                have[slot] = true;
            }
            None => {
                seen_bad.insert(slot);
            }
            _ => {}
        }
    }
    let place = |slot: usize| format!("track {} side {} sector {}", slot / per_track / 2, slot / per_track % 2, slot % per_track);
    let missing: Vec<String> = (0..have.len()).filter(|s| !have[*s]).map(place).collect();
    let bad = seen_bad.into_iter().filter(|s| !have[*s]).map(place).collect();
    let mut notes: Vec<String> = missing_note(&missing).into_iter().collect();
    if beyond > 0 {
        notes.push(format!("{beyond} sector(s) on tracks past the 80th aren't part of an ADF and are left out."));
    }
    let good = have.iter().filter(|h| **h).count();
    let shape = format!("80 tracks, 2 sides, {per_track} \u{D7} 512-byte sectors");
    Ok(Laid { bytes, shape, notes, good, expected: have.len(), bad, ending: "adf", tracks: None })
}

/// The sectors on a 1541 track.
fn sectors_1541(track: u8) -> usize {
    match track {
        1..=17 => 21,
        18..=24 => 19,
        25..=30 => 18,
        _ => 17,
    }
}

/// Commodore sectors into a D64: 35 tracks, or 40 when the extra five were used, by the zone's sector count.
fn lay_out_commodore(found: &[((u8, u8), Found)]) -> Result<Laid, String> {
    let tracks: u8 = if found.iter().any(|(_, f)| f.data.is_some() && (36..=40).contains(&f.cylinder)) { 40 } else { 35 };
    let start = |track: u8| (1..track).map(sectors_1541).sum::<usize>();
    let total = start(tracks + 1);
    let mut bytes = vec![0u8; total * 256];
    let mut have = vec![false; total];
    let mut seen_bad = std::collections::BTreeSet::new();
    for (_, sector) in found {
        if sector.cylinder == 0 || sector.cylinder > tracks || usize::from(sector.id) >= sectors_1541(sector.cylinder) {
            continue;
        }
        let slot = start(sector.cylinder) + usize::from(sector.id);
        match &sector.data {
            Some(data) if !have[slot] => {
                bytes[slot * 256..slot * 256 + 256].copy_from_slice(data);
                have[slot] = true;
            }
            None => {
                seen_bad.insert(slot);
            }
            _ => {}
        }
    }
    let place = |slot: usize| {
        let track = (1..=tracks).rfind(|t| start(*t) <= slot).unwrap_or(1);
        format!("track {track} sector {}", slot - start(track))
    };
    let missing: Vec<String> = (0..total).filter(|s| !have[*s]).map(place).collect();
    let bad = seen_bad.into_iter().filter(|s| !have[*s]).map(place).collect();
    let good = have.iter().filter(|h| **h).count();
    let shape = format!("{tracks} tracks, 17 to 21 \u{D7} 256-byte sectors");
    Ok(Laid { bytes, shape, notes: missing_note(&missing).into_iter().collect(), good, expected: total, bad, ending: "d64", tracks: None })
}

/// Apple II sectors into a DOS-ordered image of 35 tracks of 16: DOS 3.3 numbers its sectors in a different order
/// from the physical ones on the disk, which a `.do` image keeps.
fn lay_out_apple(found: &[((u8, u8), Found)]) -> Result<Laid, String> {
    const DOS_ORDER: [usize; 16] = [0, 7, 14, 6, 13, 5, 12, 4, 11, 3, 10, 2, 9, 1, 8, 15];
    let mut bytes = vec![0u8; 35 * 16 * 256];
    let mut have = vec![false; 35 * 16];
    let mut seen_bad = std::collections::BTreeSet::new();
    for (_, sector) in found {
        if sector.cylinder >= 35 || sector.id >= 16 {
            continue;
        }
        let slot = usize::from(sector.cylinder) * 16 + DOS_ORDER[usize::from(sector.id)];
        match &sector.data {
            Some(data) if !have[slot] => {
                bytes[slot * 256..slot * 256 + 256].copy_from_slice(data);
                have[slot] = true;
            }
            None => {
                seen_bad.insert(slot);
            }
            _ => {}
        }
    }
    let place = |slot: usize| format!("track {} sector {}", slot / 16, DOS_ORDER.iter().position(|l| *l == slot % 16).unwrap_or(0));
    let missing: Vec<String> = (0..have.len()).filter(|s| !have[*s]).map(place).collect();
    let bad = seen_bad.into_iter().filter(|s| !have[*s]).map(place).collect();
    let good = have.iter().filter(|h| **h).count();
    Ok(Laid { bytes, shape: "35 tracks, 16 \u{D7} 256-byte sectors".into(), notes: missing_note(&missing).into_iter().collect(), good, expected: have.len(), bad, ending: "do", tracks: None })
}

impl Volume for FluxDisk {
    fn describe(&self) -> String {
        match &self.inner {
            Some(inner) => format!("{} \u{B7} {}", self.what, inner.describe()),
            None => self.what.clone(),
        }
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if let Some(dir) = dir {
            return self.inner.as_ref().ok_or("This disk has no folders Limoniter can read.")?.list(Some(dir));
        }
        let mut entries = match &self.inner {
            Some(inner) => inner.list(None)?,
            None => Vec::new(),
        };
        if !self.plain.is_empty() {
            let mut whole = Entry::file(self.plain_name.clone(), self.plain.len() as u64, 0);
            whole.extra = WHOLE_DISK;
            whole.kind = "Disk image".into();
            whole.detail = "the sectors decoded from the flux, as a plain image".into();
            entries.push(whole);
        }
        Ok(entries)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        if entry.extra == WHOLE_DISK {
            return Ok(self.plain.as_ref().clone());
        }
        self.inner.as_ref().ok_or("That isn't on this disk.")?.read(entry)
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = self.notes.clone();
        if let Some(inner) = &self.inner {
            notes.extend(inner.notes());
        }
        notes
    }

    fn info(&self) -> Vec<InfoSection> {
        let mut info = self.info.clone();
        if let Some(inner) = &self.inner {
            info.extend(inner.info());
        }
        info
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        self.inner.as_ref().filter(|_| entry.extra != WHOLE_DISK).and_then(|inner| inner.text_preset(entry))
    }

    fn reading_bytes(&self, entry: &Entry, bytes: Vec<u8>) -> Vec<u8> {
        match &self.inner {
            Some(inner) if entry.extra != WHOLE_DISK => inner.reading_bytes(entry, bytes),
            _ => bytes,
        }
    }

    fn picture(&self) -> Option<Vec<u8>> {
        self.inner.as_ref().and_then(|inner| inner.picture())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{fixture, names};
    use super::mfm::tests::{Write, amiga_track, ibm_fm_track, ibm_mfm_track};
    use super::*;
    use pll::tests::gaps_of;

    /// The gaps for a track's cells, at `cell` nanoseconds, with drift and jitter as a real drive's.
    fn flux_of(cells: &[u8], cell: f64) -> Vec<f32> {
        gaps_of(cells, cell, 0.03, 0.1)
    }

    /// The order a PC's sectors are written in, with or without a 2:1 interleave.
    fn pc_order(interleave: bool) -> Vec<u8> {
        if interleave { (0..9).map(|i| (i * 2 % 9) as u8 + 1).collect() } else { (1..=9).collect() }
    }

    /// A PC disk's cylinders, two sides of nine 512-byte sectors, as IBM MFM cells in the order a PC formats them.
    fn pc_tracks(disk: &[u8], interleave: bool) -> Vec<((u8, u8), Vec<u8>)> {
        let mut out = Vec::new();
        for track in 0..disk.len() / (9 * 512) {
            let (cylinder, head) = ((track / 2) as u8, (track % 2) as u8);
            let sectors: Vec<Write> = pc_order(interleave).iter().map(|&id| Write { cylinder, head, id, code: 2, data: &disk[track * 4608 + (usize::from(id) - 1) * 512..][..512] }).collect();
            out.push(((cylinder, head), ibm_mfm_track(&sectors)));
        }
        out
    }

    /// Flips two data cells partway through a sector's data field on an interleaved PC track.
    fn spoil(cells: &mut [u8], sector: u8) {
        let position = pc_order(true).iter().position(|s| *s == sector).unwrap();
        let at = 80 * 16 + position * (12 + 3 + 7 + 22 + 12 + 3 + 1 + 512 + 2 + 40) * 16 + (12 + 3 + 7 + 22 + 12 + 3 + 1 + 100) * 16;
        cells[at + 1] ^= 1;
        cells[at + 3] ^= 1;
    }

    #[test]
    fn a_pc_disk_in_scp_flux_opens_as_its_fat_volume() {
        let disk = fixture("fat12.img");
        let mut tracks = pc_tracks(&disk, true);
        // Track 3 side 1's sector 4 is spoilt in the first revolution only: the second's copy is the one kept.
        // Track 7 side 0's sector 2 is spoilt in both: it's listed and left as zeros.
        let mut gaps: Vec<((u8, u8), Vec<f32>)> = Vec::new();
        for (place, cells) in tracks.iter_mut() {
            let mut first = cells.clone();
            if *place == (3, 1) {
                spoil(&mut first, 4);
            }
            if *place == (7, 0) {
                spoil(&mut first, 2);
                spoil(cells, 2);
            }
            let mut both = first;
            both.extend(cells.iter());
            gaps.push((*place, flux_of(&both, 2000.0)));
        }
        let image = scp::tests::scp_of(&gaps, 1);
        assert!(super::super::is_image("disk.scp", &image[..0x8800], image.len() as u64));
        let volume = super::super::open(Arc::new(image), "disk.scp").unwrap();
        assert_eq!(volume.describe(), "SuperCard Pro (SCP) flux \u{B7} IBM MFM \u{B7} 40 tracks, 2 sides, 9 \u{D7} 512-byte sectors numbered from 1 \u{B7} FAT12 \u{B7} TESTVOL");
        assert_eq!(volume.notes(), ["Sectors that never decoded cleanly from the flux are zeros here: track 7 side 0 sector 2."]);
        let files = volume.list(None).unwrap();
        let whole = files.iter().find(|f| f.name == "Decoded disk.img").unwrap();
        let mut expected = disk.clone();
        expected[(7 * 2) * 4608 + 512..][..512].fill(0);
        assert_eq!(volume.read(whole).unwrap(), expected);
        let decoded = &volume.info()[1];
        assert!(decoded.rows.contains(&("Sectors read cleanly".into(), "719 of 720".into())), "{decoded:?}");
        assert!(decoded.rows.contains(&("Never read cleanly".into(), "track 7 side 0 sector 2".into())), "{decoded:?}");
        // The FAT volume inside reads as the image itself does.
        let plain = super::super::open(Arc::new(fixture("fat12.img")), "fat12.img").unwrap();
        assert_eq!(names(&files[..files.len() - 1]), names(&plain.list(None).unwrap()));
    }

    #[test]
    fn an_amiga_disk_in_hfe_cells_opens_as_its_adf() {
        let adf = fixture("ofs.adf");
        let mut cylinders = Vec::new();
        for cylinder in 0..80 {
            let mut sides = Vec::new();
            for head in 0..2 {
                let track = cylinder * 2 + head;
                let sectors: Vec<&[u8]> = (0..11).map(|s| &adf[(track * 11 + s) * 512..][..512]).collect();
                sides.push(amiga_track(track as u8, &sectors));
            }
            cylinders.push(sides);
        }
        let image = hfe::tests::hfe_of(&cylinders, 250);
        let volume = super::super::open(Arc::new(image), "game.hfe").unwrap();
        assert!(volume.describe().starts_with("HxC (HFE) bit cells \u{B7} Amiga MFM \u{B7} 80 tracks, 2 sides, 11 \u{D7} 512-byte sectors \u{B7} "), "{}", volume.describe());
        assert!(volume.notes().is_empty(), "{:?}", volume.notes());
        let files = volume.list(None).unwrap();
        let whole = files.iter().find(|f| f.name == "Decoded disk.adf").unwrap();
        assert_eq!(volume.read(whole).unwrap(), adf);
        let plain = super::super::open(Arc::new(adf), "ofs.adf").unwrap();
        assert_eq!(names(&files[..files.len() - 1]), names(&plain.list(None).unwrap()));
    }

    #[test]
    fn a_commodore_disk_decodes_zone_by_zone_into_a_d64() {
        let d64 = fixture("test.d64");
        let mut gaps = Vec::new();
        let mut at = 0;
        for track in 1..=35u8 {
            let count = sectors_1541(track);
            let sectors: Vec<Vec<u8>> = (0..count).map(|s| d64[(at + s) * 256..][..256].to_vec()).collect();
            at += count;
            let cells = gcr::tests::commodore_track(track, &sectors);
            // The 1541's four speed zones: 3.25 microseconds a cell on the outside to 4 on the inside.
            let cell = match track {
                1..=17 => 3250.0,
                18..=24 => 3500.0,
                25..=30 => 3750.0,
                _ => 4000.0,
            };
            // A 1541 disk read in a 96 tpi drive: the tracks are on every other cylinder.
            gaps.push((((track - 1) * 2, 0), flux_of(&[cells.clone(), cells].concat(), cell)));
        }
        let volume = super::super::open(Arc::new(scp::tests::scp_of(&gaps, 1)), "c64.scp").unwrap();
        assert!(volume.describe().contains("Commodore 1541 GCR"), "{}", volume.describe());
        let files = volume.list(None).unwrap();
        assert_eq!(names(&files), ["HELLO", "NOTES", "Decoded disk.d64"]);
        assert_eq!(volume.read(&files[2]).unwrap(), d64);
    }

    #[test]
    fn an_apple_disk_at_hfe_double_rate_decodes_into_dos_order() {
        const DOS_ORDER: [usize; 16] = [0, 7, 14, 6, 13, 5, 12, 4, 11, 3, 10, 2, 9, 1, 8, 15];
        let disk = super::super::apple2::tests::dos33_disk();
        let mut cylinders = Vec::new();
        for track in 0..35u8 {
            let sectors: Vec<Vec<u8>> = (0..16).map(|physical| disk[(usize::from(track) * 16 + DOS_ORDER[physical]) * 256..][..256].to_vec()).collect();
            // HFE keeps Apple disks at twice their rate: each cell as itself and a 0.
            let cells: Vec<u8> = gcr::tests::apple_track(track, &sectors).into_iter().flat_map(|c| [c, 0]).collect();
            cylinders.push(vec![cells]);
        }
        let volume = super::super::open(Arc::new(hfe::tests::hfe_of(&cylinders, 250)), "apple.hfe").unwrap();
        assert!(volume.describe().contains("Apple II 6-and-2 GCR"), "{}", volume.describe());
        let files = volume.list(None).unwrap();
        let whole = files.iter().find(|f| f.name == "Decoded disk.do").unwrap();
        assert_eq!(volume.read(whole).unwrap(), disk);
        let plain = super::super::open(Arc::new(disk), "disk.do").unwrap();
        assert_eq!(names(&files[..files.len() - 1]), names(&plain.list(None).unwrap()));
    }

    /// An HP 150 disk (sixteen 256-byte sectors a track, one side) written in FM, as KryoFlux stream files in a
    /// folder: opening any one of them opens the disk, as does a ZIP of them.
    #[test]
    fn single_density_kryoflux_streams_open_as_a_set() {
        let disk = super::super::sectors::tests::hp150_disk();
        let folder = std::env::temp_dir().join(format!("limoniter-kryoflux-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut streams = Vec::new();
        for track in 0..disk.len() / 4096 {
            let sectors: Vec<Write> = (0..16).map(|id| Write { cylinder: track as u8, head: 0, id, code: 1, data: &disk[track * 4096 + usize::from(id) * 256..][..256] }).collect();
            let stream = kryoflux::tests::stream_of(&flux_of(&ibm_fm_track(&sectors), 4000.0), 2);
            let name = format!("track{track:02}.0.raw");
            std::fs::write(folder.join(&name), &stream).unwrap();
            streams.push((name, stream));
        }
        let path = folder.join("track05.0.raw");
        let volume = super::super::open_file(&path).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(volume.describe(), "KryoFlux stream files \u{B7} IBM FM \u{B7} 66 tracks, 1 side, 16 \u{D7} 256-byte sectors numbered from 0 \u{B7} FAT12 \u{B7} MAESTRO_APL");
        let files = volume.list(None).unwrap();
        assert_eq!(names(&files), ["HELLO.TXT", "NAMELESS", "Decoded disk.img"]);
        assert_eq!(volume.read(&files[2]).unwrap(), disk);
        // A ZIP of them, in a folder, opens as the disk too.
        let zip = stored_zip(&streams.iter().map(|(n, s)| (format!("Disk 1/{n}"), s.clone())).collect::<Vec<_>>());
        let zipped = super::super::open(Arc::new(zip), "disk1.zip").unwrap();
        assert!(zipped.describe().starts_with("KryoFlux stream files \u{B7} IBM FM"), "{}", zipped.describe());
        assert_eq!(zipped.read(&zipped.list(None).unwrap()[2]).unwrap(), disk);
        // A lone stream is its one track.
        let (name, stream) = &streams[3];
        assert!(super::super::is_image(name, &stream[..64], stream.len() as u64));
        let one = super::super::open(Arc::new(stream.clone()), name).unwrap();
        assert!(one.describe().starts_with("KryoFlux stream (one track) \u{B7} IBM FM"), "{}", one.describe());
        assert!(!super::super::is_image("notes.raw", b"plain bytes", 11));
    }

    /// A ZIP archive of files stored as they are.
    fn stored_zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
        let (mut out, mut directory) = (Vec::new(), Vec::new());
        for (name, bytes) in files {
            let at = out.len() as u32;
            let sizes = [(bytes.len() as u32).to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat();
            out.extend(b"PK\x03\x04\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
            out.extend(&sizes);
            out.extend((name.len() as u16).to_le_bytes());
            out.extend([0, 0]);
            out.extend(name.as_bytes());
            out.extend(bytes);
            directory.extend(b"PK\x01\x02\x14\0\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
            directory.extend(&sizes);
            directory.extend((name.len() as u16).to_le_bytes());
            directory.extend([0u8; 12]);
            directory.extend(at.to_le_bytes());
            directory.extend(name.as_bytes());
        }
        let start = out.len() as u32;
        out.extend(&directory);
        out.extend(b"PK\x05\x06\0\0\0\0");
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((directory.len() as u32).to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend([0, 0]);
        out
    }

    #[test]
    fn odd_and_damaged_flux_images_give_errors_or_notes_never_panics() {
        let disk = fixture("fat12.img");
        let tracks = pc_tracks(&disk[..4 * 4608], false);
        let gaps: Vec<((u8, u8), Vec<f32>)> = tracks.iter().map(|(p, c)| (*p, flux_of(c, 2000.0))).collect();
        let scp = scp::tests::scp_of(&gaps, 1);
        let hfe = hfe::tests::hfe_of(&[tracks.iter().map(|(_, c)| c.clone()).take(2).collect()], 250);
        let stream = kryoflux::tests::stream_of(&gaps[0].1, 1);
        for (name, image) in [("x.scp", scp), ("x.hfe", hfe), ("track00.0.raw", stream)] {
            for seed in 0..40u32 {
                let mut bytes = image.clone();
                let mut state = seed.wrapping_mul(2_654_435_761);
                for _ in 0..16 {
                    state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    // The headers and tables are where damage does the most.
                    let span = if seed % 2 == 0 { 2048.min(bytes.len()) } else { bytes.len() };
                    let at = state as usize % span;
                    bytes[at] = (state >> 16) as u8;
                }
                if let Ok(volume) = super::super::open(Arc::new(bytes.clone()), name) {
                    let _ = volume.describe();
                    let _ = volume.info();
                    for entry in volume.list(None).unwrap_or_default() {
                        let _ = volume.read(&entry);
                    }
                }
                bytes.truncate(state as usize % bytes.len());
                if let Ok(volume) = super::super::open(Arc::new(bytes), name) {
                    let _ = volume.list(None);
                }
            }
        }
        // Flux with no disk on it: a note, not a disk.
        let noise: Vec<f32> = (0..50_000).map(|n| 1000.0 + ((n * 7919) % 6000) as f32).collect();
        let volume = super::super::open(Arc::new(scp::tests::scp_of(&[((0, 0), noise)], 1)), "blank.scp").unwrap();
        assert!(volume.list(None).unwrap().is_empty());
        assert!(volume.notes()[0].starts_with("No sectors in an encoding Limoniter knows"));
    }
    /// Real flux images, from ../testmedia/flux or the folder LIMONITER_FLUX names: every .scp and .hfe file and
    /// every folder of KryoFlux streams is opened, its decoded image written to LIMONITER_FLUX_OUT (if given) to be
    /// compared with what Greaseweazle's `gw convert` makes of it, and a line about each printed. Skipped when the
    /// folder isn't there. The few in testmedia take a second; a big sweep is best run with --release.
    #[test]
    fn real_flux_images_decode_without_trouble() {
        let root = std::env::var("LIMONITER_FLUX").map_or_else(|_| std::path::PathBuf::from("../testmedia/flux"), std::path::PathBuf::from);
        if !root.is_dir() {
            return;
        }
        let out = std::env::var("LIMONITER_FLUX_OUT").ok().map(std::path::PathBuf::from);
        let mut images = Vec::new();
        let mut folders = vec![root.clone()];
        while let Some(folder) = folders.pop() {
            let mut streams = Vec::new();
            for item in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = item.path();
                let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
                if path.is_dir() {
                    folders.push(path);
                } else if name.ends_with(".scp") || name.ends_with(".hfe") {
                    images.push(path);
                } else if kryoflux::place_of(&name).is_some() {
                    streams.push(path);
                }
            }
            streams.sort();
            images.extend(streams.into_iter().next());
        }
        images.sort();
        for path in images {
            let shown = path.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
            let started = std::time::Instant::now();
            match super::super::open_file(&path) {
                Ok(volume) => {
                    let files = volume.list(None).unwrap_or_default();
                    let decoded = volume.info().iter().find(|s| s.title == "Decoded").map(|s| s.rows.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("; ")).unwrap_or_default();
                    println!("{shown}\t{:.1}s\t{}\t{decoded}", started.elapsed().as_secs_f64(), volume.describe());
                    if let (Some(out), Some(whole)) = (&out, files.iter().find(|f| f.extra == WHOLE_DISK)) {
                        let target = out.join(format!("{}.{}", shown.replace('/', "__"), whole.name.rsplit('.').next().unwrap_or("img")));
                        std::fs::write(target, volume.read(whole).unwrap()).unwrap();
                    }
                }
                Err(problem) => println!("{shown}\tREFUSED\t{problem}"),
            }
        }
    }
}
