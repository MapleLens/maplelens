//! `cfg(fixtures)` when the local game captures are present (`tests/fixtures`, kept out of git: they show players'
//! names). Tests that need them are ignored without them.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(fixtures)");
    println!("cargo::rerun-if-changed=../../tests/fixtures");
    if std::path::Path::new("../../tests/fixtures").is_dir() {
        println!("cargo::rustc-cfg=fixtures");
    }
}
