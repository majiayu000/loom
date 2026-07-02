use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

#[path = "../src/sha256.rs"]
mod sha256;

mod common;
use common::{TestDir, run_loom, write_file, write_skill};

#[test]
fn dry_run_reports_plan_and_writes_no_artifacts() {
    let root = TestDir::new("skill-compile-dry-run");
    write_compile_skill(root.path(), "demo");
    write_file(
        &root.path().join("skills/demo/references/guide.md"),
        "# Guide\nUse when detailed context is needed.\n",
    );

    let (_output, env) = run_loom(
        root.path(),
        &["skill", "compile", "demo", "--dry-run", "--agent", "codex"],
    );

    assert_eq!(env["ok"], json!(true), "{env}");
    assert_eq!(env["cmd"], json!("skill.compile.dry_run"));
    assert_eq!(env["data"]["skill"], json!("demo"));
    assert_eq!(env["data"]["agent"], json!("codex"));
    assert_eq!(env["data"]["profile"], json!("default"));
    assert_eq!(env["data"]["writes_artifacts"], json!(false));
    assert!(
        env["data"]["artifact"]["paths"]["manifest.json"]
            .as_str()
            .is_some_and(|path| path.contains("state/compiled/skills/demo/"))
    );
    assert!(
        env["data"]["source"]["digest_inputs"]
            .as_array()
            .expect("digest inputs")
            .iter()
            .any(|input| input["path"] == json!("SKILL.md"))
    );
    assert_eq!(env["data"]["manifest"]["gates"]["eval"], json!("missing"));
    assert_eq!(
        env["data"]["manifest"]["content_hashes"]["activation_md"]
            .as_str()
            .map(|value| value.starts_with("sha256:")),
        Some(true)
    );
    assert!(
        !root.path().join("state/compiled").exists(),
        "dry-run must not write compiled artifact state"
    );
}

#[test]
fn compile_writes_artifact_and_read_commands_consume_it() {
    let root = TestDir::new("skill-compile-write");
    write_non_blocking_compile_skill(root.path(), "demo");

    let (_output, env) = run_loom(
        root.path(),
        &["skill", "compile", "demo", "--agent", "codex"],
    );

    assert_eq!(env["ok"], json!(true), "{env}");
    assert_eq!(env["cmd"], json!("skill.compile"));
    assert_eq!(env["data"]["dry_run"], json!(false));
    assert_eq!(env["data"]["writes_artifacts"], json!(true));
    assert_eq!(env["data"]["manifest"]["status"], json!("experimental"));
    assert!(env["data"]["manifest"]["created_at"].as_str().is_some());
    assert_eq!(env["data"]["verification"]["valid"], json!(false));
    assert_eq!(env["data"]["verification"]["status"], json!("experimental"));
    assert_finding_value(&env["data"]["verification"], "artifact_status_not_valid");

    let artifact_id = env["data"]["artifact"]["artifact_id"]
        .as_str()
        .expect("artifact id");
    let artifact_dir = root
        .path()
        .join("state/compiled/skills/demo")
        .join(artifact_id);
    for file in [
        "manifest.json",
        "activation.md",
        "catalog.json",
        "boundaries.json",
        "tool-interface.json",
        "references.index.json",
        "source-digest.txt",
    ] {
        assert!(
            artifact_dir.join(file).is_file(),
            "missing compiled file {file}: {env}"
        );
    }

    let (_output, list) = run_loom(root.path(), &["skill", "compile", "list", "demo"]);
    assert_eq!(list["ok"], json!(true), "{list}");
    assert_eq!(list["data"]["count"], json!(1));
    assert_eq!(
        list["data"]["artifacts"][0]["artifact_id"],
        json!(artifact_id)
    );
    assert_eq!(
        list["data"]["artifacts"][0]["manifest_status"],
        json!("parseable")
    );
    assert_eq!(
        list["data"]["artifacts"][0]["status"],
        json!("experimental")
    );

    let (_output, verify) = run_loom(
        root.path(),
        &[
            "skill",
            "compile",
            "verify",
            "demo",
            "--artifact",
            artifact_id,
        ],
    );
    assert_eq!(verify["ok"], json!(true), "{verify}");
    assert_eq!(verify["data"]["count"], json!(1));
    assert_eq!(verify["data"]["valid"], json!(false));
    assert_eq!(
        verify["data"]["artifacts"][0]["artifact_id"],
        json!(artifact_id)
    );
    assert_finding(&verify, "artifact_status_not_valid");
}

