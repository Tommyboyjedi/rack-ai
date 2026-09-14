use crate::{command::run, config::Config};
pub fn verify(config: &Config) -> Result<(), String> {
    let raw = run(
        "nvidia-smi",
        &["--query-gpu=uuid,name", "--format=csv,noheader,nounits"],
    )?;
    let rows: Vec<_> = raw.lines().filter_map(|l| l.split_once(',')).collect();
    if config.media_uuid.len() != 40
        || rows
            .iter()
            .filter(|(uuid, name)| uuid.trim() == config.media_uuid && name.contains("4080 SUPER"))
            .count()
            != 1
    {
        return Err("shared media UUID is not a unique 4080 SUPER".into());
    }
    Ok(())
}
