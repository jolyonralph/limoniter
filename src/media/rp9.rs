//! Amiga Forever packages (.rp9), from Cloanto's RetroPlatform: a ZIP of a game or demo's disks (ADF floppies,
//! HDF hard drives) with `rp9-manifest.xml` saying what it is and the Amiga it was set up for, a preview picture
//! and a help text. It's browsed as the ZIP it is, so its disks open as disks; the manifest gives the heading, and
//! the set-up and the order the floppies go in are shown as notes. File properties shows all of the manifest, the
//! help text and the preview picture.

use super::zip::Zip;
use super::{Entry, InfoSection, Source, Volume};
use std::sync::Arc;

const MANIFEST: &str = "rp9-manifest.xml";
/// Larger than any real manifest, which is a page or two of XML.
const MOST_MANIFEST_BYTES: u64 = 1024 * 1024;
const PREVIEW: &str = "rp9-preview.png";
/// The help text and preview picture are small; anything this large isn't one.
const MOST_EXTRA_BYTES: u64 = 4 * 1024 * 1024;

pub struct Rp9 {
    zip: Zip,
    heading: String,
    notes: Vec<String>,
    info: Vec<InfoSection>,
    picture: Option<Vec<u8>>,
}

impl Rp9 {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let zip = Zip::open(source)?;
        let top = zip.list(None)?;
        let named = |name: &str, most: u64| top.iter().find(|e| !e.is_dir && e.name.eq_ignore_ascii_case(name) && e.size <= most);
        // The help text comes in a language each; English if it's there.
        let help = named("rp9-help-en.txt", MOST_EXTRA_BYTES)
            .or_else(|| top.iter().find(|e| !e.is_dir && e.size <= MOST_EXTRA_BYTES && e.name.to_lowercase().starts_with("rp9-help") && e.name.to_lowercase().ends_with(".txt")))
            .and_then(|e| zip.read(e).ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
            .filter(|text| !text.is_empty());
        let picture = named(PREVIEW, MOST_EXTRA_BYTES).and_then(|e| zip.read(e).ok());
        let manifest = named(MANIFEST, MOST_MANIFEST_BYTES).cloned();
        let mut info = Vec::new();
        let (heading, notes) = match manifest.map(|entry| zip.read(&entry)) {
            Some(Ok(bytes)) => match summary(&String::from_utf8_lossy(&bytes)) {
                Ok(summary) => {
                    info = sections(&String::from_utf8_lossy(&bytes)).unwrap_or_default();
                    summary
                }
                Err(problem) => ("Amiga Forever package (RP9)".to_string(), vec![format!("Its manifest can't be read: {problem}")]),
            },
            Some(Err(problem)) => ("Amiga Forever package (RP9)".to_string(), vec![format!("Its manifest can't be unpacked: {problem}")]),
            None => ("Amiga Forever package (RP9)".to_string(), vec![format!("It has no {MANIFEST}, so what it holds isn't described.")]),
        };
        if let Some(help) = help {
            info.insert(info.len().min(1), InfoSection { title: "About it".into(), rows: vec![(String::new(), help)] });
        }
        Ok(Self { zip, heading, notes, info, picture })
    }
}

