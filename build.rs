//! Puts the icon and the name people see in Windows into the program file, and the How To Code guides (v8 for the
//! Amiga, and the ZX Spectrum's, each with its examples) into the program, as lists of their files.

fn main() {
    println!("cargo:rerun-if-changed=data/limoniter.ico");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=data/howtocode");
    println!("cargo:rerun-if-changed=data/howtocode-spectrum");
    guide("howtocode", "howtocode_files.rs");
    guide("howtocode-spectrum", "howtocode_spectrum_files.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("data/limoniter.ico");
        resource.set("ProductName", "Limoniter");
        resource.set("FileDescription", "Limoniter text and hex editor");
        if let Err(problem) = resource.compile() {
            panic!("The program's icon couldn't be added: {problem}");
        }
    }
}

/// A guide's files (from data/FOLDER), each by its path in the guide and its bytes, in OUT_DIR/LIST.
fn guide(folder: &str, list: &str) {
    let root = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("data").join(folder);
    let mut files = Vec::new();
    let mut folders = vec![root.clone()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                println!("cargo:rerun-if-changed={}", path.display());
                folders.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut out = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for path in &files {
        let name = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        out.push_str(&format!("    ({name:?}, include_bytes!({:?})),\n", path.display().to_string()));
    }
    out.push_str("];\n");
    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join(list), out).unwrap();
}
