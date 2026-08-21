use std::path::Component;
use std::path::Path;

use rack_ai_domain::AllowedPath;
use rack_ai_domain::AllowedPaths;

use crate::WorkspacePath;

pub fn parse_campaign_path(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        return Err("campaign path cannot be empty".to_string());
    }

    if raw.contains('\0') {
        return Err(format!("invalid campaign path '{raw}': contains NUL"));
    }

    if Path::new(trimmed).is_absolute() {
        return Err(format!(
            "invalid campaign path '{raw}': absolute paths are not allowed"
        ));
    }

    for component in Path::new(trimmed).components() {
        match component {
            Component::CurDir | Component::ParentDir => {
                return Err(format!(
                    "invalid campaign path '{raw}': traversal components are not allowed"
                ));
            }
            _ => {}
        }
    }

    let parsed = WorkspacePath::parse(trimmed)
        .map_err(|error| format!("invalid campaign path '{raw}': {error}"))?;

    let allowed = AllowedPath::new(parsed.relative().to_string())?;
    Ok(allowed.value().to_string())
}

pub fn parse_campaign_paths(values: &[String]) -> Result<Vec<String>, String> {
    values.iter().map(|value| parse_campaign_path(value)).collect()
}

pub fn allowed_paths_from(values: &[String]) -> Result<AllowedPaths, String> {
    let parsed = values
        .iter()
        .map(|value| parse_campaign_path(value).and_then(AllowedPath::new))
        .collect::<Result<Vec<_>, _>>()?;
    AllowedPaths::new(parsed)
}

pub fn path_is_authorized(path: &str, allowlist: &[String]) -> Result<bool, String> {
    let changed = parse_campaign_path(path)?;
    let allowed = allowed_paths_from(allowlist)?;
    Ok(allowed.allows(&changed))
}

pub fn path_is_under_prefix(path: &str, prefix: &str) -> Result<bool, String> {
    let changed = parse_campaign_path(path)?;
    let allowed = AllowedPath::new(parse_campaign_path(prefix)?)?;
    Ok(allowed.allows(&changed))
}

pub fn assert_authorized_paths(changed: &[String], allowlist: &[String]) -> Result<(), String> {
    let allowed = allowed_paths_from(allowlist)?;
    let normalized = changed
        .iter()
        .map(|path| parse_campaign_path(path))
        .collect::<Result<Vec<_>, _>>()?;
    let rejected = allowed.reject_disallowed(&normalized);
    if rejected.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "changed paths outside allowed_paths: {}",
            rejected
                .iter()
                .map(|path| path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

pub fn required_prefix_satisfied(changed: &[String], required: &str) -> Result<bool, String> {
    let prefix = AllowedPath::new(parse_campaign_path(required)?)?;
    for path in changed {
        let normalized = parse_campaign_path(path)?;
        if prefix.allows(&normalized) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::assert_authorized_paths;
    use super::parse_campaign_path;
    use super::path_is_authorized;
    use super::path_is_under_prefix;
    use super::required_prefix_satisfied;

    #[test]
    fn rejects_absolute_traversal_and_malformed_paths() {
        assert!(parse_campaign_path("/etc/passwd").is_err());
        assert!(parse_campaign_path("../secret").is_err());
        assert!(parse_campaign_path("src/../secret").is_err());
        assert!(parse_campaign_path("").is_err());
        assert!(parse_campaign_path("foo\0bar").is_err());
    }

    #[test]
    fn authorization_does_not_use_raw_prefix_matching() {
        assert!(!path_is_authorized("srcfoo/lib.rs", &["src".to_string()]).unwrap());
        assert!(path_is_authorized("src/lib.rs", &["src".to_string()]).unwrap());
        assert!(path_is_authorized("src/lib.rs", &["src/".to_string()]).unwrap());
        assert!(!path_is_under_prefix("srcfoo", "src").unwrap());
        assert!(path_is_under_prefix("src/domain/mod.rs", "src/domain").unwrap());
    }

    #[test]
    fn required_prefix_uses_boundary_aware_matching() {
        let changed = vec!["src/lib.rs".to_string()];
        assert!(required_prefix_satisfied(&changed, "src").unwrap());
        assert!(!required_prefix_satisfied(&changed, "src/domain").unwrap());
        assert!(!required_prefix_satisfied(&["srcfoo.rs".to_string()], "src").unwrap());
    }

    #[test]
    fn rejects_out_of_policy_changed_paths() {
        let error = assert_authorized_paths(
            &["src/lib.rs".to_string(), "README.md".to_string()],
            &["src/".to_string()],
        )
        .unwrap_err();
        assert!(error.contains("README.md"));
    }
}
