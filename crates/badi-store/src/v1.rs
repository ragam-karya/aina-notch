// Reading the keys v1 saved in config.json ("enc:" + base64 of Electron's safeStorage output). On
// Windows that is Chromium's OSCrypt: an AES-256-GCM key kept DPAPI-protected in the profile's
// "Local State" (v1 pointed it at <userData>/chromium), and values "v10" + 12-byte nonce + ciphertext
// with its tag. Read only: nothing in v1's files changes.

use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData};

fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, 0, &mut out).ok()?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
        Some(bytes)
    }
}

// The AES key from "Local State" (os_crypt.encrypted_key: "DPAPI" + the protected key).
fn key(dir: &Path) -> Option<Vec<u8>> {
    let file = [dir.join("chromium").join("Local State"), dir.join("Local State")].into_iter().find(|p| p.exists())?;
    let state: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    let raw = STANDARD.decode(state["os_crypt"]["encrypted_key"].as_str()?).ok()?;
    unprotect(raw.strip_prefix(b"DPAPI")?)
}

/// The plain value of a v1 "enc:…" secret, or None when it can't be read.
pub fn decrypt(dir: &Path, value: &str) -> Option<String> {
    let blob = STANDARD.decode(value.strip_prefix("enc:")?).ok()?;
    let plain = match blob.strip_prefix(b"v10") {
        Some(rest) if rest.len() > 12 + 16 => {
            let cipher = Aes256Gcm::new_from_slice(&key(dir)?).ok()?;
            cipher.decrypt(Nonce::from_slice(&rest[..12]), &rest[12..]).ok()?
        }
        // Older Chromium protected each value with DPAPI directly.
        _ => unprotect(&blob)?,
    };
    String::from_utf8(plain).ok().filter(|s| !s.is_empty())
}
