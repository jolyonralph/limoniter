//! Puts the icon and the name people see in Windows into the program file.

fn main() {
    println!("cargo:rerun-if-changed=data/limoniter.ico");
    println!("cargo:rerun-if-changed=build.rs");
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
