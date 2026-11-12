fn main() {
    // Embed the app manifest: run as the invoking user (the app offers "Restart as administrator"),
    // per-monitor DPI awareness and modern common controls.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_manifest_file("assets/rigscan.manifest");
        res.set("FileDescription", "rigscan PC audit");
        res.set("ProductName", "rigscan");
        if let Err(e) = res.compile() {
            panic!("failed to embed Windows resources: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/rigscan.manifest");
}
