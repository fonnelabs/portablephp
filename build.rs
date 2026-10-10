fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("src/app_icon.ico");
        res.compile().unwrap();
    }
}