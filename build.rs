fn main() {
    println!("cargo:rerun-if-changed=assets/icons/opterm.ico");
    #[cfg(windows)]
    {
        winresource::WindowsResource::new()
            .set_icon("assets/icons/opterm.ico")
            .set("ProductName", "Opterm")
            .set("FileDescription", "Opterm Terminal")
            .compile()
            .expect("embed the Opterm Windows application icon");
    }
}
