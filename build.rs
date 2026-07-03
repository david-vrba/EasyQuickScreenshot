// Embeds the app icon into eqs.exe. Needs a resource compiler (windres or rc.exe);
// if none is installed the build still succeeds — you just get a plain exe icon.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::path::Path::new("assets/icon.ico").exists() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=exe icon not embedded (resource compiler missing?): {e}");
        }
    }
}
