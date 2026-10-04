//! argon2id hash and verify. Host adapter.

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};

/// Hash a password as argon2id (`$argon2id$v=19$…`).
///
/// # Errors
///
/// Returns a string when the password is empty.
pub fn hash_password(password: &str) -> Result<String, String> {
    if password.is_empty() {
        return Err("password is empty".to_owned());
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("argon2id hash");
    Ok(hash.to_string())
}

/// Verify `password` against a stored argon2id hash.
#[must_use]
pub fn verify_password(password: &str, encoded: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(encoded) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// True when `encoded` is an argon2id v19 PHC string.
#[must_use]
pub fn is_argon2id_hash(encoded: &str) -> bool {
    encoded.starts_with("$argon2id$v=19$") && PasswordHash::new(encoded).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let h = hash_password("hunter2-test").expect("hash");
        assert!(is_argon2id_hash(&h));
        assert!(verify_password("hunter2-test", &h));
        assert!(!verify_password("nope", &h));
        assert!(!verify_password("hunter2-test", "not-a-hash"));
        assert_eq!(hash_password("").unwrap_err(), "password is empty");
        assert!(!is_argon2id_hash("$argon2id$v=19$"));
        assert!(!is_argon2id_hash("broken"));
        assert!(!is_argon2id_hash(
            "$argon2i$v=19$m=8,t=1,p=1$YWFhYWFhYWE$YWFh"
        ));
    }
}
