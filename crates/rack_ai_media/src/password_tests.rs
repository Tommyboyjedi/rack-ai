use crate::{human_store::HumanRecord, password_kdf};
#[test]
fn argon2id_random_salt_and_exact_password_verification() {
    let password = "a synthetic unicode passphrase café";
    let first = password_kdf::hash(password).unwrap();
    let second = password_kdf::hash(password).unwrap();
    assert_ne!(first, second);
    assert!(first.starts_with("$argon2id$v=19$m=65536,t=3,p=1$"));
    assert!(!first.contains(password));
    assert!(password_kdf::verify(password, &first).unwrap());
    assert!(!password_kdf::verify("wrong synthetic phrase", &first).unwrap());
    assert!(password_kdf::validate_hash(&first.replace("m=65536", "m=4294967295")).is_err());
    assert!(password_kdf::validate_hash(&first.replace("argon2id", "argon2i")).is_err());
    assert!(password_kdf::validate_new("short").is_err());
    assert!(password_kdf::validate_new(&"a".repeat(1025)).is_err());
    assert!(password_kdf::validate_new(&"a".repeat(1024)).is_ok());
}
#[test]
fn human_record_does_not_accept_unknown_or_partial_state() {
    assert!(serde_json::from_str::<HumanRecord>(r#"{"password_hash":null,"extra":true}"#).is_err());
}
#[test]
fn private_atomic_write_starts_restrictive_and_keeps_prior_file_on_failure() {
    use rack_ai_application::durable_file::atomic_write_private;
    use std::{fs, os::unix::fs::PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.json");
    atomic_write_private(&path, "synthetic old record").unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::write(
        dir.path().join(".auth.interrupted.tmp"),
        "synthetic partial",
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "synthetic old record");
    atomic_write_private(&path, "synthetic new record").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "synthetic new record");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(atomic_write_private(&dir.path().join("missing/auth.json"), "failed").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "synthetic new record");
}
