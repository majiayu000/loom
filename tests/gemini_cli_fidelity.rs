mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{TestDir, write_file, write_skill};
use serde_json::{Value, json};

fn write_good_skill(root: &Path) {
    write_skill(
        root,
        "demo",
        "---\nname: demo\ndescription: Use when testing Gemini CLI fidelity.\n---\n# Demo\n",
    );
}

fn symlink_dir(source: &Path, destination: &Path) {
    fs::create_dir_all(destination.parent().expect("symlink parent"))
        .expect("create symlink parent");
    #[cfg(unix)]
    std::os::unix::fs::symlink(source, destination).expect("symlink directory");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(source, destination).expect("symlink directory");
}

fn run(
    root: &Path,
    home: &Path,
    current_dir: &Path,
    envs: &[(&str, &str)],
    args: &[&str],
) -> (std::process::Output, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
    command
        .arg("--json")
        .arg("--root")
        .arg(root)
        .args(args)
        .current_dir(current_dir)
        .env("HOME", home);
    for key in [
        "GEMINI_CLI_HOME",
        "GEMINI_CLI_SKILLS_DIR",
        "GEMINI_CLI_SYSTEM_DEFAULTS_PATH",
        "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
        "GEMINI_CLI_TRUSTED_FOLDERS_PATH",
        "GEMINI_CLI_TRUST_WORKSPACE",
    ] {
        command.env_remove(key);
    }
    command.env(
        "GEMINI_CLI_SYSTEM_DEFAULTS_PATH",
        home.join("missing-system-defaults.json"),
    );
    command.env(
        "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
        home.join("missing-system-settings.json"),
    );
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output().expect("run loom");
    let envelope = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse loom JSON: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output, envelope)
}

fn visibility_check<'a>(envelope: &'a Value, id: &str) -> &'a Value {
    envelope["data"]["checks"]
        .as_array()
        .expect("visibility checks")
        .iter()
        .find(|candidate| candidate["id"] == id)
        .unwrap_or_else(|| panic!("missing check {id}: {envelope}"))
}

#[test]
fn use_and_activate_share_native_gemini_root_and_reject_alias_shadow() {
    let use_root = TestDir::new("gemini-native-use-root");
    let activate_root = TestDir::new("gemini-native-activate-root");
    let home = TestDir::new("gemini-native-home");
    let use_workspace = TestDir::new("gemini-native-use-workspace");
    let activate_workspace = TestDir::new("gemini-native-activate-workspace");
    write_good_skill(use_root.path());
    write_good_skill(activate_root.path());

    let use_workspace_arg = use_workspace.path().display().to_string();
    let (use_output, use_envelope) = run(
        use_root.path(),
        home.path(),
        use_root.path(),
        &[],
        &[
            "use",
            "demo",
            "--agents",
            "gemini-cli",
            "--scope",
            "project",
            "--workspace",
            &use_workspace_arg,
        ],
    );
    assert!(
        use_output.status.success(),
        "use plan failed: {use_envelope}"
    );
    assert_eq!(
        use_envelope["data"]["steps"][0]["target_path"],
        json!(use_workspace.path().join(".gemini/skills"))
    );

    let activate_workspace_arg = activate_workspace.path().display().to_string();
    let (activate_output, activate_envelope) = run(
        activate_root.path(),
        home.path(),
        activate_root.path(),
        &[],
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "gemini-cli",
            "--scope",
            "project",
            "--workspace",
            &activate_workspace_arg,
            "--dry-run",
        ],
    );
    assert!(
        activate_output.status.success(),
        "activation plan failed: {activate_envelope}"
    );
    assert_eq!(
        activate_envelope["data"]["plan"]["target_path"],
        json!(
            activate_workspace
                .path()
                .canonicalize()
                .expect("canonical activation workspace")
                .join(".gemini/skills")
        )
    );

    let shadow = activate_workspace.path().join(".agents/skills/demo");
    symlink_dir(&activate_root.path().join("skills/demo"), &shadow);
    let (same_source_output, same_source_envelope) = run(
        activate_root.path(),
        home.path(),
        activate_root.path(),
        &[],
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "gemini-cli",
            "--scope",
            "project",
            "--workspace",
            &activate_workspace_arg,
            "--dry-run",
        ],
    );
    assert!(
        same_source_output.status.success(),
        "same-source alias is safe: {same_source_envelope}"
    );
    fs::remove_file(&shadow).expect("remove same-source alias");
    fs::create_dir_all(&shadow).expect("create higher-priority Gemini alias");
    write_file(&shadow.join("SKILL.md"), "# shadow\n");
    let (shadow_output, shadow_envelope) = run(
        activate_root.path(),
        home.path(),
        activate_root.path(),
        &[],
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "gemini-cli",
            "--scope",
            "project",
            "--workspace",
            &activate_workspace_arg,
            "--dry-run",
        ],
    );
    assert!(!shadow_output.status.success(), "shadow must fail closed");
    assert_eq!(shadow_envelope["error"]["code"], "POLICY_BLOCKED");
    assert_eq!(
        shadow_envelope["error"]["details"]["reason"],
        "gemini_alias_shadows_native_projection"
    );
    assert!(
        !activate_workspace
            .path()
            .join(".gemini/skills/demo")
            .exists(),
        "shadow check must run before native projection writes"
    );
}

