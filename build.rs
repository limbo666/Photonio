// File: build.rs
// Purpose: Photonio — embed the Windows executable icon resource at build time.
//          The icon is generated from `images/logof.png` as a multi-size ICO
//          so the exe/taskbar icon always tracks that source image.

fn main() {
    // Only meaningful on Windows; no-op elsewhere so cross-compiles don't fail.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let ico_path = generate_icon();
        let mut res = winres::WindowsResource::new();
        res.set_icon(ico_path.to_str().expect("OUT_DIR path is not UTF-8"));
        res.compile().expect("failed to compile Windows resource (icon)");
    }
}

/// Decode `images/logof.png` and encode a multi-size ICO into `OUT_DIR`.
fn generate_icon() -> std::path::PathBuf {
    let png = std::fs::read("images/logof.png").expect("read images/logof.png");
    let img = image::load_from_memory(&png)
        .expect("decode images/logof.png")
        .to_rgba8();

    // Windows scales these sizes to match the shell/taskbar; 256 is the PNG-
    // compressed large frame, the rest are downscaled from the source.
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let mut frames = Vec::new();
    for &size in &sizes {
        let resized = image::imageops::resize(
            &img,
            size,
            size,
            image::imageops::FilterType::Lanczos3,
        );
        let frame = image::codecs::ico::IcoFrame::as_png(
            resized.as_raw(),
            size,
            size,
            image::ExtendedColorType::Rgba8,
        )
        .expect("encode ICO frame");
        frames.push(frame);
    }

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let ico_path = std::path::Path::new(&out_dir).join("photonio.ico");
    let file = std::fs::File::create(&ico_path).expect("create ICO file");
    image::codecs::ico::IcoEncoder::new(file)
        .encode_images(&frames)
        .expect("encode ICO");
    ico_path
}
