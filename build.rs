// Embeds the app icon and version info into jot.exe on Windows.
fn main() {
    println!("cargo:rerun-if-changed=assets/jot.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/jot.ico");
        res.set("ProductName", "jot");
        res.set("FileDescription", "A tiny native notepad");
        res.compile().expect("failed to embed the Windows resources");
    }
}
