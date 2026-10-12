// File: build.rs
// Purpose: Photonio — embed the Windows executable icon resource at build time.

fn main() {
    // Only meaningful on Windows; no-op elsewhere so cross-compiles don't fail.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("images/icon.ico");
        res.compile().expect("failed to compile Windows resource (icon.ico)");
    }
}