#[test]
fn compile_promotes_valid_artifact_with_eval_evidence() {
    let root = TestDir::new("skill-compile-valid-eval");
    write_non_blocking_compile_skill(root.path(), "demo");
    write_passing_eval(root.path(), "demo");

    let (_output, env) = run_loom(
        root.path(),
        &["skill", "compile", "demo", "--agent", "codex"],
    );

    assert_eq!(env["ok"], json!(true), "{env}");
    assert_eq!(env["data"]["manifest"]["status"], json!("valid"));
    assert_eq!(env["data"]["manifest"]["gates"]["eval"], json!("pass"));
    assert_eq!(
        env["data"]["manifest"]["eval_evidence"]["agent"],
        json!("codex")
    );
    assert_eq!(env["data"]["verification"]["valid"], json!(true));
    let artifact_id = env["data"]["artifact"]["artifact_id"]
        .as_str()
        .expect("artifact id");

    let (_output, verify) = run_loom(
        root.path(),
        &[
            "skill",
            "compile",
            "verify",
            "demo",
            "--artifact",
            artifact_id,
        ],
    );
    assert_eq!(verify["ok"], json!(true), "{verify}");
    assert_eq!(verify["data"]["valid"], json!(true), "{verify}");

    write_file(
        &root.path().join("skills/demo/evals/tasks.jsonl"),
        r#"{"id":"changed","task":"Run the compile eval","output":"Changed result","trace":["read SKILL.md"],"checks":{"outcome_contains":["Changed"]}}
"#,
    );
    let (_output, stale) = run_loom(
        root.path(),
        &[
            "skill",
            "compile",
            "verify",
            "demo",
            "--artifact",
            artifact_id,
        ],
    );
    assert_eq!(stale["ok"], json!(true), "{stale}");
    assert_eq!(stale["data"]["valid"], json!(false));
    assert_finding(&stale, "eval_evidence_stale");
}

#[test]
fn compile_keeps_artifact_experimental_when_eval_fails() {
    let root = TestDir::new("skill-compile-eval-fail");
    write_non_blocking_compile_skill(root.path(), "demo");
    write_file(
        &root.path().join("skills/demo/evals/triggers.jsonl"),
        r#"{"id":"regression","prompt":"Use demo here","expected_trigger":true,"observed_trigger":false}
"#,
    );

    let (_output, env) = run_loom(
        root.path(),
        &["skill", "compile", "demo", "--agent", "codex"],
    );

    assert_eq!(env["ok"], json!(true), "{env}");
    assert_eq!(env["data"]["manifest"]["status"], json!("experimental"));
    assert_eq!(env["data"]["manifest"]["gates"]["eval"], json!("fail"));
    assert_eq!(env["data"]["verification"]["valid"], json!(false));
    assert_finding_value(&env["data"]["verification"], "gate_blocks_valid_artifact");
}

