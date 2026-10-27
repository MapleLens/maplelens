//! Sealing the store book for the paired phones, exactly as the web page does (`docs/app/phone/relay.js` `seal`):
//! AES-256-GCM under HKDF-SHA256(the pairing secret, salt "maplelens/store-state", info "v1"), a random 12-byte
//! nonce, and base64url(nonce || ciphertext || tag). The relay stores what it cannot read. AES-GCM is Windows' own
//! (CNG), HMAC is written on the update chain's SHA-256: no new dependency.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use windows::Win32::Security::Cryptography::{
    BCRYPT_AES_GCM_ALG_HANDLE, BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO, BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO_VERSION,
    BCRYPT_FLAGS, BCRYPT_KEY_HANDLE, BCryptDestroyKey, BCryptEncrypt, BCryptGenerateSymmetricKey,
};

const SALT: &[u8] = b"maplelens/store-state";
const INFO: &[u8] = b"v1";
const NONCE: usize = 12;
const TAG: usize = 16;
const BLOCK: usize = 64;

fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut padded = [0u8; BLOCK];
    if key.len() > BLOCK {
        padded[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        padded[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(padded.map(|b| b ^ 0x36));
    for part in parts {
        inner.update(part);
    }
    let mut outer = Sha256::new();
    outer.update(padded.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// HKDF-SHA256 (RFC 5869) with one block of output: the 256-bit key AES-GCM takes.
fn hkdf(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let prk = hmac(salt, &[ikm]);
    hmac(&prk, &[info, &[1]])
}

/// AES-256-GCM of `plain` with no additional data: ciphertext || tag.
fn aes_gcm(key: &[u8; 32], nonce: &[u8; NONCE], plain: &[u8]) -> Option<Vec<u8>> {
    let mut handle = BCRYPT_KEY_HANDLE::default();
    // SAFETY: the pseudo-handle needs no opening; `handle` receives a key we destroy below.
    unsafe { BCryptGenerateSymmetricKey(BCRYPT_AES_GCM_ALG_HANDLE, &mut handle, None, key, 0) }.ok().ok()?;
    let mut nonce = *nonce;
    let mut tag = [0u8; TAG];
    let info = BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO {
        cbSize: size_of::<BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO>() as u32,
        dwInfoVersion: BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO_VERSION,
        pbNonce: nonce.as_mut_ptr(),
        cbNonce: NONCE as u32,
        pbTag: tag.as_mut_ptr(),
        cbTag: TAG as u32,
        ..Default::default()
    };
    let mut out = vec![0u8; plain.len()];
    let mut written = 0u32;
    // SAFETY: `info` points at `nonce` and `tag`, which outlive the call; GCM output is the input's length.
    let status = unsafe {
        BCryptEncrypt(
            handle,
            Some(plain),
            Some(std::ptr::from_ref(&info).cast()),
            None,
            Some(&mut out),
            &mut written,
            BCRYPT_FLAGS(0),
        )
    };
    // SAFETY: the key made above, used no more.
    unsafe {
        let _ = BCryptDestroyKey(handle);
    }
    status.ok().ok()?;
    out.truncate(written as usize);
    out.extend_from_slice(&tag);
    Some(out)
}

/// `value` as JSON, sealed for the devices holding `key` (`<id>.<secret>`); None without randomness or a cipher.
pub fn seal(key: &str, value: &serde_json::Value) -> Option<String> {
    let (_, secret) = key.split_once('.')?;
    let aes = hkdf(secret.as_bytes(), SALT, INFO);
    let mut nonce = [0u8; NONCE];
    getrandom::fill(&mut nonce).ok()?;
    let sealed = aes_gcm(&aes, &nonce, value.to_string().as_bytes())?;
    let mut out = nonce.to_vec();
    out.extend(sealed);
    Some(URL_SAFE_NO_PAD.encode(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn hkdf_matches_rfc_5869() {
        // Test case 1, the first 32 bytes of its output.
        let salt: Vec<u8> = (0..=0x0c).collect();
        let info: Vec<u8> = (0xf0..=0xf9).collect();
        let okm = hkdf(&[0x0b; 22], &salt, &info);
        assert_eq!(hex(&okm), "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf");
    }

    #[test]
    fn aes_gcm_matches_the_standard_vectors() {
        // NIST GCM test cases 13 and 14 (AES-256, zero key and nonce).
        assert_eq!(hex(&aes_gcm(&[0; 32], &[0; 12], &[]).unwrap()), "530f8afbc74536b9a963b4f1c4cb738b");
        assert_eq!(
            hex(&aes_gcm(&[0; 32], &[0; 12], &[0; 16]).unwrap()),
            "cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919"
        );
    }

    #[test]
    fn a_sealed_book_is_nonce_ciphertext_and_tag() {
        let value = serde_json::json!({"v": 1, "owner": "ShopOwner"});
        let key = "AbCdEfGhIjKlMnOp.qRsTuVwXyZ0123456789-_ab";
        let (a, b) = (seal(key, &value).unwrap(), seal(key, &value).unwrap());
        assert_ne!(a, b, "a fresh nonce each time");
        let bytes = URL_SAFE_NO_PAD.decode(&a).unwrap();
        assert_eq!(bytes.len(), NONCE + value.to_string().len() + TAG);
        assert_eq!(seal("no-dot", &value), None);
    }
}
