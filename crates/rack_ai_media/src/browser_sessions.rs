use crate::{
    config::Principal,
    human_store::HumanRecord,
    store::Store,
    types::{BrowserSession, digest, identity, now},
};
use axum::http::{HeaderMap, header};
pub fn cookie_digest(headers: &HeaderMap) -> Result<String, String> {
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let value = cookie
        .split(';')
        .filter_map(|c| c.trim().split_once('='))
        .find(|(k, _)| *k == "rack_session")
        .map(|(_, v)| v)
        .ok_or("unauthorized")?;
    Ok(digest(value.as_bytes()))
}
pub fn principal(app: &crate::web_state::WebState, hash: &str) -> Result<Principal, String> {
    use subtle::ConstantTimeEq;
    let record = app.human.read()?;
    let state = app.store.read()?;
    let session = state
        .browsers
        .iter()
        .find(|s| {
            s.expires > now()
                && s.auth_generation == record.generation
                && bool::from(s.digest.as_bytes().ct_eq(hash.as_bytes()))
        })
        .ok_or("unauthorized")?;
    app.config
        .principals
        .iter()
        .find(|p| {
            p.id == session.owner
                && p.operator
                && record.owner.as_ref().is_none_or(|owner| owner == &p.id)
        })
        .cloned()
        .ok_or("unauthorized".into())
}
pub fn cookie(value: &str, secure: bool) -> String {
    format!(
        "rack_session={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
        crate::limits::COOKIE_SECONDS,
        if secure { "; Secure" } else { "" }
    )
}
pub fn expired_cookie(secure: bool) -> String {
    format!(
        "rack_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
        if secure { "; Secure" } else { "" }
    )
}
pub struct SessionIssue<'a> {
    pub principal: &'a Principal,
    pub record: &'a HumanRecord,
    pub revoke_others: bool,
}
pub fn issue(store: &Store, input: SessionIssue<'_>) -> Result<String, String> {
    let token = identity() + &identity();
    let now = now();
    store.update(|s| {
        if input.revoke_others {
            s.browsers.clear();
        }
        s.browsers
            .retain(|b| b.expires > now && b.auth_generation == input.record.generation);
        if s.browsers.len() >= crate::limits::BROWSER_SESSIONS {
            return Err("Browser session capacity reached".into());
        }
        s.browsers.push(BrowserSession {
            digest: digest(token.as_bytes()),
            owner: input.principal.id.clone(),
            expires: now + crate::limits::COOKIE_SECONDS,
            auth_generation: input.record.generation.clone(),
        });
        Ok(())
    })?;
    Ok(token)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secure_cookie_has_exact_thirty_day_persistence() {
        let cookie = cookie("synthetic-cookie", true);
        for value in [
            "Max-Age=2592000",
            "; Secure",
            "HttpOnly",
            "SameSite=Strict",
            "Path=/",
        ] {
            assert!(cookie.contains(value));
        }
        assert!(expired_cookie(true).contains("Max-Age=0"));
    }
}
