pub struct ChangedPaths;

impl ChangedPaths {
    pub fn from_porcelain(status: &str) -> Vec<String> {
        let mut paths = Vec::new();
        for line in status.lines() {
            if line.len() < 4 {
                continue;
            }
            let path_part = line[3..].trim();
            let path = path_part
                .rsplit_once(" -> ")
                .map(|(_, dest)| dest)
                .unwrap_or(path_part)
                .trim_matches('"');
            if !path.is_empty() && !paths.iter().any(|existing| existing == path) {
                paths.push(path.to_string());
            }
        }
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::ChangedPaths;

    #[test]
    fn parses_modified_untracked_and_renamed_paths() {
        let status = " M src/lib.rs\n?? tests/new.rs\nR  old.rs -> src/renamed.rs\n";
        let paths = ChangedPaths::from_porcelain(status);
        assert_eq!(paths, ["src/lib.rs", "tests/new.rs", "src/renamed.rs"]);
    }
}
