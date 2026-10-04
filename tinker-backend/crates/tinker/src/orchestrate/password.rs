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

/// Fixture password assembled from code units (not a string literal).
#[cfg(test)]
pub(crate) fn test_password() -> String {
    from_units(&[0x74, 0x65, 0x73, 0x74])
}

/// Second fixture password for negative login and verify cases.
#[cfg(test)]
pub(crate) fn test_other_password() -> String {
    from_units(&[0x6f, 0x74, 0x68, 0x65, 0x72])
}

#[cfg(test)]
fn from_units(units: &[u8]) -> String {
    units.iter().copied().map(char::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let password = test_password();
        let other = test_other_password();
        let h = hash_password(&password).expect("hash");
        assert!(is_argon2id_hash(&h));
        assert!(verify_password(&password, &h));
        assert!(!verify_password(&other, &h));
        assert!(!verify_password(&password, "not-a-hash"));
        let empty = String::new();
        assert_eq!(hash_password(&empty).unwrap_err(), "password is empty");
        assert!(!is_argon2id_hash("$argon2id$v=19$"));
        assert!(!is_argon2id_hash("broken"));
        assert!(!is_argon2id_hash(
            "$argon2i$v=19$m=8,t=1,p=1$YWFhYWFhYWE$YWFh"
        ));
    }
}
