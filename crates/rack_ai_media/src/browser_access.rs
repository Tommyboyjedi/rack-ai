use crate::{
    auth,
    browser_sessions::{self, SessionIssue},
    config::Principal,
    human_store::HumanRecord,
    password_kdf,
    types::{identity, now},
    web_state::WebState,
};
use subtle::ConstantTimeEq;
#[derive(Debug)]
pub enum BrowserError {
    Invalid,
    Input(String),
    Unavailable,
}
pub struct Verifier<'a> {
    pub app: &'a WebState,
    pub record: &'a HumanRecord,
}
impl Verifier<'_> {
    pub fn verify(&self, password: &str) -> Result<Principal, BrowserError> {
        if let Some(encoded) = &self.record.password_hash {
            if !password_kdf::verify(password, encoded).map_err(|_| BrowserError::Unavailable)? {
                return Err(BrowserError::Invalid);
            }
            self.app
                .config
                .principals
                .iter()
                .find(|p| p.operator && Some(&p.id) == self.record.owner.as_ref())
                .cloned()
                .ok_or(BrowserError::Unavailable)
        } else {
            auth::token_principal(self.app, password)
                .ok()
                .filter(|p| p.operator)
                .ok_or(BrowserError::Invalid)
        }
    }
}
pub fn login(app: &WebState, password: &str) -> Result<String, BrowserError> {
    let _lock = app.human.lock().map_err(|_| BrowserError::Unavailable)?;
    let record = app.human.read().map_err(|_| BrowserError::Unavailable)?;
    let principal = Verifier {
        app,
        record: &record,
    }
    .verify(password)?;
    browser_sessions::issue(
        &app.store,
        SessionIssue {
            principal: &principal,
            record: &record,
            revoke_others: false,
        },
    )
    .map_err(|_| BrowserError::Unavailable)
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordChange {
    pub current_password: String,
    pub new_password: String,
    pub confirm_password: String,
}
pub struct ChangeRequest {
    pub form: PasswordChange,
    pub session_digest: String,
}
pub fn change(app: &WebState, input: ChangeRequest) -> Result<String, BrowserError> {
    let _lock = app.human.lock().map_err(|_| BrowserError::Unavailable)?;
    let record = app.human.read().map_err(|_| BrowserError::Unavailable)?;
    let state = app.store.read().map_err(|_| BrowserError::Unavailable)?;
    let session = state
        .browsers
        .iter()
        .find(|b| {
            b.expires > now()
                && b.auth_generation == record.generation
                && bool::from(b.digest.as_bytes().ct_eq(input.session_digest.as_bytes()))
        })
        .ok_or(BrowserError::Invalid)?;
    let principal = Verifier {
        app,
        record: &record,
    }
    .verify(&input.form.current_password)?;
    if principal.id != session.owner {
        return Err(BrowserError::Invalid);
    }
    if input.form.new_password != input.form.confirm_password {
        return Err(BrowserError::Input(
            "The new passwords do not match.".into(),
        ));
    }
    if auth::token_principal(app, &input.form.new_password).is_ok() {
        return Err(BrowserError::Input(
            "Choose a new personal password.".into(),
        ));
    }
    password_kdf::validate_new(&input.form.new_password).map_err(BrowserError::Input)?;
    let next = HumanRecord {
        generation: identity(),
        owner: Some(principal.id.clone()),
        password_hash: Some(
            password_kdf::hash(&input.form.new_password).map_err(|_| BrowserError::Unavailable)?,
        ),
    };
    // Commit revocation first: interrupted session persistence cannot revive old cookies.
    app.human
        .write(&next)
        .map_err(|_| BrowserError::Unavailable)?;
    browser_sessions::issue(
        &app.store,
        SessionIssue {
            principal: &principal,
            record: &next,
            revoke_others: true,
        },
    )
    .map_err(|_| BrowserError::Unavailable)
}
