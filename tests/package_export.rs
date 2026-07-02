mod common;
#[path = "../src/sha256.rs"]
mod sha256;

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use common::{TestDir, run_loom, run_loom_in_cwd, write_file, write_skill};
use serde_json::json;
use tar::{Archive, Builder, EntryType, Header};

fn write_fixture_skill(root: &Path, skill: &str) {
    write_skill(
        root,
        skill,
        &format!(
            "---\nname: {skill}\ndescription: Use when packaging deterministic skill artifacts.\n---\n# {skill}\n\nPortable content.\n"
        ),
    );
}

#[test]
fn package_plan_build_verify_and_rebuild_are_deterministic() {
    let root = TestDir::new("package-round-trip");
    write_fixture_skill(root.path(), "fixflow");
    let plan = root.path().join("plan.json");
    let artifact_a = root.path().join("fixflow-a.tar");
    let artifact_b = root.path().join("fixflow-b.tar");
    let plan_arg = plan.to_string_lossy().into_owned();
    let artifact_a_arg = artifact_a.to_string_lossy().into_owned();
    let artifact_b_arg = artifact_b.to_string_lossy().into_owned();

    let (output, env) = run_loom(
        root.path(),
        &[
            "package",
            "plan",
            "skill:fixflow",
            "--format",
            "agent-skills-archive",
            "--output-plan",
            &plan_arg,
        ],
    );
    assert!(output.status.success(), "package plan should pass: {env}");
    assert_eq!(env["cmd"], json!("package.plan"));
    assert_eq!(env["data"]["plan"]["source"]["kind"], json!("skill"));
    assert_eq!(
        env["data"]["plan"]["checks"]["portable_lint"],
        json!("pass")
    );
    assert_eq!(
        env["data"]["plan"]["checks"]["safety_scan"],
        json!("not_run")
    );
    assert!(plan.is_file());
    assert!(
        !artifact_a.exists(),
        "plan must not write package artifacts"
    );

    let (output, build) = run_loom(
        root.path(),
        &[
            "package",
            "build",
            &plan_arg,
            "--output",
            &artifact_a_arg,
            "--idempotency-key",
            "key-a",
        ],
    );
    assert!(
        output.status.success(),
        "package build should pass: {build}"
    );
    assert_eq!(build["cmd"], json!("package.build"));
    assert_eq!(build["data"]["active_state_claim"], json!(false));
    assert!(artifact_a.is_file());

    let (output, replay) = run_loom(
        root.path(),
        &[
            "package",
            "build",
            &plan_arg,
            "--output",
            &artifact_a_arg,
            "--idempotency-key",
            "key-a",
        ],
    );
    assert!(
        output.status.success(),
        "matching existing artifact should replay: {replay}"
    );
    assert_eq!(replay["data"]["idempotent_replay"], json!(true));

    let (output, verify) = run_loom(
        root.path(),
        &[
            "package",
            "verify",
            &artifact_a_arg,
            "--format",
            "agent-skills-archive",
        ],
    );
    assert!(
        output.status.success(),
        "package verify should pass: {verify}"
    );
    assert_eq!(verify["cmd"], json!("package.verify"));
    assert_eq!(verify["data"]["valid"], json!(true));
    assert_eq!(
        verify["data"]["manifest"]["source_digest"],
        build["data"]["manifest"]["source_digest"]
    );

    let (output, second) = run_loom(
        root.path(),
        &[
            "package",
            "build",
            &plan_arg,
            "--output",
            &artifact_b_arg,
            "--idempotency-key",
            "key-b",
        ],
    );
    assert!(
        output.status.success(),
        "second build should pass: {second}"
    );
    assert_eq!(
        fs::read(&artifact_a).expect("read first artifact"),
        fs::read(&artifact_b).expect("read second artifact"),
        "rebuilding the same reviewed plan should be byte deterministic"
    );
}

