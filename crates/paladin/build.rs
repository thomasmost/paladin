//! Embeds the application icon into `paladin.exe` on Windows so Explorer,
//! the taskbar, and the installer's shortcuts show the logo. cargo-packager
//! does not do this itself. No-op elsewhere.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/paladin.ico");
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/icons/paladin.ico");
        resource
            .compile()
            .expect("embedding the Windows icon resource");
    }
}
