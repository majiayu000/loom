mod common;

use std::fs;

use common::{TestDir, run_loom, write_file};
use serde_json::{Value, json};

fn provider_ids(env: &Value) -> Vec<String> {
    let mut ids: Vec<String> = env["data"]["providers"]
        .as_array()
        .expect("provider array")
        .iter()
        .map(|provider| provider["id"].as_str().expect("provider id").to_string())
        .collect();
    ids.sort();
    ids
}

#[test]
fn provider_config_search_show_and_remove_are_deterministic() {
    let root = TestDir::new("provider-config-root");
    let catalog = TestDir::new("provider-config-catalog");
    write_file(
        &catalog.path().join("skills/alpha/SKILL.md"),
        "---\nname: alpha\ndescription: Alpha helper for catalog search.\nlicense: MIT\n---\n# Alpha\n",
    );
    write_file(
        &catalog.path().join("skills/beta/SKILL.md"),
        "---\nname: beta\ndescription: Beta helper.\n---\n# Beta\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "provider",
            "add",
            "corp-local",
            "--kind",
            "local",
            "--url",
            catalog.path().to_str().expect("catalog path"),
        ],
    );
    assert!(output.status.success(), "provider add should pass: {env}");
    assert_eq!(env["data"]["provider"]["id"], json!("corp-local"));

    let (output, list) = run_loom(root.path(), &["provider", "list"]);
    assert!(output.status.success(), "provider list should pass: {list}");
    assert_eq!(provider_ids(&list), vec!["corp-local", "github", "local"]);
    assert_eq!(list["data"]["count"], json!(3));

    let providers_before = fs::read_to_string(root.path().join("state/registry/providers.json"))
        .expect("read providers before");
    let (output, search) = run_loom(
        root.path(),
        &["catalog", "search", "alpha", "--provider", "corp-local"],
    );
    assert!(
        output.status.success(),
        "catalog search should pass: {search}"
    );
    assert_eq!(search["data"]["provider"], json!("corp-local"));
    assert_eq!(
        search["data"]["results"].as_array().expect("results").len(),
        1
    );
    let result = &search["data"]["results"][0];
    assert_eq!(result["name"], json!("alpha"));
    assert_eq!(result["source"]["provider"], json!("corp-local"));
    assert!(
        result["locator"]
            .as_str()
            .expect("locator")
            .starts_with("corp-local:"),
        "custom provider search must return the selected provider id: {search}"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("state/registry/providers.json"))
            .expect("read providers after search"),
        providers_before,
        "catalog search must not mutate provider state"
    );

    let locator = result["locator"].as_str().expect("locator");
    let (output, show) = run_loom(root.path(), &["catalog", "show", locator]);
    assert!(output.status.success(), "catalog show should pass: {show}");
    assert_eq!(show["data"]["result"]["name"], json!("alpha"));
    assert_eq!(
        show["data"]["result"]["source"]["provider"],
        json!("corp-local")
    );

    let (output, removed) = run_loom(root.path(), &["provider", "remove", "corp-local"]);
    assert!(
        output.status.success(),
        "provider remove should pass: {removed}"
    );
    assert_eq!(removed["data"]["provider_id"], json!("corp-local"));

    let (output, list) = run_loom(root.path(), &["provider", "list"]);
    assert!(output.status.success(), "provider list should pass: {list}");
    assert_eq!(provider_ids(&list), vec!["github", "local"]);

    let (output, env) = run_loom(root.path(), &["provider", "remove", "local"]);
    assert!(
        !output.status.success(),
        "built-in provider remove must fail: {env}"
    );
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
}

#[test]
fn provider_state_parse_errors_fail_without_overwrite() {
    let root = TestDir::new("provider-malformed-state");
    let providers_path = root.path().join("state/registry/providers.json");
    fs::create_dir_all(providers_path.parent().expect("provider state parent"))
        .expect("create provider state parent");
    write_file(&providers_path, "{ not json\n");
    let before = fs::read_to_string(&providers_path).expect("read malformed providers");

    let (output, env) = run_loom(root.path(), &["provider", "list"]);
    assert!(
        !output.status.success(),
        "provider list must fail on malformed state: {env}"
    );
    assert_eq!(env["error"]["code"], json!("STATE_CORRUPT"));
    assert_eq!(
        fs::read_to_string(&providers_path).expect("read providers after list"),
        before,
        "read failure must not overwrite malformed provider state"
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "provider",
            "add",
            "corp-local",
            "--kind",
            "local",
            "--url",
            root.path().to_str().expect("root path"),
        ],
    );
    assert!(
        !output.status.success(),
        "provider add must fail on malformed state: {env}"
    );
    assert_eq!(env["error"]["code"], json!("STATE_CORRUPT"));
    assert_eq!(
        fs::read_to_string(&providers_path).expect("read providers after add"),
        before,
        "write failure must preserve malformed provider state for manual repair"
    );
}
