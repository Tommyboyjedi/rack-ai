use std::path::Path;
use std::process::Command;

pub struct GitCommand;

impl GitCommand {
    pub fn run(repo: &Path, args: &[&str]) -> Result<String, String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("git {} failed", args.join(" "))
            } else {
                stderr
            });
        }
        String::from_utf8(output.stdout)
            .map_err(|error| error.to_string())
            .map(|text| text.trim_end().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::GitCommand;
    use std::path::PathBuf;

    #[test]
    fn fails_in_missing_repository() {
        let error = GitCommand::run(
            PathBuf::from("/tmp/missing-rack-ai-git").as_path(),
            &["status"],
        )
        .unwrap_err();
        assert!(!error.is_empty());
    }
}