#[test]
fn package_verify_detects_checksum_and_stale_source() {
    let root = TestDir::new("package-verify-failures");
    write_fixture_skill(root.path(), "fixflow");
    let plan = root.path().join("plan.json");
    let artifact = root.path().join("fixflow.tar");
    let tampered = root.path().join("fixflow-tampered.tar");
    let plan_arg = plan.to_string_lossy().into_owned();
    let artifact_arg = artifact.to_string_lossy().into_owned();
    let tampered_arg = tampered.to_string_lossy().into_owned();

    assert!(
        run_loom(
            root.path(),
            &[
                "package",
                "plan",
                "fixflow",
                "--format",
                "agent-skills-archive",
                "--output-plan",
                &plan_arg,
            ],
        )
        .0
        .status
        .success()
    );
    assert!(
        run_loom(
            root.path(),
            &[
                "package",
                "build",
                &plan_arg,
                "--output",
                &artifact_arg,
                "--idempotency-key",
                "key",
            ],
        )
        .0
        .status
        .success()
    );

    rewrite_archive_with_tampered_skill(&artifact, &tampered);
    let (output, env) = run_loom(root.path(), &["package", "verify", &tampered_arg]);
    assert!(!output.status.success(), "tampered artifact must fail");
    assert_eq!(env["error"]["code"], json!("STATE_CORRUPT"));
    assert!(
        env["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("checksum mismatch")),
        "unexpected checksum error: {env}"
    );

    write_file(
        &root.path().join("skills/fixflow/SKILL.md"),
        "---\nname: fixflow\ndescription: Use when packaging changed artifacts.\n---\n# Changed\n",
    );
    let (output, env) = run_loom(root.path(), &["package", "verify", &artifact_arg]);
    assert!(!output.status.success(), "stale source must fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
}

#[test]
fn package_verify_binds_contents_to_manifest_and_allows_missing_source() {
    let root = TestDir::new("package-verify-manifest-binding");
    write_fixture_skill(root.path(), "fixflow");
    let plan = root.path().join("plan.json");
    let artifact = root.path().join("fixflow.tar");
    let tampered = root.path().join("fixflow-tampered-rechecksummed.tar");
    let plan_arg = plan.to_string_lossy().into_owned();
    let artifact_arg = artifact.to_string_lossy().into_owned();
    let tampered_arg = tampered.to_string_lossy().into_owned();

    assert!(
        run_loom(
            root.path(),
            &[
                "package",
                "plan",
                "fixflow",
                "--format",
                "agent-skills-archive",
                "--output-plan",
                &plan_arg,
            ],
        )
        .0
        .status
        .success()
    );
    assert!(
        run_loom(
            root.path(),
            &[
                "package",
                "build",
                &plan_arg,
                "--output",
                &artifact_arg,
                "--idempotency-key",
                "key",
            ],
        )
        .0
        .status
        .success()
    );

    rewrite_archive_with_tampered_skill_and_checksums(&artifact, &tampered);
    let (output, env) = run_loom(root.path(), &["package", "verify", &tampered_arg]);
    assert!(
        !output.status.success(),
        "manifest hash mismatch should fail even when checksums match"
    );
    assert_eq!(env["error"]["code"], json!("STATE_CORRUPT"));
    assert!(
        env["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("manifest")),
        "unexpected manifest binding error: {env}"
    );

    fs::remove_dir_all(root.path().join("skills/fixflow")).expect("remove local source");
    let (output, env) = run_loom(root.path(), &["package", "verify", &artifact_arg]);
    assert!(
        output.status.success(),
        "portable artifact should verify without local source: {env}"
    );
    assert_eq!(env["data"]["source_fresh"], json!("unknown"));
}

#[test]
fn package_plan_blocks_unsafe_sources_and_unsupported_formats() {
    let root = TestDir::new("package-policy-blocks");
    write_fixture_skill(root.path(), "fixflow");
    let (output, env) = run_loom(
        root.path(),
        &["package", "plan", "fixflow", "--format", "codex-plugin"],
    );
    assert!(!output.status.success(), "unsupported format must fail");
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));

    write_file(
        &root.path().join("state/registry/trust.json"),
        r#"{"schema_version":1,"skills":[{"skill_id":"fixflow","trust":"blocked","quarantined":false,"reason":"test","updated_at":"2026-07-01T00:00:00Z","updated_by":"test"}]}
"#,
    );
    let (output, env) = run_loom(
        root.path(),
        &[
            "package",
            "plan",
            "fixflow",
            "--format",
            "agent-skills-archive",
        ],
    );
    assert!(!output.status.success(), "blocked skill must fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));

    write_file(
        &root.path().join("state/registry/trust.json"),
        r#"{"schema_version":1,"skills":[]}
"#,
    );
    write_file(
        &root.path().join("skills/fixflow/references/local.md"),
        &format!("local path {}\n", root.path().display()),
    );
    let (output, env) = run_loom(
        root.path(),
        &[
            "package",
            "plan",
            "fixflow",
            "--format",
            "agent-skills-archive",
        ],
    );
    assert!(!output.status.success(), "local path content must fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
}

#[test]
fn package_plan_resolves_skillsets_and_rejects_ambiguous_bare_ids() {
    let root = TestDir::new("package-skillset-source");
    write_fixture_skill(root.path(), "bundle");
    write_fixture_skill(root.path(), "fixflow");
    let (output, env) = run_loom(root.path(), &["skillset", "create", "bundle"]);
    assert!(
        output.status.success(),
        "skillset create should pass: {env}"
    );
    let (output, env) = run_loom(
        root.path(),
        &[
            "skillset",
            "add",
            "bundle",
            "fixflow",
            "--role",
            "execution",
        ],
    );
    assert!(output.status.success(), "skillset add should pass: {env}");

    let (output, env) = run_loom(
        root.path(),
        &[
            "package",
            "plan",
            "skillset:bundle",
            "--format",
            "agent-skills-archive",
        ],
    );
    assert!(
        output.status.success(),
        "skillset package plan should pass: {env}"
    );
    assert_eq!(env["data"]["plan"]["source"]["kind"], json!("skillset"));
    assert_eq!(
        env["data"]["plan"]["source"]["members"][0]["skill_id"],
        json!("fixflow")
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "package",
            "plan",
            "bundle",
            "--format",
            "agent-skills-archive",
        ],
    );
    assert!(!output.status.success(), "ambiguous bare id should fail");
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));

    write_fixture_skill(root.path(), "lintflow");
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "add", "bundle", "lintflow", "--role", "review"],
    );
    assert!(
        output.status.success(),
        "skillset mutation should pass: {env}"
    );
    let plan = root.path().join("bundle-plan.json");
    let artifact = root.path().join("bundle.tar");
    let plan_arg = plan.to_string_lossy().into_owned();
    let artifact_arg = artifact.to_string_lossy().into_owned();
    assert!(
        run_loom(
            root.path(),
            &[
                "package",
                "plan",
                "skillset:bundle",
                "--format",
                "agent-skills-archive",
                "--output-plan",
                &plan_arg,
            ],
        )
        .0
        .status
        .success()
    );
    write_fixture_skill(root.path(), "testflow");
    let (output, env) = run_loom(root.path(), &["skillset", "add", "bundle", "testflow"]);
    assert!(
        output.status.success(),
        "skillset stale mutation should pass: {env}"
    );
    let (output, env) = run_loom(
        root.path(),
        &[
            "package",
            "build",
            &plan_arg,
            "--output",
            &artifact_arg,
            "--idempotency-key",
            "key",
        ],
    );
    assert!(!output.status.success(), "stale skillset plan must fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
}