/// All the manifest says, for File properties: what the program is, the Amiga it was set up for, its disks, and
/// the player it was made for.
fn sections(xml: &str) -> Result<Vec<InfoSection>, String> {
    let document = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let root = document.root_element();
    let find = |name: &str| root.descendants().find(|n| n.has_tag_name(name));
    let trimmed = |n: roxmltree::Node| n.text().map(str::trim).unwrap_or_default().to_string();
    let mut out = Vec::new();
    let mut add = |title: &str, rows: Vec<(String, String)>| {
        if !rows.is_empty() {
            out.push(InfoSection { title: title.into(), rows });
        }
    };

    let mut program = Vec::new();
    if let Some(description) = find("description") {
        for (tag, label) in [("title", "Title"), ("entity", "By"), ("year", "Year"), ("type", "Kind"), ("genre", "Genre"), ("rating", "Rating")] {
            for node in description.children().filter(|n| n.has_tag_name(tag)) {
                let value = trimmed(node);
                if !value.is_empty() {
                    program.push((label.to_string(), value));
                }
            }
        }
    }
    add("Program", program);

    let mut machine = Vec::new();
    let mut settings = Vec::new();
    if let Some(configuration) = find("configuration") {
        for node in configuration.children().filter(roxmltree::Node::is_element) {
            let value = trimmed(node);
            match node.tag_name().name() {
                "system" => machine.push(("Model".to_string(), machine_name(&value))),
                "ram" => {
                    let size = value.parse::<u64>().map_or(value.clone(), memory);
                    let kind = match node.attribute("type") {
                        Some("chip") => "Chip RAM",
                        Some("z3") => "Zorro III RAM",
                        Some("fast") => "Fast RAM",
                        Some(other) => other,
                        None => "RAM",
                    };
                    machine.push((kind.to_string(), size));
                }
                "peripheral" => match value.as_str() {
                    "cpu" => {
                        let speed = match node.attribute("speed") {
                            Some("max") => " (as fast as it will go)",
                            _ => "",
                        };
                        machine.push(("CPU".into(), format!("{}{speed}", node.attribute("type").unwrap_or("68000"))));
                    }
                    "fpu" => machine.push(("FPU".into(), node.attribute("type").unwrap_or("yes").to_string())),
                    "jit" => {
                        let cache = node.attribute("memory").and_then(|m| m.parse::<u64>().ok()).map(|m| format!(", {} cache", memory(m))).unwrap_or_default();
                        let fpu = if node.attribute("fpu") == Some("false") { ", without the FPU" } else { "" };
                        machine.push(("JIT".into(), format!("on{cache}{fpu}")));
                    }
                    "floppy" => {
                        let unit = node.attribute("unit").unwrap_or("0");
                        machine.push((format!("Drive DF{unit}:"), node.attribute("type").map_or("floppy".to_string(), |t| t.to_uppercase())));
                    }
                    "a-501" => machine.push(("Expansion".into(), "A501 (512 KB more)".into())),
                    other => settings.push(("Peripheral".into(), other.to_string())),
                },
                "boot" => {
                    let readonly = if node.attribute("readonly") == Some("true") { ", read only" } else { "" };
                    machine.push(("Starts from".into(), format!("Amiga Forever's system {} {value}{readonly}", node.attribute("type").unwrap_or("disk").to_uppercase())));
                }
                "compatibility" => settings.push(("Compatibility".into(), value)),
                "clip" => {
                    let at = |name| node.attribute(name).unwrap_or("?");
                    settings.push(("Display clip".into(), format!("{}\u{D7}{} at {}, {}", at("width"), at("height"), at("left"), at("top"))));
                }
                other if !value.is_empty() => settings.push((other.to_string(), value)),
                _ => {}
            }
        }
    }
    add("Machine", machine);

    let mut media = Vec::new();
    if let Some(node) = find("media") {
        let mut floppy = 0;
        for item in node.children().filter(roxmltree::Node::is_element) {
            let name = trimmed(item);
            match item.tag_name().name() {
                "floppy" => {
                    floppy += 1;
                    media.push((format!("Floppy {floppy}"), name));
                }
                "harddrive" => media.push(("Hard drive".into(), name)),
                other => media.push((other.to_string(), name)),
            }
        }
    }
    add("Disks", media);
    add("Emulator settings", settings);

    let mut made_for = Vec::new();
    for (tag, label) in [("host", "Player"), ("playerversion", "Player version")] {
        if let Some(value) = find(tag).map(trimmed).filter(|v| !v.is_empty()) {
            made_for.push((label.to_string(), value));
        }
    }
    if let Some(application) = find("application") {
        if let Some(version) = application.attribute("libraryversion") {
            made_for.push(("Library version".into(), version.to_string()));
        }
        if let Some(oid) = application.attribute("oid") {
            made_for.push(("Catalogue ID".into(), oid.to_string()));
        }
    }
    add("Made for", made_for);
    Ok(out)
}

