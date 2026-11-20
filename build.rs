//! Puts the icon and the name people see in Windows into the program file, and How To Code v8 (the Amiga demo
//! coder's guide, with its examples) into the program, as a list of its files.

fn main() {
    println!("cargo:rerun-if-changed=data/limoniter.ico");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=data/howtocode");
    guide();
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

/// How To Code's files, each by its path in the guide and its bytes, in OUT_DIR/howtocode_files.rs.
fn guide() {
    let root = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("data/howtocode");
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
    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("howtocode_files.rs"), out).unwrap();
}
