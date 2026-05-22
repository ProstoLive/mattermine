use anyhow::{anyhow, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("Failed to hash password: {e}"))?;
    Ok(hash.to_string())
}

#[allow(dead_code)]
pub fn verify_password(password: &str, hash_str: &str) -> Result<bool> {
    let parsed = PasswordHash::new(hash_str)
        .map_err(|e| anyhow!("Failed to parse password hash: {e}"))?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(e) => Err(anyhow!("Password verification error: {e}")),
    }
}
