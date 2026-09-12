use crate::{
    config::Config,
    types::{Artifact, Job, digest, identity},
};
use serde::Deserialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path},
};
pub const MAX_ARTIFACT: u64 = 16 * 1024 * 1024;
#[derive(Deserialize)]
struct OutputImage {
    filename: String,
    subfolder: String,
    #[serde(rename = "type")]
    kind: String,
}
pub fn collect(
    config: &Config,
    job: &Job,
    terminal: &serde_json::Value,
) -> Result<Vec<Artifact>, String> {
    let prompt = terminal
        .get("prompt")
        .and_then(|p| p.as_array())
        .ok_or("history lacks exact prompt")?;
    if prompt.get(1).and_then(|p| p.as_str()) != Some(&job.prompt_id)
        || prompt.get(2) != Some(&job.workflow)
    {
        return Err("history prompt identity/workflow mismatch".into());
    }
    let status = terminal.get("status").ok_or("history lacks status")?;
    if status.get("status_str").and_then(|s| s.as_str()) != Some("success")
        || status.get("completed").and_then(|s| s.as_bool()) != Some(true)
    {
        return Err("render did not succeed".into());
    }
    let images = terminal
        .pointer("/outputs/9/images")
        .and_then(|v| v.as_array())
        .ok_or("expected output node missing")?;
    if images.len() != 1 {
        return Err("expected exactly one image".into());
    }
    let output: OutputImage =
        serde_json::from_value(images[0].clone()).map_err(|_| "invalid image record")?;
    if output.kind != "output"
        || output.subfolder != format!("rack/{}", job.id)
        || Path::new(&output.filename).components().count() != 1
        || !matches!(
            Path::new(&output.filename).components().next(),
            Some(Component::Normal(_))
        )
    {
        return Err("artifact namespace violation".into());
    }
    let root = config
        .output_root
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let source = root.join(&output.subfolder).join(&output.filename);
    let canonical = source.canonicalize().map_err(|_| "artifact missing")?;
    if !canonical.starts_with(root.join(&output.subfolder)) {
        return Err("artifact symlink escape".into());
    }
    for path in [&source, &root.join(&output.subfolder), &root.join("rack")] {
        if fs::symlink_metadata(path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("artifact symlink rejected".into());
        }
    }
    let data = read_bounded(&canonical)?;
    let (width, height) = validate_png(&data)?;
    if width != job.request.parameters.width || height != job.request.parameters.height {
        return Err("artifact dimensions mismatch".into());
    }
    let artifact = Artifact {
        id: identity(),
        filename: "image.png".into(),
        sha256: digest(&data),
        bytes: data.len() as u64,
        content_type: "image/png".into(),
        width,
        height,
    };
    let destination = config.state_root.join("artifacts").join(&job.id);
    fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
    let path = destination.join(format!("{}.png", artifact.id));
    let temp = destination.join(format!(".{}.tmp", artifact.id));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(&data)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(temp, path).map_err(|e| e.to_string())?;
    File::open(destination)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(vec![artifact])
}
pub fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_ARTIFACT {
        return Err("invalid artifact file".into());
    }
    let mut bytes = vec![];
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_ARTIFACT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_ARTIFACT {
        return Err("oversized artifact".into());
    }
    Ok(bytes)
}
pub fn validate_png(data: &[u8]) -> Result<(u32, u32), String> {
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(data), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(crate::limits::IMAGE_MAX_EDGE);
    limits.max_image_height = Some(crate::limits::IMAGE_MAX_EDGE);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| "invalid PNG content")?;
    Ok((image.width(), image.height()))
}
