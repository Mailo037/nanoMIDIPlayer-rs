fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "nanoMIDIPlayer");
        res.set("FileDescription", "nanoMIDIPlayer");
        if let Err(e) = res.compile() {
            println!("cargo:warning=failed to embed icon: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
