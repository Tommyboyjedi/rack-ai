use crate::{
    config::Profile,
    types::{JobRequest, VERSION, digest},
};
use serde_json::{Value, json};
pub fn validate(request: &JobRequest, profile: &Profile) -> Result<(), String> {
    if request.schema != VERSION {
        return Err("validation: unsupported schema".into());
    }
    if request.profile != profile.id
        || request.profile_version != profile.version
        || !profile.available
    {
        return Err("unsupported: image profile unavailable".into());
    }
    for id in [
        &request.work_id,
        &request.submission_id,
        &request.idempotency_key,
    ] {
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err("validation: invalid identity".into());
        }
    }
    let p = &request.parameters;
    if p.prompt.trim().is_empty()
        || p.prompt.len() > 4096
        || p.negative_prompt.len() > 4096
        || p.seed > i64::MAX as u64
        || !(1..=50).contains(&p.steps)
        || [p.width, p.height]
            .iter()
            .any(|n| !(64..=1024).contains(n) || n % 64 != 0)
        || !(10..=900).contains(&request.timeout_seconds)
    {
        return Err("validation: parameters outside approved bounds".into());
    }
    Ok(())
}
pub fn workflow(request: &JobRequest, profile: &Profile, job: &str) -> Result<Value, String> {
    let p = &request.parameters;
    let name = profile
        .checkpoint
        .file_name()
        .and_then(|p| p.to_str())
        .ok_or("invalid checkpoint name")?;
    Ok(json!({
        "3":{"class_type":"KSampler","inputs":{"seed":p.seed,"steps":p.steps,"cfg":7.0,
            "sampler_name":"euler","scheduler":"normal","denoise":1.0,"model":["4",0],
            "positive":["6",0],"negative":["7",0],"latent_image":["5",0]}},
        "4":{"class_type":"CheckpointLoaderSimple","inputs":{"ckpt_name":name}},
        "5":{"class_type":"EmptyLatentImage","inputs":{"width":p.width,"height":p.height,"batch_size":1}},
        "6":{"class_type":"CLIPTextEncode","inputs":{"text":p.prompt,"clip":["4",1]}},
        "7":{"class_type":"CLIPTextEncode","inputs":{"text":p.negative_prompt,"clip":["4",1]}},
        "8":{"class_type":"VAEDecode","inputs":{"samples":["3",0],"vae":["4",2]}},
        "9":{"class_type":"SaveImage","inputs":{"filename_prefix":format!("rack/{job}/image"),"images":["8",0]}}
    }))
}
pub fn verify_checkpoint(profile: &Profile) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f =
        std::fs::File::open(&profile.checkpoint).map_err(|_| "approved checkpoint missing")?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = f.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if format!("{:x}", hash.finalize()) != profile.checkpoint_sha256 {
        return Err("checkpoint hash mismatch".into());
    }
    Ok(())
}
pub fn workflow_hash(value: &Value) -> Result<String, String> {
    Ok(digest(
        &serde_json::to_vec(value).map_err(|e| e.to_string())?,
    ))
}
