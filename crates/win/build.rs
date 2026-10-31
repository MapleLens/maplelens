//! Embed the app icon and version details in maplelens.exe (Explorer, the taskbar and antivirus scanners read
//! them; an exe without version info looks more suspicious to the latter), load its DLLs from System32 only, and
//! `cfg(fixtures)` when the local game captures are present (`tests/fixtures`, kept out of git: they show players'
//! names; tests that need them are ignored without them).

fn main() {
    println!("cargo::rustc-check-cfg=cfg(fixtures)");
    println!("cargo::rerun-if-changed=../../tests/fixtures");
    if std::path::Path::new("../../tests/fixtures").is_dir() {
        println!("cargo::rustc-cfg=fixtures");
    }
    println!("cargo:rerun-if-changed=assets/maplelens.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // The exe's imports (dwmapi, d3d11, bcrypt, winmm...) come from System32 only, never from the folder it was
        // run from, where any program can drop a DLL of that name: the app runs as administrator (`elevate`).
        // 0x800 is LOAD_LIBRARY_SEARCH_SYSTEM32.
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/maplelens.ico")
        .set("FileDescription", "MapleLens - overlays for MapleStory Classic")
        .set("ProductName", "MapleLens")
        .set("LegalCopyright", "GPL-3.0-or-later, MapleLens contributors");
    if let Err(error) = resource.compile() {
        // The icon is cosmetic: a machine without the Windows SDK's rc.exe still builds a working exe.
        println!("cargo:warning=app icon not embedded: {error}");
    }
}
