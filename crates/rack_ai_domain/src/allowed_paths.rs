use serde::Deserialize;
use serde::Serialize;

use crate::AllowedPath;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AllowedPaths(Vec<AllowedPath>);

impl AllowedPaths {
    pub fn new(paths: Vec<AllowedPath>) -> Result<Self, String> {
        if paths.is_empty() {
            return Err("allowed paths cannot be empty".to_string());
        }
        Ok(Self(paths))
    }

    pub fn values(&self) -> &[AllowedPath] {
        self.0.as_slice()
    }

    pub fn allows(&self, changed_path: &str) -> bool {
        self.0.iter().any(|path| path.allows(changed_path))
    }

    pub fn reject_disallowed<'a>(&self, changed_paths: &'a [String]) -> Vec<&'a String> {
        changed_paths
            .iter()
            .filter(|path| !self.allows(path))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::AllowedPaths;
    use crate::AllowedPath;

    #[test]
    fn rejects_empty_list() {
        assert_eq!(
            AllowedPaths::new(vec![]),
            Err("allowed paths cannot be empty".to_string())
        );
    }

    #[test]
    fn reports_disallowed_changed_paths() {
        let allowed = AllowedPaths::new(vec![
            AllowedPath::new("src".to_string()).unwrap(),
            AllowedPath::new("Cargo.toml".to_string()).unwrap(),
        ])
        .unwrap();
        let changed = vec![
            "src/lib.rs".to_string(),
            "README.md".to_string(),
            "Cargo.toml".to_string(),
        ];
        let rejected = allowed.reject_disallowed(&changed);
        assert_eq!(rejected, vec![&"README.md".to_string()]);
    }
}
