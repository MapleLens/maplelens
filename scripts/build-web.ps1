# Rebuild the web app's engine (docs/app/pkg) from crates/web: `./scripts/build-web.ps1`.
#
# GitHub Pages serves docs/ as it is in the repository, so the compiled engine is committed: rebuild and commit it
# whenever crates/core or crates/web change. Needs the wasm32 target and wasm-bindgen-cli at the version pinned in
# crates/web/Cargo.toml (`rustup target add wasm32-unknown-unknown`, `cargo install wasm-bindgen-cli --version X`).
$ErrorActionPreference = 'Stop'
$root = Join-Path $PSScriptRoot '..'
cargo build --release --package maplelens-web --target wasm32-unknown-unknown --manifest-path (Join-Path $root 'Cargo.toml')
if ($LASTEXITCODE) { exit $LASTEXITCODE }
wasm-bindgen --target web --no-typescript --out-dir (Join-Path $root 'docs/app/pkg') (Join-Path $root 'target/wasm32-unknown-unknown/release/maplelens_web.wasm')
if ($LASTEXITCODE) { exit $LASTEXITCODE }
& (Join-Path $PSScriptRoot 'stamp-web.ps1')
Write-Host 'docs/app/pkg rebuilt'