/// The heading and notes the manifest gives.
fn summary(xml: &str) -> Result<(String, Vec<String>), String> {
    let document = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let root = document.root_element();
    let find = |name: &str| root.descendants().find(|n| n.has_tag_name(name));
    let text = |name: &str| find(name).and_then(|n| n.text()).map(str::trim).filter(|t| !t.is_empty());

    let mut heading = "Amiga Forever package (RP9)".to_string();
    if let Some(title) = text("title") {
        heading += &format!(": {title}");
        let by: Vec<&str> = [text("entity"), text("year")].into_iter().flatten().collect();
        if !by.is_empty() {
            heading += &format!(" ({})", by.join(", "));
        }
    }
    if let Some(kind) = text("type") {
        heading += &format!(" \u{B7} {kind}");
    }

    let mut notes = Vec::new();
    let mut setup = Vec::new();
    if let Some(system) = text("system") {
        setup.push(machine_name(system));
    }
    let peripherals = || root.descendants().filter(|n| n.has_tag_name("peripheral"));
    if let Some(cpu) = peripherals().find(|n| n.text() == Some("cpu")).and_then(|n| n.attribute("type")) {
        setup.push(cpu.to_string());
    }
    for ram in root.descendants().filter(|n| n.has_tag_name("ram")) {
        if let Some(bytes) = ram.text().and_then(|t| t.trim().parse::<u64>().ok()) {
            let kind = match ram.attribute("type") {
                Some("chip") => "chip RAM",
                Some("z3") => "Zorro III RAM",
                _ => "fast RAM",
            };
            setup.push(format!("{} {kind}", memory(bytes)));
        }
    }
    if !setup.is_empty() {
        notes.push(format!("Set up for: {}.", setup.join(", ")));
    }
    if let Some(boot) = find("boot") {
        let disk = boot.attribute("type").unwrap_or("disk").to_uppercase();
        let version = boot.text().map(str::trim).unwrap_or_default();
        notes.push(format!("It starts from Amiga Forever's own system {disk} ({version}), which isn't in the package."));
    }
    if let Some(media) = find("media") {
        let named = |tag: &str| media.children().filter(|n| n.has_tag_name(tag)).filter_map(|n| n.text()).map(str::trim).collect::<Vec<_>>();
        let floppies = named("floppy");
        match floppies.len() {
            0 => {}
            1 => notes.push(format!("Floppy: {}.", floppies[0])),
            _ => notes.push(format!("Floppies, in the order they go in: {}.", floppies.join(", "))),
        }
        let drives = named("harddrive");
        if !drives.is_empty() {
            notes.push(format!("Hard drive{}: {}.", if drives.len() == 1 { "" } else { "s" }, drives.join(", ")));
        }
    }
    Ok((heading, notes))
}

/// "a-1200" as Amiga people say it, "A1200".
fn machine_name(system: &str) -> String {
    match system.strip_prefix("a-") {
        Some(model) => format!("A{}", model.to_uppercase()),
        None => system.to_string(),
    }
}

fn memory(bytes: u64) -> String {
    if bytes >= 1 << 20 && bytes.is_multiple_of(1 << 20) { format!("{} MB", bytes >> 20) } else { format!("{} KB", bytes >> 10) }
}

impl Volume for Rp9 {
    fn describe(&self) -> String {
        self.heading.clone()
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        self.zip.list(dir)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        self.zip.read(entry)
    }

    fn notes(&self) -> Vec<String> {
        self.notes.iter().cloned().chain(self.zip.notes()).collect()
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        self.zip.text_preset(entry)
    }

    fn info(&self) -> Vec<InfoSection> {
        self.info.clone()
    }

