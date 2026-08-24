use std::fs;
use std::path::{Path, PathBuf};

use rack_ai_application::ImplementWorkerRuntime;

#[derive(Debug)]
pub struct JCodeExecutionConfig {
    home_dir: PathBuf,
}

impl JCodeExecutionConfig {
    pub fn prepare_at(root: &Path, runtime: &ImplementWorkerRuntime) -> Result<Self, String> {
        if runtime.endpoint().trim().is_empty() {
            return Err(format!(
                "worker {} is missing an endpoint binding",
                runtime.worker_id()
            ));
        }
        if runtime.api_model_id().trim().is_empty() {
            return Err(format!(
                "worker {} is missing an api_model_id binding",
                runtime.worker_id()
            ));
        }
        if runtime.tool_profile() == Some("minimal") && runtime.context_window().is_none() {
            return Err(format!(
                "worker {} requires context_window for minimal JCode execution",
                runtime.worker_id()
            ));
        }
        let home_dir = root.join("home");
        let jcode_dir = home_dir.join(".jcode");
        fs::create_dir_all(&jcode_dir).map_err(|error| error.to_string())?;
        fs::write(jcode_dir.join("config.toml"), render(runtime)).map_err(|error| error.to_string())?;
        Ok(Self { home_dir })
    }

    pub fn home_dir(&self) -> &Path {
        self.home_dir.as_path()
    }
}

fn render(runtime: &ImplementWorkerRuntime) -> String {
    let profile = runtime.provider_profile();
    let model = runtime.api_model_id();
    let mut text = format!(
        concat!(
            "[provider]\n",
            "default_provider = \"{profile}\"\n",
            "default_model = \"{model}\"\n\n",
            "[providers.\"{profile}\"]\n",
            "type = \"open-ai-compatible\"\n",
            "base_url = \"{endpoint}\"\n",
            "auth = \"none\"\n",
            "default_model = \"{model}\"\n",
            "requires_api_key = false\n",
            "provider_routing = false\n",
            "model_catalog = false\n",
            "allow_provider_pinning = false\n\n",
            "[[providers.\"{profile}\".models]]\n",
            "id = \"{model}\"\n"
        ),
        profile = profile,
        model = model,
        endpoint = runtime.endpoint(),
    );
    if let Some(context_window) = runtime.context_window() {
        text.push_str(format!("context_window = {context_window}\n").as_str());
    }
    text
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rack_ai_application::ImplementWorkerRuntime;

    use super::JCodeExecutionConfig;

    #[test]
    fn writes_authoritative_endpoint_model_and_context_window() {
        let root = temp_root();
        let runtime = ImplementWorkerRuntime::new(
            "local-coder".to_string(),
            "/home/tomp/.local/bin/jcode".to_string(),
            "local-coder".to_string(),
            "local-coder".to_string(),
            "http://127.0.0.1:8018/v1".to_string(),
        )
        .with_tool_profile(Some("minimal".to_string()))
        .with_context_window(Some(16368));

        let config = JCodeExecutionConfig::prepare_at(&root, &runtime).unwrap();
        let text = fs::read_to_string(config.home_dir().join(".jcode/config.toml")).unwrap();

        assert!(text.contains("default_provider = \"local-coder\""));
        assert!(text.contains("base_url = \"http://127.0.0.1:8018/v1\""));
        assert!(text.contains("default_model = \"local-coder\""));
        assert!(text.contains("id = \"local-coder\""));
        assert!(text.contains("context_window = 16368"));
    }

    #[test]
    fn rejects_minimal_worker_without_context_window() {
        let root = temp_root();
        let runtime = ImplementWorkerRuntime::new(
            "local-coder".to_string(),
            "/home/tomp/.local/bin/jcode".to_string(),
            "local-coder".to_string(),
            "local-coder".to_string(),
            "http://127.0.0.1:8018/v1".to_string(),
        )
        .with_tool_profile(Some("minimal".to_string()));

        let error = JCodeExecutionConfig::prepare_at(&root, &runtime).unwrap_err();

        assert!(error.contains("context_window"));
        assert!(error.contains("local-coder"));
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-jcode-config-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
