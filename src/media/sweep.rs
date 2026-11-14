//! A sweep over a folder of real images, to compare what's read with independent tools. Run by hand:
//! `LIMONITER_SWEEP=<folder> LIMONITER_SWEEP_OUT=<folder> cargo test sweep_a_folder -- --ignored`. Each image's
//! description, notes and files are written to `listing.txt` in the output folder, and each file's bytes to
//! `<image>/<file>`, beside it; an image that won't open says why. Nothing in the folder is run.

use super::{Entry, Volume, open};
use std::path::Path;
use std::sync::Arc;

fn save(volume: &dyn Volume, dir: Option<&Entry>, prefix: &str, out: &Path, listing: &mut String, depth: usize) {
    if depth > 6 {
        return;
    }
    let entries = match volume.list(dir) {
        Ok(entries) => entries,
        Err(problem) => return listing.push_str(&format!("  LIST {prefix}: {problem}\n")),
    };
    for entry in entries {
        let path = format!("{prefix}{}", entry.name);
        if entry.is_dir {
            save(volume, Some(&entry), &format!("{path}/"), out, listing, depth + 1);
            continue;
        }
        match volume.read(&entry) {
            Ok(bytes) => {
                listing.push_str(&format!("  {path}\t{}\t{}\t{}\t{}\n", entry.open_name, entry.kind, entry.detail, bytes.len()));
                let ending = entry.open_name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
                if crate::basic::is_program_file(&ending, Some(&bytes)) {
                    let basic = crate::basic::list(&bytes, &ending, crate::charset::utf8());
                    listing.push_str(&format!("    BASIC {}: {} lines {:?}\n", basic.dialect, basic.lines.len(), basic.notes));
                    let _ = std::fs::write(out.join(format!("{}.lst", path.replace('/', "_"))), basic.lines.join("\n"));
                }
                // A second file of the same name, as tapes often have, is kept as name#2.
                let mut target = out.join(path.replace('/', "_"));
                let mut copy = 1;
                while target.exists() {
                    copy += 1;
                    target = out.join(format!("{}#{copy}", path.replace('/', "_")));
                }
                let _ = std::fs::write(target, &bytes);
            }
            Err(problem) => listing.push_str(&format!("  READ {path}: {problem}\n")),
        }
    }
}

#[test]
#[ignore]
fn sweep_a_folder() {
    let (Ok(folder), Ok(out)) = (std::env::var("LIMONITER_SWEEP"), std::env::var("LIMONITER_SWEEP_OUT")) else {
        return;
    };
    let (folder, out) = (Path::new(&folder), Path::new(&out));
    let mut names: Vec<_> = std::fs::read_dir(folder).unwrap().flatten().map(|item| item.path()).filter(|path| path.is_file()).collect();
    names.sort();
    let mut listing = String::new();
    for path in names {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        match open(Arc::new(bytes), &name) {
            Ok(volume) => {
                listing.push_str(&format!("# {name}: {}\n", volume.describe()));
                for note in volume.notes() {
                    listing.push_str(&format!("  NOTE {note}\n"));
                }
                let target = out.join(&name);
                std::fs::create_dir_all(&target).unwrap();
                save(&*volume, None, "", &target, &mut listing, 0);
            }
            Err(problem) => listing.push_str(&format!("# {name}: REFUSED: {problem}\n")),
        }
    }
    std::fs::create_dir_all(out).unwrap();
    std::fs::write(out.join("listing.txt"), listing).unwrap();
}

/// The same for pictures: `LIMONITER_PICTURES=<folder> LIMONITER_SWEEP_OUT=<folder>`, each picture saved as a PNG
/// beside a `pictures.txt` saying what each was found to be.
#[test]
#[ignore]
fn sweep_pictures() {
    let (Ok(folder), Ok(out)) = (std::env::var("LIMONITER_PICTURES"), std::env::var("LIMONITER_SWEEP_OUT")) else {
        return;
    };
    let (folder, out) = (Path::new(&folder), Path::new(&out));
    std::fs::create_dir_all(out).unwrap();
    let mut names: Vec<_> = std::fs::read_dir(folder).unwrap().flatten().map(|item| item.path()).filter(|path| path.is_file()).collect();
    names.sort();
    let mut listing = String::new();
    for path in names {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        let Some(format) = crate::pictures::detect(&name, &bytes) else {
            listing.push_str(&format!("{name}: not a picture\n"));
            continue;
        };
        match crate::pictures::decode_named(format, &name, &bytes) {
            Ok(picture) => {
                listing.push_str(&format!("{name}: {}x{} {:?}\n", picture.width, picture.height, picture.info));
                std::fs::write(out.join(format!("{name}.png")), crate::pictures::to_png(&picture).unwrap()).unwrap();
            }
            Err(problem) => listing.push_str(&format!("{name}: {problem}\n")),
        }
    }
    std::fs::write(out.join("pictures.txt"), listing).unwrap();
}
