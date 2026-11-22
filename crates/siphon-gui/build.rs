fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=../../assets/siphon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/siphon.ico");
        res.set("ProductName", "Siphon Downloader");
        res.set("FileDescription", "Siphon — Windows 多线程下载器");
        res.set("LegalCopyright", "Copyright (c) 2026 Siphon Contributors (MIT)");
        if let Err(e) = res.compile() {
            println!("cargo:warning=winresource compile failed: {e}");
        }
    }
}
