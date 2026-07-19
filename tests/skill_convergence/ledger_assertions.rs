use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};

use super::snapshot_tree;

pub(crate) fn assert_exact_retained_ledger(journal_path: &Path, phase: &str) -> Value {
    let journal: Value =
        serde_json::from_slice(&std::fs::read(journal_path).expect("retained transaction journal"))
            .expect("parse retained transaction journal");
    assert_eq!(journal["phase"], json!(phase));
    let plan_id = journal["plan_id"].as_str().expect("journal plan id");
    let attempts = journal["ownership_attempts"]
        .as_array()
        .expect("attempt ledger");
    assert!(!attempts.is_empty(), "ownership ledger must not be empty");
    let mut candidates = BTreeSet::new();
    for attempt in attempts {
        let candidate = attempt["candidate_path"].as_str().expect("candidate path");
        candidates.insert(candidate.to_string());
        let state = attempt["state"].as_str().expect("attempt state");
        let path = match state {
            "retained" => attempt["destination"].as_str().expect("destination"),
            "abandoned" => candidate,
            unexpected => panic!("non-terminal ownership state {unexpected}"),
        };
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if state == "abandoned" && err.kind() == std::io::ErrorKind::NotFound => {
                continue;
            }
            Err(err) => panic!("ledgered path is unavailable: {path}: {err}"),
        };
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        if state == "retained" {
            assert_eq!(read_marker(path, ".owner"), plan_id);
            assert_eq!(
                read_marker(path, ".reservation-owner"),
                attempt["proof"].as_str().expect("proof")
            );
            let manifest: Value = serde_json::from_slice(
                &std::fs::read(Path::new(path).join(".ownership-manifest.json"))
                    .expect("ownership manifest"),
            )
            .expect("parse ownership manifest");
            assert_eq!(manifest["plan_id"], json!(plan_id));
            assert_eq!(manifest["destination"], attempt["destination"]);
            assert_eq!(manifest["proof"], attempt["proof"]);
        }
    }
    for candidate in &candidates {
        let parent = Path::new(candidate).parent().expect("candidate parent");
        for entry in std::fs::read_dir(parent).expect("attempt parent") {
            let sibling = entry.expect("attempt sibling").path();
            if sibling
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(".ownership-attempt-"))
            {
                assert!(
                    candidates.contains(&sibling.display().to_string()),
                    "unledgered ownership attempt: {}",
                    sibling.display()
                );
            }
        }
    }
    journal
}

pub(crate) fn snapshot_without_ledgered_paths(
    root: &Path,
    journal_path: &Path,
    phase: &str,
) -> BTreeMap<String, Vec<u8>> {
    let journal = assert_exact_retained_ledger(journal_path, phase);
    let mut snapshot = snapshot_tree(root);
    let canonical_root = std::fs::canonicalize(root).expect("canonical snapshot root");
    for attempt in journal["ownership_attempts"]
        .as_array()
        .expect("attempt ledger")
    {
        let path = match attempt["state"].as_str().expect("attempt state") {
            "retained" => attempt["destination"].as_str().expect("destination"),
            "abandoned" => attempt["candidate_path"].as_str().expect("candidate"),
            state => panic!("non-terminal ownership state {state}"),
        };
        let comparable =
            std::fs::canonicalize(path).unwrap_or_else(|_| Path::new(path).to_path_buf());
        if let Ok(relative) = comparable.strip_prefix(&canonical_root) {
            let artifact_root =
                Path::new(journal["artifact_root"].as_str().expect("artifact root"));
            let is_artifact_root = Path::new(path) == artifact_root;
            snapshot.retain(|entry, _| {
                let Ok(descendant) = Path::new(entry).strip_prefix(relative) else {
                    return true;
                };
                let Some(name) = descendant
                    .components()
                    .next()
                    .and_then(|component| component.as_os_str().to_str())
                else {
                    return true;
                };
                let ownership_metadata = matches!(
                    name,
                    ".owner" | ".reservation-owner" | ".ownership-manifest.json"
                );
                let declared_child = if is_artifact_root {
                    name == "index"
                        || name.starts_with("source")
                        || name.starts_with("projection-")
                        || name.starts_with("registry-")
                } else {
                    name.starts_with("stage")
                };
                !ownership_metadata && !declared_child
            });
        }
    }
    snapshot
}

pub(crate) fn status_without_ledgered_transactions(
    repo: &Path,
    status: &str,
    journal_path: &Path,
    phase: &str,
) -> String {
    let journal = assert_exact_retained_ledger(journal_path, phase);
    let tx_dir = journal_path.parent().expect("transaction directory");
    let mut allowed = vec![journal_path.to_path_buf()];
    for attempt in journal["ownership_attempts"]
        .as_array()
        .expect("attempt ledger")
    {
        for field in ["destination", "candidate_path"] {
            let path = Path::new(attempt[field].as_str().expect("ledger path"));
            if path.starts_with(tx_dir) {
                allowed.push(path.to_path_buf());
            }
        }
    }
    for entry in std::fs::read_dir(tx_dir).expect("transaction directory") {
        let path = entry.expect("transaction entry").path();
        assert!(
            allowed
                .iter()
                .any(|allowed| path == *allowed || path.starts_with(allowed)),
            "unledgered transaction entry: {}",
            path.display()
        );
    }
    let tx_relative = tx_dir
        .strip_prefix(repo)
        .expect("transaction dir under repo");
    status
        .lines()
        .filter(|line| line.get(3..) != Some(&format!("{}/", tx_relative.display())))
        .map(|line| format!("{line}\n"))
        .collect()
}

fn read_marker(root: &str, name: &str) -> String {
    std::fs::read_to_string(Path::new(root).join(name))
        .expect("ownership marker")
        .trim()
        .to_string()
}
