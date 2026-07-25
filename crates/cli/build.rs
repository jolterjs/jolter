#[cfg(windows)]
fn main() {
    let mut res = winres::WindowsResource::new();
    res.set("CompanyName", "Jolter");
    res.set(
        "FileDescription",
        "Jolter - JavaScript Runtime and Tool Manager",
    );
    res.set("ProductName", "Jolter");
    res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
    res.set("FileVersion", env!("CARGO_PKG_VERSION"));
    res.set("LegalCopyright", "Copyright (c) 2026 Jolter Contributors");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = std::path::Path::new(&manifest_dir).join("../..");
    let ico_path = root.join("assets/app-icon.ico");

    res.set_icon(ico_path.to_str().unwrap());

    if let Err(e) = res.compile() {
        eprintln!("Failed to compile Windows resources: {e}");
    }
}

#[cfg(not(windows))]
fn main() {}
