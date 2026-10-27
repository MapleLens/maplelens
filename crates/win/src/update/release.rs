//! Maintainer tools, kept in the app so they share its exact manifest format and built-in key:
//! - `maplelens --sign-release maplelens-vX.Y.Z-windows.zip`: release step; writes the signed manifest (`.json`
//!   and `.sig`) next to the zip. The private key comes from the UPDATE_SIGNING_KEY environment variable (a CI
//!   secret); the result is verified against the built-in public key, so a wrong or rotated secret fails the
//!   release instead of shipping updates nobody would accept.
//! - `maplelens --make-update-key PATH [--no-secret]`: one-time setup; backs the new private key up to PATH (never
//!   overwriting), stores it as the UPDATE_SIGNING_KEY Actions secret through `gh` (stdin, never argv) and prints
//!   the public key for `update::PUBLIC_KEY`. Rotating the key strands every installed copy.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use super::manifest::{asset_name, manifest_name, verify_manifest};
use super::version::{format_version, parse_version};
use super::{PUBLIC_KEY, REPO, UpdateError, fail};

const SECRET_NAME: &str = "UPDATE_SIGNING_KEY";

pub fn sign_release(zip: &Path) -> Result<String, UpdateError> {
    let secret = std::env::var(SECRET_NAME).unwrap_or_default();
    let raw: [u8; 32] = STANDARD
        .decode(secret.trim())
        .ok()
        .and_then(|k| k.try_into().ok())
        .ok_or_else(|| UpdateError(format!("{SECRET_NAME} is not set or not a base64 Ed25519 key")))?;
    let name = zip.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let version =
        format_version(parse_version(name.trim_start_matches("maplelens-").trim_end_matches("-windows.zip"))?);
    if name != asset_name(&version) {
        return fail(format!("unexpected asset name {name}"));
    }
    let data = std::fs::read(zip).map_err(|e| UpdateError(format!("cannot read {name}: {e}")))?;
    let sha256: String = Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
    let manifest = serde_json::to_string_pretty(&serde_json::json!({
        "version": version, "asset": name, "size": data.len(), "sha256": sha256
    }))
    .map_err(|e| UpdateError(e.to_string()))?;
    let signature = STANDARD.encode(SigningKey::from_bytes(&raw).sign(manifest.as_bytes()).to_bytes());
    verify_manifest(manifest.as_bytes(), signature.as_bytes(), PUBLIC_KEY)?; // the app's key must accept it
    let out = zip.with_file_name(manifest_name(&version));
    let write = |path: &Path, text: &str| std::fs::write(path, text).map_err(|e| UpdateError(e.to_string()));
    write(&out, &manifest)?;
    write(&out.with_file_name(format!("{}.sig", manifest_name(&version))), &signature)?;
    Ok(format!("signed {} ({} bytes)", out.display(), data.len()))
}

pub fn make_update_key(out: &Path, set_secret: bool) -> Result<String, UpdateError> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| UpdateError(format!("no randomness: {e}")))?;
    let key = SigningKey::from_bytes(&seed);
    let private = STANDARD.encode(key.to_bytes());
    let public = STANDARD.encode(key.verifying_key().to_bytes());
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| UpdateError(e.to_string()))?;
    }
    // create_new: never clobber an older key.
    let mut backup = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(|e| UpdateError(format!("cannot create {}: {e}", out.display())))?;
    writeln!(backup, "{private}").map_err(|e| UpdateError(e.to_string()))?;
    if set_secret {
        let mut gh = Command::new("gh")
            .args(["secret", "set", SECRET_NAME, "--repo", REPO])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| UpdateError(format!("gh could not run ({e}); the key is saved in {}", out.display())))?;
        if let Some(mut stdin) = gh.stdin.take() {
            stdin.write_all(private.as_bytes()).map_err(|e| UpdateError(e.to_string()))?;
        }
        let status = gh.wait().map_err(|e| UpdateError(e.to_string()))?;
        if !status.success() {
            return fail(format!("gh could not store the secret; the key is saved in {}", out.display()));
        }
    }
    Ok(format!("private key backed up to {}\npublic key: {public}", out.display()))
}
