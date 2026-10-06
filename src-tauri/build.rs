fn main() {
    // Replaces Tauri's default manifest (which only declares Common-Controls v6) with one that also
    // pins asInvoker, Windows 10/11 compatibility, PerMonitorV2 DPI and long paths.
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app.manifest"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run tauri-build");
}
