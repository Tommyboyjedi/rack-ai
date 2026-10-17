use crate::config::Profile;
use rack_ai_media::command::{CommandSpec, run_bounded};
pub fn verify(profile: &Profile) -> Result<(), String> {
    let Some(launch) = &profile.launch else {
        return Ok(());
    };
    if launch.file_pins.is_empty() {
        return Ok(());
    }
    let names = launch
        .file_pins
        .iter()
        .map(|f| f.path.to_str().ok_or("invalid_launch_pin_path"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut args = vec!["--"];
    args.extend(names);
    let result = run_bounded(CommandSpec {
        program: "sha256sum",
        args: &args,
        seconds: profile.artifact_verify_seconds,
    })
    .map_err(|_| "launch_integrity_unavailable")?;
    let hashes = result
        .lines()
        .map(|line| line.split_whitespace().next().unwrap_or(""))
        .collect::<Vec<_>>();
    if hashes.len() != launch.file_pins.len()
        || hashes
            .iter()
            .zip(&launch.file_pins)
            .any(|(actual, expected)| *actual != expected.sha256)
    {
        return Err("launch_file_hash_mismatch".into());
    }
    Ok(())
}