#[test]
fn small_skill_reports_no_op() {
    let root = TestDir::new("skill-compile-no-op");
    write_compile_skill(root.path(), "small");

    let (_output, env) = run_loom(root.path(), &["skill", "compile", "small", "--dry-run"]);

    assert_eq!(env["ok"], json!(true), "{env}");
    assert_eq!(env["data"]["no_op"], json!(true));
    assert!(
        env["data"]["no_op_reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("below compile threshold")),
        "{env}"
    );
}

#[test]
fn list_and_verify_use_deterministic_artifact_order() {
    let root = TestDir::new("skill-compile-list-sort");
    write_compile_skill(root.path(), "demo");
    let plan = compile_plan(root.path(), "demo");
    write_artifact_from_plan(root.path(), "demo", &plan, "zeta", None);
    write_artifact_from_plan(root.path(), "demo", &plan, "alpha", None);

    let (_output, list) = run_loom(root.path(), &["skill", "compile", "list", "demo"]);
    assert_eq!(list["ok"], json!(true), "{list}");
    let listed = collect_artifact_ids(&list["data"]["artifacts"]);
    assert_eq!(listed, vec!["alpha", "zeta"]);

    let (_output, verify) = run_loom(root.path(), &["skill", "compile", "verify", "demo"]);
    assert_eq!(verify["ok"], json!(true), "{verify}");
    let verified = collect_artifact_ids(&verify["data"]["artifacts"]);
    assert_eq!(verified, vec!["alpha", "zeta"]);
}

#[test]
fn verify_detects_missing_files() {
    let root = TestDir::new("skill-compile-missing-files");
    write_compile_skill(root.path(), "demo");
    let plan = compile_plan(root.path(), "demo");
    let dir = write_artifact_from_plan(root.path(), "demo", &plan, "missing", None);
    fs::remove_file(dir.join("activation.md")).expect("remove activation");

    let (_output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "compile",
            "verify",
            "demo",
            "--artifact",
            "missing",
        ],
    );

    assert_eq!(env["ok"], json!(true), "{env}");
    assert_eq!(env["data"]["valid"], json!(false));
    assert_finding(&env, "required_file_missing");
}

#[test]
fn verify_rejects_malformed_manifest_with_typed_error() {
    let root = TestDir::new("skill-compile-malformed-manifest");
    write_compile_skill(root.path(), "demo");
    let dir = root.path().join("state/compiled/skills/demo/bad");
    fs::create_dir_all(&dir).expect("create artifact dir");
    write_file(&dir.join("manifest.json"), "{not-json");

    let (output, env) = run_loom(
        root.path(),
        &["skill", "compile", "verify", "demo", "--artifact", "bad"],
    );

    assert!(!output.status.success(), "{env}");
    assert_eq!(env["ok"], json!(false), "{env}");
    assert_eq!(env["error"]["code"], json!("SCHEMA_MISMATCH"));
}

#[test]
fn verify_detects_stale_source_digest_after_source_edit() {
    let root = TestDir::new("skill-compile-stale");
    write_compile_skill(root.path(), "demo");
    let plan = compile_plan(root.path(), "demo");
    write_artifact_from_plan(
        root.path(),
        "demo",
        &plan,
        "fresh-then-stale",
        Some(|manifest| {
            manifest["status"] = json!("valid");
            manifest["gates"] = json!({
                "lint": "pass",
                "safety": "pass",
                "dependency": "pass",
                "eval": "pass"
            });
        }),
    );
    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        &format!("{}\n\n# Edited\n", base_skill_body("demo")),
    );

    let (_output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "compile",
            "verify",
            "demo",
            "--artifact",
            "fresh-then-stale",
        ],
    );

    assert_eq!(env["ok"], json!(true), "{env}");
    assert_eq!(env["data"]["artifacts"][0]["source_stale"], json!(true));
    assert_finding(&env, "source_digest_stale");
}

