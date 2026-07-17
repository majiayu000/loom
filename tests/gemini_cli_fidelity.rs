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

fn trust_workspace(home: &Path, workspace: &Path) {
    write_file(
        &home.join(".gemini/trustedFolders.json"),
        &format!(
            "{{{:?}:\"TRUST_FOLDER\"}}\n",
            workspace.display().to_string()
        ),
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

fn frontmatter_check_ok(envelope: &Value) -> bool {
    envelope["data"]["checks"]
        .as_array()
        .expect("visibility checks")
        .iter()
        .find(|candidate| {
            candidate["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("gemini-cli_frontmatter_valid:"))
        })
        .unwrap_or_else(|| panic!("missing Gemini frontmatter check: {envelope}"))["ok"]
        == true
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
fn trusted_runtime_dotenv_home_drives_roots_but_not_bootstrap_config() {
    let root = TestDir::new("gemini-trusted-dotenv-root");
    let os_home = TestDir::new("gemini-trusted-dotenv-os-home");
    let gemini_home = TestDir::new("gemini-trusted-dotenv-runtime-home");
    write_good_skill(root.path());
    write_file(
        &os_home.path().join(".gemini/trustedFolders.json"),
        &format!(
            "{{{:?}:\"TRUST_FOLDER\"}}\n",
            root.path().display().to_string()
        ),
    );
    write_file(
        &root.path().join(".gemini/.env"),
        &format!("GEMINI_CLI_HOME={}\n", gemini_home.path().display()),
    );
    write_file(
        &os_home.path().join(".gemini/settings.json"),
        "{\"skills\":{\"disabled\":[\"demo\"]},\"advanced\":{\"ignoreLocalEnv\":true,\"excludedEnvVars\":[\"GEMINI_CLI_HOME\"]}}\n",
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
        "trusted runtime activation failed: {activate_envelope}"
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
            "trusted runtime home missing from status: {adapter}"
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
        "user settings must remain anchored to the bootstrap home"
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
fn generic_dotenv_redirect_requires_effective_settings_invariant_to_cli_ignore_env() {
    let root = TestDir::new("gemini-unknown-cli-root");
    let home = TestDir::new("gemini-unknown-cli-home");
    let attacker = TestDir::new("gemini-unknown-cli-attacker");
    write_good_skill(root.path());
    trust_workspace(home.path(), root.path());
    write_file(
        &root.path().join(".env"),
        &format!("GEMINI_CLI_HOME={}\n", attacker.path().display()),
    );
    let (output, envelope) = run(
        root.path(),
        home.path(),
        root.path(),
        &[],
        &["skill", "activate", "demo", "--agent", "gemini-cli"],
    );
    assert!(output.status.success(), "activation failed: {envelope}");
    assert_eq!(
        envelope["data"]["target"]["path"],
        json!(home.path().join(".gemini/skills")),
        "generic project dotenv differs under Gemini --ignore-env and must not redirect"
    );

    for scope in ["user", "workspace", "system"] {
        let root = TestDir::new("gemini-ignore-local-root");
        let home = TestDir::new("gemini-ignore-local-home");
        let local = TestDir::new("gemini-ignore-local-attacker");
        let stable = TestDir::new("gemini-ignore-local-stable");
        write_good_skill(root.path());
        trust_workspace(home.path(), root.path());
        write_file(
            &root.path().join(".env"),
            &format!("GEMINI_CLI_HOME={}\n", local.path().display()),
        );
        write_file(
            &home.path().join(".env"),
            &format!("GEMINI_CLI_HOME={}\n", stable.path().display()),
        );
        let settings = "{\"advanced\":{\"ignoreLocalEnv\":true}}\n";
        let system_path = root.path().join("system-settings.json");
        match scope {
            "user" => write_file(&home.path().join(".gemini/settings.json"), settings),
            "workspace" => write_file(&root.path().join(".gemini/settings.json"), settings),
            "system" => write_file(&system_path, settings),
            _ => unreachable!(),
        }
        let system_path_arg = system_path.display().to_string();
        let envs = (scope == "system")
            .then_some(("GEMINI_CLI_SYSTEM_SETTINGS_PATH", system_path_arg.as_str()))
            .into_iter()
            .collect::<Vec<_>>();
        let (output, envelope) = run(
            root.path(),
            home.path(),
            root.path(),
            &envs,
            &["skill", "activate", "demo", "--agent", "gemini-cli"],
        );
        assert!(
            output.status.success(),
            "{scope} activation failed: {envelope}"
        );
        assert_eq!(
            envelope["data"]["target"]["path"],
            json!(stable.path().join(".gemini/skills")),
            "{scope} ignoreLocalEnv was not effective"
        );
    }
}

#[test]
fn excluded_env_vars_union_blocks_home_dotenv_redirect_in_every_settings_layer() {
    for scope in ["user", "workspace", "system"] {
        let root = TestDir::new("gemini-excluded-root");
        let home = TestDir::new("gemini-excluded-home");
        let attacker = TestDir::new("gemini-excluded-attacker");
        write_good_skill(root.path());
        trust_workspace(home.path(), root.path());
        write_file(
            &home.path().join(".env"),
            &format!("GEMINI_CLI_HOME={}\n", attacker.path().display()),
        );
        let settings = "{\"advanced\":{\"excludedEnvVars\":[\"GEMINI_CLI_HOME\"]}}\n";
        let system_path = root.path().join("system-settings.json");
        match scope {
            "user" => write_file(&home.path().join(".gemini/settings.json"), settings),
            "workspace" => write_file(&root.path().join(".gemini/settings.json"), settings),
            "system" => write_file(&system_path, settings),
            _ => unreachable!(),
        }
        let system_path_arg = system_path.display().to_string();
        let envs = (scope == "system")
            .then_some(("GEMINI_CLI_SYSTEM_SETTINGS_PATH", system_path_arg.as_str()))
            .into_iter()
            .collect::<Vec<_>>();
        let (output, envelope) = run(
            root.path(),
            home.path(),
            root.path(),
            &envs,
            &["skill", "activate", "demo", "--agent", "gemini-cli"],
        );
        assert!(
            output.status.success(),
            "{scope} activation failed: {envelope}"
        );
        assert_eq!(
            envelope["data"]["target"]["path"],
            json!(home.path().join(".gemini/skills")),
            "{scope} excludedEnvVars union did not block GEMINI_CLI_HOME"
        );
    }
}

#[test]
fn malformed_bootstrap_settings_or_trust_blocks_activation_and_marks_diagnostics_unavailable() {
    for malformed in ["settings", "trust"] {
        let root = TestDir::new("gemini-malformed-root");
        let home = TestDir::new("gemini-malformed-home");
        write_good_skill(root.path());
        let path = match malformed {
            "settings" => home.path().join(".gemini/settings.json"),
            "trust" => home.path().join(".gemini/trustedFolders.json"),
            _ => unreachable!(),
        };
        write_file(&path, "{ malformed\n");
        let (output, envelope) = run(
            root.path(),
            home.path(),
            root.path(),
            &[],
            &["skill", "activate", "demo", "--agent", "gemini-cli"],
        );
        assert!(
            !output.status.success(),
            "{malformed} must fail: {envelope}"
        );
        assert_eq!(envelope["error"]["code"], "ADAPTER_INVALID");
        assert!(!home.path().join(".gemini/skills/demo").exists());

        let (status, status_envelope) = run(
            root.path(),
            home.path(),
            root.path(),
            &[],
            &["workspace", "status"],
        );
        assert!(status.status.success(), "status failed: {status_envelope}");
        let adapter = status_envelope["data"]["agent_adapters"]["adapters"]
            .as_array()
            .expect("adapters")
            .iter()
            .find(|adapter| adapter["id"] == "gemini-cli")
            .expect("Gemini adapter");
        assert!(
            adapter["default_skill_dirs"]
                .as_array()
                .is_some_and(Vec::is_empty)
        );
        assert!(
            adapter["discovery_roots"]
                .as_array()
                .expect("roots")
                .iter()
                .filter(|root| root["scope"] == "user")
                .all(|root| root["available"] == false
                    && root["unavailable_reason"]
                        .as_str()
                        .is_some_and(|reason| reason.contains("runtime home unavailable")))
        );
    }
}

#[test]
fn untrusted_dotenv_home_cannot_redirect_roots_settings_or_trust() {
    let root = TestDir::new("gemini-untrusted-dotenv-root");
    let home = TestDir::new("gemini-untrusted-dotenv-home");
    let attacker_home = TestDir::new("gemini-untrusted-dotenv-attacker");
    write_good_skill(root.path());
    write_file(
        &root.path().join(".env"),
        &format!("GEMINI_CLI_HOME={}\n", attacker_home.path().display()),
    );
    write_file(
        &attacker_home.path().join(".gemini/settings.json"),
        "{\"skills\":{\"disabled\":[\"demo\"]}}\n",
    );
    write_file(
        &attacker_home.path().join(".gemini/trustedFolders.json"),
        &format!(
            "{{{:?}:\"TRUST_FOLDER\"}}\n",
            root.path().display().to_string()
        ),
    );

    let (output, envelope) = run(
        root.path(),
        home.path(),
        root.path(),
        &[],
        &["skill", "activate", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        output.status.success(),
        "untrusted activation failed: {envelope}"
    );
    assert_eq!(
        envelope["data"]["target"]["path"],
        json!(home.path().join(".gemini/skills")),
        "untrusted dotenv must not redirect the user root"
    );

    let project_root = TestDir::new("gemini-untrusted-project-root");
    write_good_skill(project_root.path());
    write_file(
        &project_root.path().join(".env"),
        &format!("GEMINI_CLI_HOME={}\n", attacker_home.path().display()),
    );
    write_file(
        &attacker_home.path().join(".gemini/trustedFolders.json"),
        &format!(
            "{{{:?}:\"TRUST_FOLDER\"}}\n",
            project_root.path().display().to_string()
        ),
    );
    let workspace_arg = project_root.path().display().to_string();
    let (project_output, project_envelope) = run(
        project_root.path(),
        home.path(),
        project_root.path(),
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
        project_output.status.success(),
        "project activation failed: {project_envelope}"
    );
    let (visibility_output, visibility_envelope) = run(
        project_root.path(),
        home.path(),
        project_root.path(),
        &[],
        &[
            "skill",
            "visibility",
            "demo",
            "--agent",
            "gemini-cli",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert!(
        visibility_output.status.success(),
        "visibility failed: {visibility_envelope}"
    );
    assert_eq!(
        visibility_check(&visibility_envelope, "gemini-cli_skill_not_disabled")["ok"],
        true
    );
    assert_eq!(
        visibility_check(&visibility_envelope, "gemini-cli_workspace_trusted")["ok"],
        false
    );
}

#[test]
fn visibility_uses_cwd_secure_trust_bootstrap_and_valid_gemini_frontmatter() {
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

    let trust_workspace = [("GEMINI_CLI_TRUST_WORKSPACE", "true")];
    for (label, body, expected) in [
        (
            "YAML parse errors use Gemini's text fallback",
            "---\nname: demo\ndescription: Use when: reviewing code\n---\n# Demo\n",
            true,
        ),
        (
            "frontmatter names are case insensitive",
            "---\nname: DeMo\ndescription: Reviewing code\n---\n# Demo\n",
            true,
        ),
        (
            "parsed empty descriptions do not use the fallback",
            "---\nname: demo\ndescription:\n---\n# Demo\n",
            false,
        ),
        (
            "parsed non-string descriptions do not use the fallback",
            "---\nname: demo\ndescription: [reviewing, code]\n---\n# Demo\n",
            false,
        ),
        (
            "unclosed frontmatter does not pass the fallback",
            "---\nname: demo\ndescription: reviewing code\n# missing close\n",
            false,
        ),
    ] {
        write_file(&root.path().join("skills/demo/SKILL.md"), body);
        let (output, envelope) = run(
            root.path(),
            home.path(),
            workspace.path(),
            &trust_workspace,
            &["skill", "visibility", "demo", "--agent", "gemini-cli"],
        );
        assert!(output.status.success(), "{label}: {envelope}");
        assert_eq!(frontmatter_check_ok(&envelope), expected, "{label}");
    }
}

#[test]
fn visibility_sanitizes_every_gemini_frontmatter_filename_character() {
    for character in [':', '\\', '/', '<', '>', '*', '?', '"', '|'] {
        let root = TestDir::new("gemini-frontmatter-sanitize-root");
        let home = TestDir::new("gemini-frontmatter-sanitize-home");
        write_skill(
            root.path(),
            "de-mo",
            &format!(
                "---\nname: 'de{character}mo'\ndescription: Gemini sanitization fixture.\n---\n# Demo\n"
            ),
        );
        let (activate_output, activate_envelope) = run(
            root.path(),
            home.path(),
            root.path(),
            &[],
            &["skill", "activate", "de-mo", "--agent", "gemini-cli"],
        );
        assert!(
            activate_output.status.success(),
            "activation failed for {character:?}: {activate_envelope}"
        );
        let (output, envelope) = run(
            root.path(),
            home.path(),
            root.path(),
            &[],
            &["skill", "visibility", "de-mo", "--agent", "gemini-cli"],
        );
        assert!(output.status.success(), "visibility failed: {envelope}");
        assert!(
            envelope["data"]["checks"]
                .as_array()
                .expect("checks")
                .iter()
                .any(|check| check["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("gemini-cli_frontmatter_valid:"))
                    && check["ok"] == true),
            "Gemini character {character:?} was not sanitized: {envelope}"
        );
    }
}
