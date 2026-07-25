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
    let png_path = root.join("assets/app-icon.png");
    let ico_path = root.join("assets/app-icon.ico");

    if ico_path.exists() {
        res.set_icon(ico_path.to_str().unwrap());
    } else if png_path.exists() {
        if let Ok(png_bytes) = std::fs::read(&png_path) {
            let out_dir = std::env::var("OUT_DIR").unwrap();
            let generated_ico = std::path::Path::new(&out_dir).join("app-icon.ico");
            let mut ico_bytes = Vec::with_capacity(22 + png_bytes.len());
            ico_bytes.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
            ico_bytes.push(0);
            ico_bytes.push(0);
            ico_bytes.push(0);
            ico_bytes.push(0);
            ico_bytes.extend_from_slice(&1_u16.to_le_bytes());
            ico_bytes.extend_from_slice(&32_u16.to_le_bytes());
            ico_bytes.extend_from_slice(&(png_bytes.len() as u32).to_le_bytes());
            ico_bytes.extend_from_slice(&22_u32.to_le_bytes());
            ico_bytes.extend_from_slice(&png_bytes);
            if std::fs::write(&generated_ico, &ico_bytes).is_ok() {
                res.set_icon(generated_ico.to_str().unwrap());
            }
        }
    }

    if let Err(e) = res.compile() {
        eprintln!("Failed to compile Windows resources: {e}");
    }
}

#[cfg(not(windows))]
fn main() {}