    fn picture(&self) -> Option<Vec<u8>> {
        self.picture.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest as Amiga Forever's packages have it, cut down.
    const TWO_FLOPPIES: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<rp9 xmlns="http://www.retroplatform.com">
	<application>
		<description><type>demo</type><entity>Kefrens</entity><title>Desert Dream</title><year>1993</year></description>
		<configuration>
			<system>a-500</system>
			<ram type="chip">1048576</ram>
			<peripheral>a-501</peripheral>
			<peripheral type="dd" unit="1">floppy</peripheral>
		</configuration>
		<media><floppy>desertdream-1.adf</floppy><floppy>desertdream-2.adf</floppy></media>
	</application>
</rp9>"#;

    #[test]
    fn the_manifest_says_what_it_is_and_how_it_was_set_up() {
        let (heading, notes) = summary(TWO_FLOPPIES).unwrap();
        assert_eq!(heading, "Amiga Forever package (RP9): Desert Dream (Kefrens, 1993) \u{B7} demo");
        assert_eq!(notes, ["Set up for: A500, 1 MB chip RAM.", "Floppies, in the order they go in: desertdream-1.adf, desertdream-2.adf."]);
    }

    #[test]
    fn file_properties_get_the_whole_manifest() {
        let info = sections(TWO_FLOPPIES).unwrap();
        let titles: Vec<&str> = info.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Program", "Machine", "Disks"]);
        let row = |title: &str, label: &str| info.iter().find(|s| s.title == title).and_then(|s| s.rows.iter().find(|r| r.0 == label)).map(|r| r.1.clone());
        assert_eq!(row("Program", "By").as_deref(), Some("Kefrens"));
        assert_eq!(row("Machine", "Chip RAM").as_deref(), Some("1 MB"));
        assert_eq!(row("Machine", "Drive DF1:").as_deref(), Some("DD"));
        assert_eq!(row("Disks", "Floppy 2").as_deref(), Some("desertdream-2.adf"));
    }

    #[test]
    fn a_hard_drive_package_says_it_needs_amiga_forevers_system() {
        let xml = r#"<rp9 xmlns="http://www.retroplatform.com"><application><description><title>1000%</title></description>
            <configuration><system>a-1200</system><boot type="hdf" readonly="true">311</boot><ram type="fast">8388608</ram>
            <peripheral type="68020" speed="max">cpu</peripheral></configuration>
            <media><harddrive>af-application.hdf</harddrive></media></application></rp9>"#;
        let (heading, notes) = summary(xml).unwrap();
        assert_eq!(heading, "Amiga Forever package (RP9): 1000%");
        assert_eq!(notes, ["Set up for: A1200, 68020, 8 MB fast RAM.", "It starts from Amiga Forever's own system HDF (311), which isn't in the package.", "Hard drive: af-application.hdf."]);
    }

    /// Amiga Forever packages, if there are some in ..\testmedia\rp9 (or the folder LIMONITER_RP9 names): each opens
    /// with its manifest read, and every disk inside opens and has every file read.
    #[test]
    fn real_packages_open_and_their_disks_read() {
        let folder = std::env::var_os("LIMONITER_RP9").map_or_else(|| std::path::PathBuf::from("../testmedia/rp9"), std::path::PathBuf::from);
        let Ok(items) = std::fs::read_dir(&folder) else {
            return;
        };
        let mut problems = Vec::new();
        let (mut opened, mut own_loader) = (0, 0);
        for path in items.flatten().map(|i| i.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rp9"))) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let bytes = std::fs::read(&path).unwrap();
            assert!(super::super::is_image(&name, &bytes[..bytes.len().min(0x8800)], bytes.len() as u64), "{name} is taken for an image");
            let volume = super::super::open(Arc::new(bytes), &name).unwrap_or_else(|p| panic!("{name}: {p}"));
            assert!(volume.describe().starts_with("Amiga Forever package (RP9): "), "{name}: {}", volume.describe());
            assert!(!volume.notes().iter().any(|n| n.contains("manifest")), "{name}: {:?}", volume.notes());
            let info = volume.info();
            assert!(info.iter().any(|s| s.title == "Program") && info.iter().any(|s| s.title == "About it"), "{name}: {info:?}");
            assert!(volume.picture().is_some_and(|p| p.starts_with(b"\x89PNG")), "{name} has its preview");
            for entry in volume.list(None).unwrap() {
                let lower = entry.name.to_lowercase();
                if !(lower.ends_with(".adf") || lower.ends_with(".hdf")) {
                    continue;
                }
                let disk = volume.read(&entry).unwrap();
                match super::super::open(Arc::new(disk), &entry.open_name) {
                    Ok(inner) => {
                        let mut report = String::new();
                        super::super::tests::collect(&*inner, None, "", &format!("{name}!{}", entry.name), &mut report, &mut problems, 0, &mut |b| b.len().to_string());
                        opened += 1;
                    }
                    // A demo that loads itself its own way has no files to list; saying so is right.
                    Err(problem) if problem.contains("can still be shown in hex") => own_loader += 1,
                    Err(problem) => problems.push(format!("{name}!{}: {problem}", entry.name)),
                }
            }
        }
        eprintln!("{opened} disks opened, {own_loader} load themselves their own way");
        // Kefrens wrote this file's 22 blocks straight to the boot area with their own loader, as bare data with
        // none of the Old File System's block headers; unadf can't read it either, and nor could AmigaDOS.
        const KNOWN: [&str; 1] = ["Desert Dream (Kefrens, 1993, Amiga).rp9!desertdream-2.adf BootGirl.data: "];
        problems.retain(|p| !KNOWN.iter().any(|k| p.starts_with(k)));
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
