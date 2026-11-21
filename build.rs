fn main() {
    // Embed the app manifest: run as the invoking user (the app offers "Restart as administrator"),
    // per-monitor DPI awareness and modern common controls.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_manifest_file("assets/bootmend.manifest");
        res.set_icon("assets/bootmend.ico");
        res.set("FileDescription", "Bootmend PC check-up");
        res.set("ProductName", "Bootmend");
        res.set("LegalCopyright", "Copyright © 2026 Tanishk Kamra");
        if let Err(e) = res.compile() {
            panic!("failed to embed Windows resources: {e}");
        }
    }
    // Load the DLLs bootmend.exe links to from System32 only, never from the folder it was
    // started in, where a downloaded DLL of the same name could sit beside it.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
    println!("cargo:rerun-if-changed=assets/bootmend.manifest");
    println!("cargo:rerun-if-changed=assets/bootmend.ico");
}
