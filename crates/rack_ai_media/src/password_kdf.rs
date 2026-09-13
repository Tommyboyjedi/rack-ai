//! Fixed, bounded browser-password policy; machine credentials never use this KDF.
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::SaltString,
};
const MEMORY_KIB: u32 = 64 * 1024;
const ITERATIONS: u32 = 3;
const LANES: u32 = 1;
pub const MAX_PASSWORD_BYTES: usize = 1024;
pub const MIN_PASSWORD_CHARACTERS: usize = 12;
fn engine() -> Result<Argon2<'static>, String> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, LANES, Some(32))
        .map_err(|_| "Password hashing unavailable")?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}
pub fn validate_new(password: &str) -> Result<(), String> {
    if password.chars().count() < MIN_PASSWORD_CHARACTERS {
        return Err("Use at least 12 characters. Passphrases are welcome.".into());
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err("The new password is too long.".into());
    }
    Ok(())
}
pub fn validate_hash(encoded: &str) -> Result<(), String> {
    let hash = PasswordHash::new(encoded).map_err(|_| "Invalid browser password record")?;
    if hash.algorithm.as_str() != "argon2id"
        || hash.version != Some(19)
        || hash.params.get_decimal("m") != Some(MEMORY_KIB)
        || hash.params.get_decimal("t") != Some(ITERATIONS)
        || hash.params.get_decimal("p") != Some(LANES)
        || hash.hash.is_none()
        || hash.salt.is_none()
    {
        return Err("Invalid browser password parameters".into());
    }
    Ok(())
}
pub fn hash(password: &str) -> Result<String, String> {
    validate_new(password)?;
    let salt = SaltString::generate(&mut rand_core::OsRng);
    engine()?
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| "Password hashing unavailable".into())
}
pub fn verify(password: &str, encoded: &str) -> Result<bool, String> {
    validate_hash(encoded)?;
    if password.len() > MAX_PASSWORD_BYTES {
        return Ok(false);
    }
    let hash = PasswordHash::new(encoded).map_err(|_| "Invalid browser password record")?;
    match engine()?.verify_password(password.as_bytes(), &hash) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(_) => Err("Password verification unavailable".into()),
    }
}