#[test]
fn verify_blocks_valid_status_when_required_gate_is_missing() {
    let root = TestDir::new("skill-compile-gate-block");
    write_compile_skill(root.path(), "demo");
    let plan = compile_plan(root.path(), "demo");
    write_artifact_from_plan(
        root.path(),
        "demo",
        &plan,
        "gate-missing",
        Some(|manifest| {
            manifest["status"] = json!("valid");
            manifest["gates"]["eval"] = json!("missing");
        }),
    );

    let (_output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "compile",
            "verify",
            "demo",
            "--artifact",
            "gate-missing",
        ],
    );

    assert_eq!(env["ok"], json!(true), "{env}");
    assert_eq!(env["data"]["valid"], json!(false));
    assert_finding(&env, "gate_blocks_valid_artifact");
}

#[test]
fn verify_rejects_unsafe_artifact_id_before_path_join() {
    let root = TestDir::new("skill-compile-unsafe-artifact");
    write_compile_skill(root.path(), "demo");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "compile",
            "verify",
            "demo",
            "--artifact",
            "../escape",
        ],
    );

    assert!(!output.status.success(), "{env}");
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
}

#[test]
fn verify_detects_sidecar_path_escape() {
    let root = TestDir::new("skill-compile-path-escape");
    write_compile_skill(root.path(), "demo");
    let plan = compile_plan(root.path(), "demo");
    write_artifact_from_plan(
        root.path(),
        "demo",
        &plan,
        "escape",
        Some(|manifest| {
            manifest["planned_reference_escape"] = json!(true);
        }),
    );

    let (_output, env) = run_loom(
        root.path(),
        &["skill", "compile", "verify", "demo", "--artifact", "escape"],
    );

    assert_eq!(env["ok"], json!(true), "{env}");
    assert_eq!(env["data"]["valid"], json!(false));
    assert_finding(&env, "reference_path_escape");
}

#[test]
fn parser_disambiguates_list_skill_name_from_list_subcommand() {
    let root = TestDir::new("skill-compile-disambiguation");
    write_compile_skill(root.path(), "list");
    let plan = compile_plan(root.path(), "list");
    write_artifact_from_plan(root.path(), "list", &plan, "artifact-a", None);

    let (_output, dry_run) = run_loom(
        root.path(),
        &["skill", "compile", "--skill", "list", "--dry-run"],
    );
    assert_eq!(dry_run["ok"], json!(true), "{dry_run}");
    assert_eq!(dry_run["cmd"], json!("skill.compile.dry_run"));
    assert_eq!(dry_run["data"]["skill"], json!("list"));

    let (_output, list) = run_loom(root.path(), &["skill", "compile", "list", "list"]);
    assert_eq!(list["ok"], json!(true), "{list}");
    assert_eq!(list["cmd"], json!("skill.compile.list"));
    assert_eq!(list["data"]["count"], json!(1));
}

fn compile_plan(root: &Path, skill: &str) -> Value {
    let args = if matches!(skill, "list" | "verify") {
        vec![
            "skill",
            "compile",
            "--skill",
            skill,
            "--dry-run",
            "--agent",
            "codex",
        ]
    } else {
        vec!["skill", "compile", skill, "--dry-run", "--agent", "codex"]
    };
    let (_output, env) = run_loom(root, &args);
    assert_eq!(env["ok"], json!(true), "{env}");
    env
}

