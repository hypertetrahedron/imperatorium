//! Release manifests are signed (Ed25519). The SHA-256s in `manifest.json`
//! prove a download matches its manifest; the signature in
//! `manifest.json.sig` proves the manifest is the project's, since both
//! files would otherwise come from the same place. `imperatorium update`
//! (and the window's check) refuse a manifest that no trusted key signed.
//!
//! The signature is base64 of the 64-byte Ed25519 signature over the exact
//! bytes of `manifest.json`. `imperatorium release-keygen` makes a key
//! pair; `imperatorium release-package --sign-key-env NAME` signs with the
//! seed in that environment variable (the release workflow's secret).

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// The project's release public keys, base64. A release signed by any of
/// them is trusted; while a key is rotated both are listed, the new one
/// first, for at least one stable release.
/// The key made 2026-11-15; its secret is the release workflow's
/// IMPERATORIUM_SIGNING_KEY.
pub const RELEASE_KEYS: &[&str] = &["e+3P7hpKZUTaX+niTYFXB/eXOnAEjozNtkUphBXxMks="];

/// One more trusted key, for a local or mock release (rust/release/build.ps1
/// prints it). Whoever can set this variable can already run anything as
/// this user, so it opens nothing new.
pub const EXTRA_KEY_ENV: &str = "IMPERATORIUM_RELEASE_PUBKEY";

pub fn keygen() -> Result<(String, String), String> {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| format!("no randomness: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    Ok((B64.encode(seed), B64.encode(key.verifying_key().to_bytes())))
}

fn bytes32(b64: &str, what: &str) -> Result<[u8; 32], String> {
    let raw = B64.decode(b64.trim()).map_err(|e| format!("{what}: not base64 ({e})"))?;
    raw.try_into().map_err(|_| format!("{what}: not 32 bytes"))
}

/// The signature (base64) of `body` with the base64 seed `seed`.
pub fn sign(seed: &str, body: &[u8]) -> Result<String, String> {
    let key = SigningKey::from_bytes(&bytes32(seed, "signing key")?);
    Ok(B64.encode(key.sign(body).to_bytes()))
}

pub fn public_of(seed: &str) -> Result<String, String> {
    let key = SigningKey::from_bytes(&bytes32(seed, "signing key")?);
    Ok(B64.encode(key.verifying_key().to_bytes()))
}

fn trusted() -> Vec<String> {
    let mut keys: Vec<String> = RELEASE_KEYS.iter().map(|k| k.to_string()).collect();
    if let Some(extra) = crate::paths::env_nonempty(EXTRA_KEY_ENV) {
        keys.extend(extra.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()));
    }
    keys
}

/// Ok when a trusted key signed `body`.
pub fn verify(body: &[u8], sig: &str) -> Result<(), String> {
    verify_with(body, sig, &trusted())
}

fn verify_with(body: &[u8], sig: &str, keys: &[String]) -> Result<(), String> {
    let raw = B64.decode(sig.trim()).map_err(|_| "manifest.json.sig is not base64".to_string())?;
    let raw: [u8; 64] = raw.try_into().map_err(|_| "manifest.json.sig is not an Ed25519 signature".to_string())?;
    let sig = Signature::from_bytes(&raw);
    for k in keys {
        let Ok(b) = bytes32(k, "release key") else { continue };
        let Ok(vk) = VerifyingKey::from_bytes(&b) else { continue };
        if vk.verify(body, &sig).is_ok() {
            return Ok(());
        }
    }
    Err("manifest.json is not signed by a trusted release key - nothing installed".into())
}

/// `imperatorium release-keygen`: a new key pair, as JSON. The secret goes
/// in the release workflow's IMPERATORIUM_SIGNING_KEY secret (and an
/// offline backup), the public key in RELEASE_KEYS.
pub fn keygen_main() -> i32 {
    match keygen() {
        Ok((secret, public)) => {
            println!("{}", serde_json::json!({"secret": secret, "public": public}));
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_verifies() {
        let (secret, public) = keygen().unwrap();
        assert_eq!(public_of(&secret).unwrap(), public);
        let body = b"{\"version\": \"0.4.0\"}\n";
        let sig = sign(&secret, body).unwrap();
        assert!(verify_with(body, &sig, std::slice::from_ref(&public)).is_ok());
        // Another body, another key, or a mangled signature: refused.
        assert!(verify_with(b"{\"version\": \"9.9.9\"}\n", &sig, std::slice::from_ref(&public)).is_err());
        let (_, other) = keygen().unwrap();
        assert!(verify_with(body, &sig, std::slice::from_ref(&other)).is_err());
        assert!(verify_with(body, &sig, &[other, public.clone()]).is_ok());
        assert!(verify_with(body, "not base64!", std::slice::from_ref(&public)).is_err());
        assert!(verify_with(body, &B64.encode([0u8; 64]), &[public]).is_err());
    }

    #[test]
    fn embedded_keys_are_keys() {
        for k in RELEASE_KEYS {
            let b = bytes32(k, "release key").unwrap();
            assert!(VerifyingKey::from_bytes(&b).is_ok(), "{k}");
        }
    }
}
