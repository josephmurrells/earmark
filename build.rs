//! Embeds the icon and version details in `earmark.exe`, so Explorer and the
//! taskbar show them. Regenerate the icon with `cargo test write_ico -- --ignored`.

fn main() {
    println!("cargo:rerun-if-changed=assets/earmark.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/earmark.ico")
        .set("ProductName", "Earmark")
        .set("FileDescription", "Earmark")
        .set(
            "LegalCopyright",
            "Copyright (c) 2026 The Earmark authors. MIT OR Apache-2.0.",
        );
    // A build tool may panic: a missing resource compiler should stop the build loudly.
    resource
        .compile()
        .expect("embed the Windows icon and version info (needs the Windows SDK's rc.exe)");
}
