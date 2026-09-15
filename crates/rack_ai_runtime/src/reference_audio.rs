//! Same PCM reference contract as the existing Chatterbox registry.
pub const MAX_BYTES: usize = 6 * 1024 * 1024;
pub fn validate(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_BYTES {
        return Err("voice_file_too_large".into());
    }
    if bytes.len() < 44
        || &bytes[..4] != b"RIFF"
        || &bytes[8..12] != b"WAVE"
        || u32le(&bytes[4..8]) as usize + 8 != bytes.len()
    {
        return Err("invalid_reference_wav".into());
    }
    let mut offset = 12usize;
    let mut rate = None;
    let mut samples = None;
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset + 8)
            .ok_or("invalid_reference_wav")?;
        let size = u32le(&header[4..8]) as usize;
        let start = offset + 8;
        let end = start.checked_add(size).ok_or("invalid_reference_wav")?;
        let chunk = bytes.get(start..end).ok_or("invalid_reference_wav")?;
        match &header[..4] {
            b"fmt " => {
                if rate.is_some()
                    || size < 16
                    || chunk[..4] != [1, 0, 1, 0]
                    || chunk[12..16] != [2, 0, 16, 0]
                {
                    return Err("invalid_reference_wav".into());
                }
                let sample_rate = u32le(&chunk[4..8]);
                if ![16000, 22050, 24000, 44100, 48000].contains(&sample_rate)
                    || u32le(&chunk[8..12]) != sample_rate * 2
                {
                    return Err("unsupported_reference_format".into());
                }
                rate = Some(sample_rate as usize);
            }
            b"data" => {
                if rate.is_none() || samples.is_some() || !size.is_multiple_of(2) {
                    return Err("invalid_reference_wav".into());
                }
                samples = Some(size / 2);
            }
            _ => {}
        }
        offset = end.checked_add(size % 2).ok_or("invalid_reference_wav")?;
        if offset > bytes.len() {
            return Err("invalid_reference_wav".into());
        }
    }
    let rate = rate.ok_or("invalid_reference_wav")?;
    let samples = samples.ok_or("invalid_reference_wav")?;
    if samples <= rate * 5 || samples > rate * 30 {
        return Err("reference_duration_bounds".into());
    }
    Ok(())
}
fn u32le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
