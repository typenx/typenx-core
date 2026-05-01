use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub fn random_url_token(bytes: usize) -> String {
    let mut buf = vec![0_u8; bytes];
    OsRng.fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn hash_token(secret: &str, token: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any size");
    mac.update(token.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

pub fn protect_token(secret: &str, plaintext: &str) -> String {
    let key = Sha256::digest(secret.as_bytes());
    let bytes = plaintext
        .as_bytes()
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % key.len()])
        .collect::<Vec<_>>();
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn unprotect_token(secret: &str, ciphertext: &str) -> Result<String, SecurityError> {
    let key = Sha256::digest(secret.as_bytes());
    let bytes = URL_SAFE_NO_PAD
        .decode(ciphertext)
        .map_err(|_| SecurityError::InvalidCiphertext)?
        .into_iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % key.len()])
        .collect::<Vec<_>>();
    String::from_utf8(bytes).map_err(|_| SecurityError::InvalidCiphertext)
}

#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("invalid protected token")]
    InvalidCiphertext,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_stable_and_protected_tokens_round_trip() {
        assert_eq!(hash_token("secret", "token"), hash_token("secret", "token"));
        let protected = protect_token("secret", "access-token");
        assert_ne!(protected, "access-token");
        assert_eq!(
            unprotect_token("secret", &protected).unwrap(),
            "access-token"
        );
    }
}
