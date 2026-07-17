mod common;

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use common::{TestDir, run_loom};

const SKILL_NAME: &str = "loom-registry";

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create destination directory");
    for entry in fs::read_dir(source).expect("read source directory") {
        let entry = entry.expect("read source entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("read source entry type");
        if file_type.is_dir() {
            copy_tree(&source_path, &destination_path);
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).expect("copy source file");
        } else {
            panic!("shipped skill must not contain symlinks or special files");
        }
    }
}

fn trigger_cases() -> Vec<Value> {
    fs::read_to_string(repo_path("skills/loom-registry/evals/triggers.jsonl"))
        .expect("read shipped trigger fixtures")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse shipped trigger fixture"))
        .collect()
}

#[test]
fn shipped_skill_has_collision_resistant_metadata_and_trigger_boundaries() {
    let skill = fs::read_to_string(repo_path("skills/loom-registry/SKILL.md"))
        .expect("release source must contain skills/loom-registry/SKILL.md");
    assert!(skill.starts_with("---\nname: loom-registry\ndescription:"));
    assert!(skill.contains("Loom.com"));
    assert!(skill.contains("loom --json --root"));
    assert!(skill.contains("loom --version"));
    assert!(skill.contains("data.safe_to_apply=true"));
    assert!(skill.contains("data.convergence"));
    assert!(skill.contains("registry_transport=SYNCED"));
    assert!(skill.contains("visibility=restart_required"));
    assert!(skill.contains("never authorizes the write by itself"));
    assert!(!skill.contains("skill capture"));
    assert!(!skill.contains("skill activate \"$SKILL\" --agent codex --scope user\n"));
    assert!(skill.contains("docs/AGENT_USAGE.md"));
    assert!(skill.contains("docs/SINGLE_SKILL_LIFECYCLE.md"));

    let manifest = fs::read_to_string(repo_path("skills/loom-registry/loom.skill.toml"))
        .expect("read shipped Loom manifest");
    assert!(manifest.contains("schema = \"loom.skill.v1\""));
    assert!(manifest.contains("name = \"loom-registry\""));
    assert!(manifest.contains("requires_tools = [\"loom\"]"));

    let openai = fs::read_to_string(repo_path("skills/loom-registry/agents/openai.yaml"))
        .expect("read shipped OpenAI metadata");
    assert!(openai.contains("$loom-registry"));

    let cases = trigger_cases();
    let positives: Vec<_> = cases
        .iter()
        .filter(|case| case["expected_trigger"] == json!(true))
        .collect();
    let negatives: Vec<_> = cases
        .iter()
        .filter(|case| case["expected_trigger"] == json!(false))
        .collect();
    assert!(
        positives.len() >= 4,
        "local registry coverage is incomplete"
    );
    assert!(
        negatives.len() >= 3,
        "Loom.com exclusion coverage is incomplete"
    );
    assert!(
        positives
            .iter()
            .all(|case| case["observed_trigger"] == json!(true))
    );
    assert!(
        negatives
            .iter()
            .all(|case| case["observed_trigger"] == json!(false))
    );
    assert!(negatives.iter().all(|case| {
        let prompt = case["prompt"].as_str().unwrap_or_default().to_lowercase();
        prompt.contains("loom.com") || prompt.contains("video")
    }));
}

#[test]
fn shipped_skill_passes_portable_agent_and_offline_trigger_checks() {
    let source = repo_path("skills/loom-registry");
    assert!(source.is_dir(), "first-party skill source is missing");
    let root = TestDir::new("shipped-loom-registry-skill");
    copy_tree(&source, &root.path().join("skills").join(SKILL_NAME));

    for agent in ["claude", "codex"] {
        let (output, envelope) = run_loom(
            root.path(),
            &["skill", "lint", SKILL_NAME, "--strict", "--agent", agent],
        );
        assert!(
            output.status.success(),
            "{agent} lint failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(envelope["ok"], json!(true));
        assert_eq!(envelope["data"]["valid"], json!(true));
        assert_eq!(
            envelope["data"]["sections"]["agent_compatibility"][agent]["status"],
            json!("pass")
        );
    }

    let (output, envelope) = run_loom(
        root.path(),
        &["skill", "eval", SKILL_NAME, "--matrix", "claude,codex"],
    );
    assert!(
        output.status.success(),
        "offline trigger eval failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let fixture_count = trigger_cases().len() * 2;
    assert_eq!(envelope["data"]["summary"]["case_count"], fixture_count);
    assert_eq!(envelope["data"]["summary"]["failed"], json!(0));
    assert_eq!(envelope["data"]["summary"]["trigger_precision"], json!(1.0));
    assert_eq!(envelope["data"]["summary"]["trigger_recall"], json!(1.0));
}

#[test]
fn release_and_install_surfaces_ship_the_same_fail_closed_skill() {
    let workflow = fs::read_to_string(repo_path(".github/workflows/release.yml"))
        .expect("read release workflow");
    assert!(workflow.contains("skills/loom-registry"));
    assert!(workflow.contains("agents/openai.yaml"));
    assert!(workflow.contains("pkgshare.install \"skills\""));
    assert!(workflow.contains("pkgshare.install \"loom\""));
    assert!(workflow.contains("bin.install_symlink pkgshare/\"loom\""));
    assert!(workflow.contains("--contract-version \"$contract_version\""));
    assert!(workflow.contains("git fetch origin"));
    assert!(workflow.contains("git restore -- Formula/loom.rb"));
    assert!(workflow.contains("gh pr list"));
    assert!(!workflow.contains("--force"));

    let readme = fs::read_to_string(repo_path("README.md")).expect("read README");
    assert!(readme.contains("$HOME/.claude/skills/loom-registry"));
    assert!(readme.contains("$HOME/.agents/skills/loom-registry"));
    assert!(readme.contains("Refusing to overwrite existing Skill"));
    assert!(readme.contains("brew --prefix loom"));

    let runbook = fs::read_to_string(repo_path("docs/AGENT_USAGE.md")).expect("read agent runbook");
    assert!(runbook.contains("`loom-registry`"));
    assert!(!runbook.contains("`loom` 技能"));
    assert!(!runbook.contains("skill save"));
    assert!(!runbook.contains("skill snapshot"));
    assert!(!runbook.contains("skill capture"));
    assert!(runbook.contains("skill commit"));
    assert!(runbook.contains("skill release <skill> --anchor"));

    let lifecycle = fs::read_to_string(repo_path("docs/SINGLE_SKILL_LIFECYCLE.md"))
        .expect("read single-Skill lifecycle guide");
    assert!(!lifecycle.contains("skill save"));
    assert!(!lifecycle.contains("skill snapshot"));
    assert!(lifecycle.contains("skill commit <skill> --from-source"));
    assert!(lifecycle.contains("skill release fixflow --anchor"));
}