#[test]
fn package_plan_blocks_unsafe_skillset_metadata() {
    let root = TestDir::new("package-skillset-metadata-policy");
    write_fixture_skill(root.path(), "fixflow");
    let (output, env) = run_loom(
        root.path(),
        &[
            "skillset",
            "create",
            "bundle",
            "--description",
            "token=secret",
        ],
    );
    assert!(
        output.status.success(),
        "skillset create should pass: {env}"
    );
    let (output, env) = run_loom(root.path(), &["skillset", "add", "bundle", "fixflow"]);
    assert!(output.status.success(), "skillset add should pass: {env}");
    let (output, env) = run_loom(
        root.path(),
        &[
            "package",
            "plan",
            "skillset:bundle",
            "--format",
            "agent-skills-archive",
        ],
    );
    assert!(
        !output.status.success(),
        "unsafe skillset metadata must fail"
    );
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
}

#[test]
fn package_build_rejects_output_inside_source() {
    let root = TestDir::new("package-output-guard");
    write_fixture_skill(root.path(), "fixflow");
    let plan = root.path().join("plan.json");
    let plan_arg = plan.to_string_lossy().into_owned();
    let output = root.path().join("skills/fixflow/out.tar");
    let output_arg = output.to_string_lossy().into_owned();
    assert!(
        run_loom(
            root.path(),
            &[
                "package",
                "plan",
                "fixflow",
                "--format",
                "agent-skills-archive",
                "--output-plan",
                &plan_arg,
            ],
        )
        .0
        .status
        .success()
    );
    let (output_status, env) = run_loom(
        root.path(),
        &[
            "package",
            "build",
            &plan_arg,
            "--output",
            &output_arg,
            "--idempotency-key",
            "key",
        ],
    );
    assert!(!output_status.status.success());
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));

    let relative_output = "skills/fixflow/new/out.tar";
    let (output_status, env) = run_loom_in_cwd(
        root.path(),
        root.path(),
        &[
            "package",
            "build",
            &plan_arg,
            "--output",
            relative_output,
            "--idempotency-key",
            "key",
        ],
    );
    assert!(
        !output_status.status.success(),
        "relative output inside missing source dir must fail"
    );
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
}

