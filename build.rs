fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/icons/icon.ico");
        resource.set("ProductName", "MD Reader");
        resource.set("FileDescription", "Native Markdown Reader");
        resource.set("LegalCopyright", "MIT License");
        resource
            .compile()
            .expect("failed to compile Windows resources");
    }
}