#[test]
fn dotenv_gemini_home_drives_activation_status_and_config() {
    let root = TestDir::new("gemini-dotenv-home-root");
    let os_home = TestDir::new("gemini-dotenv-home-os");
    let gemini_home = TestDir::new("gemini-dotenv-home-effective");
    write_good_skill(root.path());
    write_file(
        &root.path().join(".env"),
        &format!("GEMINI_CLI_HOME={}\n", gemini_home.path().display()),
    );
    write_file(
        &gemini_home.path().join(".gemini/settings.json"),
        "{\"skills\":{\"disabled\":[\"demo\"]}}\n",
    );

    let (activate_output, activate_envelope) = run(
        root.path(),
        os_home.path(),
        root.path(),
        &[],
        &["skill", "activate", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        activate_output.status.success(),
        "dotenv-home activation failed: {activate_envelope}"
    );
    assert_eq!(
        activate_envelope["data"]["target"]["path"],
        json!(gemini_home.path().join(".gemini/skills"))
    );
    assert!(gemini_home.path().join(".gemini/skills/demo").exists());
    assert!(!os_home.path().join(".gemini/skills/demo").exists());

    let (status_output, status_envelope) = run(
        root.path(),
        os_home.path(),
        root.path(),
        &[],
        &["workspace", "status"],
    );
    assert!(
        status_output.status.success(),
        "status failed: {status_envelope}"
    );
    let adapter = status_envelope["data"]["agent_adapters"]["adapters"]
        .as_array()
        .expect("adapters")
        .iter()
        .find(|candidate| candidate["id"] == "gemini-cli")
        .expect("Gemini adapter");
    for suffix in [".agents/skills", ".gemini/skills"] {
        assert!(
            adapter["default_skill_dirs"]
                .as_array()
                .expect("default dirs")
                .iter()
                .any(|path| path == &json!(gemini_home.path().join(suffix))),
            "dotenv Gemini home missing from status: {adapter}"
        );
    }

    let (visibility_output, visibility_envelope) = run(
        root.path(),
        os_home.path(),
        root.path(),
        &[],
        &["skill", "visibility", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        visibility_output.status.success(),
        "visibility failed: {visibility_envelope}"
    );
    assert_eq!(
        visibility_check(&visibility_envelope, "gemini-cli_skill_not_disabled")["ok"],
        false,
        "visibility must read settings from dotenv Gemini home"
    );

    let process_root = TestDir::new("gemini-process-home-root");
    let process_home = TestDir::new("gemini-process-home-effective");
    write_good_skill(process_root.path());
    write_file(
        &process_root.path().join(".env"),
        &format!("GEMINI_CLI_HOME={}\n", gemini_home.path().display()),
    );
    let process_home_arg = process_home.path().display().to_string();
    let (process_output, process_envelope) = run(
        process_root.path(),
        os_home.path(),
        process_root.path(),
        &[("GEMINI_CLI_HOME", &process_home_arg)],
        &["skill", "activate", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        process_output.status.success(),
        "process-home activation failed: {process_envelope}"
    );
    assert_eq!(
        process_envelope["data"]["target"]["path"],
        json!(process_home.path().join(".gemini/skills")),
        "process environment must take precedence over repo dotenv"
    );
}

#[test]
fn gemini_home_dotenv_precedes_explicit_root_when_cwd_is_outside_home() {
    let root = TestDir::new("gemini-home-fallback-root");
    let home = TestDir::new("gemini-home-fallback-home");
    let current_dir = TestDir::new("gemini-home-fallback-cwd");
    let home_effective = TestDir::new("gemini-home-fallback-effective");
    let root_effective = TestDir::new("gemini-root-fallback-effective");
    write_good_skill(root.path());
    write_file(
        &home.path().join(".env"),
        &format!("GEMINI_CLI_HOME={}\n", home_effective.path().display()),
    );
    write_file(
        &root.path().join(".env"),
        &format!("GEMINI_CLI_HOME={}\n", root_effective.path().display()),
    );

    let (output, envelope) = run(
        root.path(),
        home.path(),
        current_dir.path(),
        &[],
        &["skill", "activate", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        output.status.success(),
        "HOME dotenv activation failed: {envelope}"
    );
    assert_eq!(
        envelope["data"]["target"]["path"],
        json!(home_effective.path().join(".gemini/skills")),
        "HOME dotenv must precede the explicit root fallback"
    );
}

#[test]
fn visibility_uses_cwd_secure_trust_bootstrap_and_gemini_frontmatter_fallback() {
    let root = TestDir::new("gemini-visibility-root");
    let home = TestDir::new("gemini-visibility-home");
    let workspace = TestDir::new("gemini-visibility-workspace");
    let trust_file = root.path().join("custom-trusted-folders.json");
    write_good_skill(root.path());
    write_file(
        &trust_file,
        &format!(
            "{{{:?}:\"TRUST_FOLDER\"}}\n",
            workspace.path().display().to_string()
        ),
    );
    write_file(
        &root.path().join(".env"),
        &format!(
            "GEMINI_CLI_TRUST_WORKSPACE=true\nGEMINI_CLI_TRUSTED_FOLDERS_PATH={}\n",
            trust_file.display()
        ),
    );
    let workspace_arg = workspace.path().display().to_string();
    let (activate_output, activate_envelope) = run(
        root.path(),
        home.path(),
        root.path(),
        &[],
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "gemini-cli",
            "--scope",
            "project",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert!(
        activate_output.status.success(),
        "project activation failed: {activate_envelope}"
    );

    let (dotenv_output, dotenv_envelope) = run(
        root.path(),
        home.path(),
        workspace.path(),
        &[],
        &["skill", "visibility", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        dotenv_output.status.success(),
        "dotenv visibility failed: {dotenv_envelope}"
    );
    assert_eq!(
        visibility_check(&dotenv_envelope, "gemini-cli_workspace_trusted")["ok"],
        false,
        "repo dotenv must not self-authorize workspace trust"
    );

    let trust_file_arg = trust_file.display().to_string();
    let (cwd_output, cwd_envelope) = run(
        root.path(),
        home.path(),
        workspace.path(),
        &[("GEMINI_CLI_TRUSTED_FOLDERS_PATH", &trust_file_arg)],
        &["skill", "visibility", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        cwd_output.status.success(),
        "cwd visibility failed: {cwd_envelope}"
    );
    assert_eq!(
        visibility_check(&cwd_envelope, "gemini-cli_workspace_trusted")["ok"],
        true,
        "omitted --workspace must use cwd and the process trust-file override"
    );

    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription:\n  Use when: reviewing code\n---\n# Demo\n",
    );
    let trust_workspace = [("GEMINI_CLI_TRUST_WORKSPACE", "true")];
    let (fallback_output, fallback_envelope) = run(
        root.path(),
        home.path(),
        workspace.path(),
        &trust_workspace,
        &["skill", "visibility", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        fallback_output.status.success(),
        "fallback visibility failed: {fallback_envelope}"
    );
    assert_eq!(
        visibility_check(&fallback_envelope, "gemini-cli_workspace_trusted")["ok"],
        true,
        "process GEMINI_CLI_TRUST_WORKSPACE must be honored"
    );
    assert!(
        fallback_envelope["data"]["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .any(|candidate| candidate["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("gemini-cli_frontmatter_valid:"))
                && candidate["ok"] == true),
        "Gemini's multiline description fallback must remain visible"
    );

    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription:\n  Use when: reviewing code\n# missing close\n",
    );
    let (unclosed_output, unclosed_envelope) = run(
        root.path(),
        home.path(),
        workspace.path(),
        &trust_workspace,
        &["skill", "visibility", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        unclosed_output.status.success(),
        "unclosed report failed: {unclosed_envelope}"
    );
    assert!(
        unclosed_envelope["data"]["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .any(|candidate| candidate["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("gemini-cli_frontmatter_valid:"))
                && candidate["ok"] == false),
        "unclosed frontmatter must not pass Gemini fallback"
    );
}
