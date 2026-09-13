//! Local administrative recovery: no HTTP endpoint, passwords or API-token mutation.
use rack_ai_media::{config::Config, human_store::HumanStore};
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[1] != "reset-browser-password" {
        return Err("usage: rack_ai_media_admin CONFIG reset-browser-password".into());
    }
    let config = Config::load(args[0].clone().into())?;
    HumanStore::new(&config)?.reset()?;
    println!(
        "Browser password reset. Existing browser cookies are revoked. Use the existing operator credential once to set a new password. API credentials are unchanged."
    );
    Ok(())
}