#[test]
fn package_verify_rejects_unsupported_tar_entries() {
    let root = TestDir::new("package-unsupported-entry");
    write_fixture_skill(root.path(), "fixflow");
    let plan = root.path().join("plan.json");
    let artifact = root.path().join("fixflow.tar");
    let with_fifo = root.path().join("fixflow-fifo.tar");
    let plan_arg = plan.to_string_lossy().into_owned();
    let artifact_arg = artifact.to_string_lossy().into_owned();
    let with_fifo_arg = with_fifo.to_string_lossy().into_owned();

    assert!(
        run_loom(
            root.path(),
            &[
                "package",
                "plan",
                "fixflow",
                "--format",
                "agent-skills-archive",
                "--output-plan",
                &plan_arg,
            ],
        )
        .0
        .status
        .success()
    );
    assert!(
        run_loom(
            root.path(),
            &[
                "package",
                "build",
                &plan_arg,
                "--output",
                &artifact_arg,
                "--idempotency-key",
                "key",
            ],
        )
        .0
        .status
        .success()
    );

    rewrite_archive_with_fifo(&artifact, &with_fifo);
    let (output, env) = run_loom(root.path(), &["package", "verify", &with_fifo_arg]);
    assert!(!output.status.success(), "unsupported entry must fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
}

#[cfg(unix)]
#[test]
fn package_plan_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let root = TestDir::new("package-symlink");
    write_fixture_skill(root.path(), "fixflow");
    write_file(&root.path().join("outside.txt"), "outside\n");
    fs::create_dir_all(root.path().join("skills/fixflow/references"))
        .expect("create references dir");
    symlink(
        root.path().join("outside.txt"),
        root.path().join("skills/fixflow/references/outside.txt"),
    )
    .expect("create symlink");

    let (output, env) = run_loom(
        root.path(),
        &[
            "package",
            "plan",
            "fixflow",
            "--format",
            "agent-skills-archive",
        ],
    );
    assert!(!output.status.success(), "symlink source must fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
}

fn rewrite_archive_with_tampered_skill(src: &Path, dst: &Path) {
    let file = File::open(src).expect("open src archive");
    let mut archive = Archive::new(file);
    let out = File::create(dst).expect("create dst archive");
    let mut builder = Builder::new(out);
    for entry in archive.entries().expect("entries") {
        let mut entry = entry.expect("entry");
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().expect("path").into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read entry");
        if path.to_string_lossy().ends_with("SKILL.md") {
            bytes.extend_from_slice(b"\nTampered after checksum.\n");
        }
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, path, &bytes[..])
            .expect("append entry");
    }
    builder.finish().expect("finish archive");
}

fn rewrite_archive_with_tampered_skill_and_checksums(src: &Path, dst: &Path) {
    let mut entries = read_archive_entries(src);
    for (path, bytes) in &mut entries {
        if path.to_string_lossy().ends_with("SKILL.md") {
            bytes.extend_from_slice(b"\nTampered but rechecksummed.\n");
        }
    }
    let checksums = entries
        .iter()
        .filter(|(path, _)| archive_rel(path) != "checksums.txt")
        .map(|(path, bytes)| format!("{}  {}\n", digest_bytes(bytes), archive_rel(path)))
        .collect::<String>();
    for (path, bytes) in &mut entries {
        if archive_rel(path) == "checksums.txt" {
            *bytes = checksums.clone().into_bytes();
        }
    }
    write_archive_entries(dst, entries);
}

fn rewrite_archive_with_fifo(src: &Path, dst: &Path) {
    let entries = read_archive_entries(src);
    let root = entries
        .first()
        .and_then(|(path, _)| path.components().next())
        .expect("archive root")
        .as_os_str()
        .to_owned();
    let out = File::create(dst).expect("create dst archive");
    let mut builder = Builder::new(out);
    append_entries(&mut builder, entries);
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Fifo);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_size(0);
    header.set_cksum();
    builder
        .append_data(
            &mut header,
            Path::new(&root).join("unsafe-fifo"),
            std::io::empty(),
        )
        .expect("append fifo");
    builder.finish().expect("finish archive");
}

fn read_archive_entries(src: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let file = File::open(src).expect("open src archive");
    let mut archive = Archive::new(file);
    let mut entries = Vec::new();
    for entry in archive.entries().expect("entries") {
        let mut entry = entry.expect("entry");
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().expect("path").into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read entry");
        entries.push((path, bytes));
    }
    entries
}

fn write_archive_entries(dst: &Path, entries: Vec<(std::path::PathBuf, Vec<u8>)>) {
    let out = File::create(dst).expect("create dst archive");
    let mut builder = Builder::new(out);
    append_entries(&mut builder, entries);
    builder.finish().expect("finish archive");
}

fn append_entries(builder: &mut Builder<File>, entries: Vec<(std::path::PathBuf, Vec<u8>)>) {
    for (path, bytes) in entries {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, path, &bytes[..])
            .expect("append entry");
    }
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = sha256::Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", sha256::to_hex(&hasher.finalize()))
}

fn archive_rel(path: &Path) -> String {
    path.components()
        .skip(1)
        .collect::<std::path::PathBuf>()
        .to_string_lossy()
        .replace('\\', "/")
}