fn write_artifact_from_plan(
    root: &Path,
    skill: &str,
    plan: &Value,
    artifact_id: &str,
    mutate_manifest: Option<fn(&mut Value)>,
) -> PathBuf {
    let artifact_dir = root
        .join("state/compiled/skills")
        .join(skill)
        .join(artifact_id);
    fs::create_dir_all(&artifact_dir).expect("create artifact dir");
    let content = &plan["data"]["planned_content"];
    let files = [
        "activation.md",
        "catalog.json",
        "boundaries.json",
        "tool-interface.json",
        "references.index.json",
        "source-digest.txt",
    ];
    for file in files {
        let path = artifact_dir.join(file);
        if file.ends_with(".json") {
            write_test_json_file(&path, &content[file]);
        } else {
            write_file(&path, content[file].as_str().expect("planned text file"));
        }
    }

    let mut manifest = plan["data"]["manifest"].clone();
    manifest["artifact_id"] = json!(artifact_id);
    manifest["content_hashes"] = json!({
        "activation_md": hash_file(&artifact_dir.join("activation.md")),
        "catalog_json": hash_file(&artifact_dir.join("catalog.json")),
        "boundaries_json": hash_file(&artifact_dir.join("boundaries.json")),
        "tool_interface_json": hash_file(&artifact_dir.join("tool-interface.json")),
        "references_index_json": hash_file(&artifact_dir.join("references.index.json")),
    });
    if let Some(mutate) = mutate_manifest {
        mutate(&mut manifest);
    }
    if manifest
        .get("planned_reference_escape")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        manifest
            .as_object_mut()
            .expect("manifest object")
            .remove("planned_reference_escape");
        let mut references = content["references.index.json"].clone();
        references["references"] = json!([{
            "path": "../secret.md",
            "role": "metadata",
            "load_condition": "on-demand",
            "content_hash": "sha256:0000"
        }]);
        write_test_json_file(&artifact_dir.join("references.index.json"), &references);
        manifest["content_hashes"]["references_index_json"] =
            json!(hash_file(&artifact_dir.join("references.index.json")));
    }
    write_test_json_file(&artifact_dir.join("manifest.json"), &manifest);
    artifact_dir
}

fn write_test_json_file(path: &Path, value: &Value) {
    let mut raw = serde_json::to_string_pretty(value).expect("serialize json file");
    raw.push('\n');
    write_file(path, &raw);
}

fn hash_file(path: &Path) -> String {
    let bytes = fs::read(path).expect("read hash file");
    let mut hasher = sha256::Sha256::new();
    hasher.update(&bytes);
    format!("sha256:{}", sha256::to_hex(&hasher.finalize()))
}

fn collect_artifact_ids(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("artifact array")
        .iter()
        .map(|artifact| {
            artifact["artifact_id"]
                .as_str()
                .expect("artifact id")
                .to_string()
        })
        .collect()
}

fn assert_finding(env: &Value, id: &str) {
    let findings = env["data"]["artifacts"][0]["findings"]
        .as_array()
        .expect("findings array");
    assert!(
        findings.iter().any(|finding| finding["id"] == json!(id)),
        "missing finding {id}: {env}"
    );
}

fn assert_finding_value(report: &Value, id: &str) {
    let findings = report["findings"].as_array().expect("findings array");
    assert!(
        findings.iter().any(|finding| finding["id"] == json!(id)),
        "missing finding {id}: {report}"
    );
}

fn write_compile_skill(root: &Path, skill: &str) {
    write_skill(root, skill, &base_skill_body(skill));
}

fn write_non_blocking_compile_skill(root: &Path, skill: &str) {
    write_skill(root, skill, &non_blocking_skill_body(skill));
}

fn write_passing_eval(root: &Path, skill: &str) {
    write_file(
        &root.join("skills").join(skill).join("evals/tasks.jsonl"),
        r#"{"id":"happy-path","task":"Run the compile eval","output":"Done with concise result","trace":["read SKILL.md","checked output"],"metrics":{"tokens":40,"commands":1},"checks":{"outcome_contains":["Done"],"process_contains":["SKILL.md"],"style_contains":["concise"],"max_tokens":100,"max_commands":3}}
"#,
    );
}

fn base_skill_body(skill: &str) -> String {
    format!(
        "---\nname: {skill}\ndescription: Use when testing compile planning.\nallowed-tools: shell\n---\n# {skill}\n\nUse when testing compile planning.\n\nDo not use for production claims.\n"
    )
}

fn non_blocking_skill_body(skill: &str) -> String {
    format!(
        "---\nname: {skill}\ndescription: Use when testing compile artifact writes.\n---\n# {skill}\n\nUse when testing compile artifact writes.\n\nDo not use for production claims.\n"
    )
}
