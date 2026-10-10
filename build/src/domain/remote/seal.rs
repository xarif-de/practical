//! Sealing a Nextcloud app password at rest.
//!
//! Why at all: Nextcloud has no calendar-only scope, so the app password a sync link must keep
//! reaches the account's FILES as well (measured: `PROPFIND /remote.php/dav/files/<user>/` answers
//! 207 with it). A copy of PractiCal's SQLite file alone must therefore not be enough to use it.
//!
//! ChaCha20-Poly1305 with a fresh random 96-bit nonce per encryption, stored in front of the
//! ciphertext. The AAD binds the row to its account and user, so a sealed value cannot be moved to
//! another account — a database write by someone who cannot read the key still cannot redirect a
//! credential.

use crate::error::{AppError, AppResult};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::ChaCha20Poly1305;

const NONCE: usize = 12;

fn cipher(key: &[u8]) -> AppResult<ChaCha20Poly1305> {
    ChaCha20Poly1305::new_from_slice(key).map_err(|_| AppError::Internal("remote secret key size".into()))
}

fn aad(account_id: &str, principal: &str) -> Vec<u8> {
    let mut v = account_id.as_bytes().to_vec();
    v.push(0);
    v.extend_from_slice(principal.as_bytes());
    v
}

pub fn seal(key: &[u8], account_id: &str, principal: &str, secret: &str) -> AppResult<Vec<u8>> {
    use rand::RngExt;
    let mut nonce = [0u8; NONCE];
    rand::rng().fill(&mut nonce[..]);
    let ct = cipher(key)?
        .encrypt((&nonce).into(), Payload { msg: secret.as_bytes(), aad: &aad(account_id, principal) })
        .map_err(|_| AppError::Internal("sealing the remote secret failed".into()))?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn open(key: &[u8], account_id: &str, principal: &str, sealed: &[u8]) -> AppResult<super::dav::Secret> {
    if sealed.len() <= NONCE {
        return Err(AppError::Internal("sealed remote secret too short".into()));
    }
    let (head, ct) = sealed.split_at(NONCE);
    let nonce: [u8; NONCE] = head.try_into().map_err(|_| AppError::Internal("sealed remote secret nonce".into()))?;
    let pt = cipher(key)?
        .decrypt((&nonce).into(), Payload { msg: ct, aad: &aad(account_id, principal) })
        .map_err(|_| AppError::Internal("the remote secret does not open with this key".into()))?;
    Ok(super::dav::Secret(String::from_utf8(pt).map_err(|_| AppError::Internal("sealed remote secret is not text".into()))?))
}

