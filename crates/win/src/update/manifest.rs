//! The signed release manifest: what a downloaded zip must be, vouched for by a key baked into the app.
//!
//! A hash published beside the zip only catches a corrupt download; anyone able to swap the zip could swap the
//! hash. The manifest is signed with an Ed25519 key whose public half ships in the app, so it holds up even if the
//! download host is compromised.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::Value;

use super::version::{format_version, parse_version};
use super::{MAX_ZIP_BYTES, UpdateError, fail};

pub fn asset_name(version: &str) -> String {
    format!("maplelens-v{version}-windows.zip")
}

pub fn manifest_name(version: &str) -> String {
    format!("maplelens-v{version}-manifest.json")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub version: String,
    pub asset: String,
    pub size: u64,
    pub sha256: String,
}

/// Check the signature over the exact bytes first; only then is the content parsed.
pub fn verify_manifest(data: &[u8], signature: &[u8], public_key: &str) -> Result<Manifest, UpdateError> {
    let invalid = || UpdateError("the update's signature is not valid".into());
    let key: [u8; 32] = STANDARD.decode(public_key).ok().and_then(|k| k.try_into().ok()).ok_or_else(invalid)?;
    let key = VerifyingKey::from_bytes(&key).map_err(|_| invalid())?;
    let text = std::str::from_utf8(signature).map_err(|_| invalid())?;
    let signature: [u8; 64] = STANDARD.decode(text.trim()).ok().and_then(|s| s.try_into().ok()).ok_or_else(invalid)?;
    key.verify_strict(data, &Signature::from_bytes(&signature)).map_err(|_| invalid())?;

    let malformed = || UpdateError("the update manifest is malformed".into());
    let raw: Value = serde_json::from_slice(data).map_err(|_| malformed())?;
    let version =
        format_version(parse_version(raw["version"].as_str().ok_or_else(malformed)?).map_err(|_| malformed())?);
    let asset = raw["asset"].as_str().ok_or_else(malformed)?.to_string();
    // A number, not a string or a bool, and a whole one.
    let size = raw["size"].as_u64().filter(|_| raw["size"].is_u64()).ok_or_else(malformed)?;
    let sha256 = raw["sha256"].as_str().ok_or_else(malformed)?.to_string();
    let hex = sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if asset != asset_name(&version) || size == 0 || size > MAX_ZIP_BYTES || !hex {
        return fail("the update manifest is malformed");
    }
    Ok(Manifest { version, asset, size, sha256 })
}
