//! On Windows the executable carries the icon and the version resource (the version is the
//! workspace's, as the other programs have it).

fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/workpiece.ico");
        resource.set("ProductName", "Workpiece");
        resource.set("FileDescription", "Workpiece");
        resource.set("LegalCopyright", "MIT OR Apache-2.0");
        if let Err(error) = resource.compile() {
            panic!("cannot add the Windows resources: {error}");
        }
    }
}
