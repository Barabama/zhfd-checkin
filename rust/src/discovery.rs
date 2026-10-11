use std::{collections::HashSet, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCandidate {
    pub path: PathBuf,
    pub source: &'static str,
    pub exists: bool,
}

#[derive(Debug, Clone)]
pub struct DiscoveryReport {
    pub tool: &'static str,
    pub candidates: Vec<ToolCandidate>,
    selected_index: Option<usize>,
}

impl DiscoveryReport {
    pub fn from_candidates(
        tool: &'static str,
        candidates: impl IntoIterator<Item = (PathBuf, &'static str)>,
    ) -> Self {
        let mut seen = HashSet::new();
        let candidates: Vec<ToolCandidate> = candidates
            .into_iter()
            .filter_map(|(path, source)| {
                if path.as_os_str().is_empty() {
                    return None;
                }
                let key = path.to_string_lossy().to_ascii_lowercase();
                if !seen.insert(key) {
                    return None;
                }
                Some(ToolCandidate {
                    exists: path.is_file(),
                    path,
                    source,
                })
            })
            .collect();
        let selected_index = candidates.iter().position(|candidate| candidate.exists);
        Self {
            tool,
            candidates,
            selected_index,
        }
    }

    pub fn selected(&self) -> Option<&ToolCandidate> {
        self.selected_index
            .and_then(|index| self.candidates.get(index))
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected().map(|candidate| candidate.path.clone())
    }

    pub fn summary(&self) -> String {
        if let Some(candidate) = self.selected() {
            return format!(
                "{}: {} (来源: {})",
                self.tool,
                candidate.path.display(),
                candidate.source
            );
        }
        if self.candidates.is_empty() {
            return format!("{}: 未发现候选路径", self.tool);
        }
        let checked = self
            .candidates
            .iter()
            .take(8)
            .map(|candidate| format!("{} [{}]", candidate.path.display(), candidate.source))
            .collect::<Vec<_>>()
            .join("; ");
        let more = self.candidates.len().saturating_sub(8);
        if more == 0 {
            format!("{}: 未找到。已检查: {}", self.tool, checked)
        } else {
            format!(
                "{}: 未找到。已检查: {}; 另有 {} 个候选",
                self.tool, checked, more
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "zhfd-discovery-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn selects_first_existing_candidate_and_preserves_source() {
        let root = temp_dir("priority");
        fs::create_dir_all(&root).unwrap();
        let missing = root.join("missing.exe");
        let found = root.join("tool.exe");
        fs::write(&found, b"fixture").unwrap();
        let report = DiscoveryReport::from_candidates(
            "tool",
            [(missing, "config"), (found.clone(), "sibling")],
        );
        assert_eq!(report.selected_path(), Some(found.clone()));
        assert_eq!(report.selected().unwrap().source, "sibling");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn empty_report_explains_that_no_candidates_were_found() {
        let report = DiscoveryReport::from_candidates("ADB", []);
        assert!(report.candidates.is_empty());
        assert!(report.selected().is_none());
        assert!(report.summary().contains("未发现候选路径"));
    }

    #[test]
    fn duplicate_paths_keep_the_first_source() {
        let root = temp_dir("deduplicate");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("tool.exe");
        fs::write(&executable, b"fixture").unwrap();
        let report = DiscoveryReport::from_candidates(
            "tool",
            [(executable.clone(), "config"), (executable.clone(), "path")],
        );
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.selected().unwrap().source, "config");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn windows_case_variants_of_the_same_path_are_deduplicated() {
        let root = temp_dir("case-deduplicate");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("adb.exe");
        fs::write(&executable, b"fixture").unwrap();
        let uppercase = root.join("ADB.EXE");
        let report = DiscoveryReport::from_candidates(
            "ADB",
            [(executable.clone(), "config"), (uppercase, "path")],
        );
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.selected_path(), Some(executable));
        assert_eq!(report.selected().unwrap().source, "config");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_report_lists_candidate_and_source() {
        let path = temp_dir("missing").join("adb.exe");
        let report = DiscoveryReport::from_candidates("ADB", [(path.clone(), "config")]);
        let summary = report.summary();
        assert!(summary.contains("未找到"));
        assert!(summary.contains(&path.display().to_string()));
        assert!(summary.contains("config"));
    }
}
