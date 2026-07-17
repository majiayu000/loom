use std::fs;

use chrono::{Duration, Utc};
use serde_json::{Value, json};

mod common;

use common::{TestDir, run_loom, write_file, write_skill};

fn timestamp(days_ago: i64) -> String {
    (Utc::now() - Duration::days(days_ago)).to_rfc3339()
}

fn event(
    id: &str,
    kind: &str,
    skill_id: Option<&str>,
    observed_name: Option<&str>,
    agent: Option<&str>,
    days_ago: i64,
    failure_category: Option<&str>,
) -> String {
    let mut value = json!({
        "schema_version": 3,
        "event_id": id,
        "event_type": kind,
        "timestamp": timestamp(days_ago),
        "metrics": {},
        "privacy": {
            "raw_prompt_stored": false,
            "raw_code_stored": false,
            "redacted": true
        }
    });
    if let Some(skill_id) = skill_id {
        value["skill_id"] = json!(skill_id);
    }
    if let Some(observed_name) = observed_name {
        value["observed_skill_name"] = json!(observed_name);
    }
    if let Some(agent) = agent {
        value["agent"] = json!(agent);
    }
    if let Some(category) = failure_category {
        value["metrics"]["failure_category"] = json!(category);
    }
    value.to_string()
}

fn setup_registry(root: &TestDir) {
    for skill in [
        "active",
        "zombie",
        "unbound-unused",
        "unbound-used",
        "multi",
        "error-only",
    ] {
        write_skill(
            root.path(),
            skill,
            &format!(
                "---\nname: {skill}\ndescription: Use when testing skill stats.\n---\n# {skill}\n"
            ),
        );
    }
    let registry = root.path().join("state/registry");
    write_file(
        &registry.join("schema.json"),
        r#"{"schema_version":1,"created_at":"2026-04-09T10:00:00Z","writer":"test"}
"#,
    );
    write_file(
        &registry.join("targets.json"),
        &json!({
            "schema_version": 1,
            "targets": [
                {"target_id":"target_codex","agent":"codex","path":"/tmp/codex/skills","ownership":"managed","capabilities":{"symlink":true,"copy":true,"watch":true},"created_at":"2026-04-09T10:00:00Z"},
                {"target_id":"target_claude","agent":"claude","path":"/tmp/claude/skills","ownership":"managed","capabilities":{"symlink":true,"copy":true,"watch":true},"created_at":"2026-04-09T10:00:00Z"}
            ]
        })
        .to_string(),
    );
    write_file(
        &registry.join("bindings.json"),
        &json!({
            "schema_version": 1,
            "bindings": [
                {"binding_id":"bind_codex","agent":"codex","profile_id":"default","workspace_matcher":{"kind":"path_prefix","value":"/tmp"},"default_target_id":"target_codex","policy_profile":"safe-capture","active":true,"created_at":"2026-04-09T10:00:00Z"},
                {"binding_id":"bind_claude","agent":"claude","profile_id":"default","workspace_matcher":{"kind":"path_prefix","value":"/tmp"},"default_target_id":"target_claude","policy_profile":"safe-capture","active":true,"created_at":"2026-04-09T10:00:00Z"}
            ]
        })
        .to_string(),
    );
    let rules = [
        ("bind_codex", "active", "target_codex"),
        ("bind_codex", "zombie", "target_codex"),
        ("bind_codex", "multi", "target_codex"),
        ("bind_claude", "multi", "target_claude"),
        ("bind_codex", "error-only", "target_codex"),
    ]
    .into_iter()
    .map(|(binding_id, skill_id, target_id)| {
        json!({"binding_id":binding_id,"skill_id":skill_id,"target_id":target_id,"method":"symlink","watch_policy":"observe_only","created_at":"2026-04-09T10:00:00Z"})
    })
    .collect::<Vec<_>>();
    write_file(
        &registry.join("rules.json"),
        &json!({"schema_version":1,"rules":rules}).to_string(),
    );
    write_file(
        &registry.join("projections.json"),
        r#"{"schema_version":1,"projections":[]}
"#,
    );
    write_file(
        &registry.join("ops/checkpoint.json"),
        r#"{"schema_version":1,"last_scanned_op_id":null,"last_acked_op_id":null,"updated_at":"2026-04-09T10:00:00Z"}
"#,
    );
    write_file(&registry.join("ops/operations.jsonl"), "");
}

fn write_default_events(root: &TestDir) {
    let rows = [
        event(
            "evt_active",
            "skill.invocation",
            Some("active"),
            None,
            Some("codex"),
            1,
            None,
        ),
        event(
            "evt_zombie",
            "skill.invocation",
            Some("zombie"),
            None,
            Some("codex"),
            60,
            None,
        ),
        event(
            "evt_unbound",
            "skill.invocation",
            Some("unbound-used"),
            None,
            Some("codex"),
            2,
            None,
        ),
        event(
            "evt_multi",
            "skill.invocation",
            Some("multi"),
            None,
            Some("codex"),
            1,
            None,
        ),
        event(
            "evt_error",
            "skill.error",
            Some("error-only"),
            None,
            Some("codex"),
            1,
            Some("timeout"),
        ),
        event(
            "evt_orphan",
            "skill.invocation",
            None,
            Some("retired"),
            Some("codex"),
            1,
            None,
        ),
        event(
            "evt_deleted",
            "skill.error",
            Some("deleted"),
            None,
            Some("claude"),
            1,
            Some("tool_error"),
        ),
    ];
    write_file(
        &root.path().join("state/telemetry/events.jsonl"),
        &(rows.join("\n") + "\n"),
    );
}

fn report(root: &TestDir, extra: &[&str]) -> Value {
    let mut args = vec!["skill", "stats"];
    args.extend_from_slice(extra);
    let (output, envelope) = run_loom(root.path(), &args);
    assert!(output.status.success(), "stats failed: {envelope}");
    envelope["data"].clone()
}

fn skill<'a>(data: &'a Value, name: &str) -> &'a Value {
    data["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["skill"] == name)
        .unwrap_or_else(|| panic!("missing skill {name}: {data}"))
}

#[test]
fn command_is_read_only_and_linear() {
    let root = TestDir::new("skill-stats-read-only");
    setup_registry(&root);
    write_default_events(&root);
    let registry_before = fs::read(root.path().join("state/registry/rules.json")).unwrap();
    let events_before = fs::read(root.path().join("state/telemetry/events.jsonl")).unwrap();
    let data = report(&root, &[]);
    assert_eq!(data["window_events"], 7);
    assert_eq!(
        fs::read(root.path().join("state/registry/rules.json")).unwrap(),
        registry_before
    );
    assert_eq!(
        fs::read(root.path().join("state/telemetry/events.jsonl")).unwrap(),
        events_before
    );
    assert!(!root.path().join("state/events/commands.jsonl").exists());
}

#[test]
fn current_snapshot_ignores_stale_binding_observations() {
    let root = TestDir::new("skill-stats-stale-observation");
    setup_registry(&root);
    write_file(
        &root.path().join("state/registry/observations/old.jsonl"),
        r#"{"skill_id":"unbound-unused","agent":"codex","health":"healthy"}
"#,
    );
    let data = report(&root, &[]);
    assert_eq!(skill(&data, "unbound-unused")["category"], "unbound_unused");
    assert_eq!(skill(&data, "zombie")["category"], "zombie");
}

#[test]
fn stats_reads_one_locked_snapshot() {
    let root = TestDir::new("skill-stats-lock");
    setup_registry(&root);
    write_file(
        &root.path().join("state/locks/workspace.lock"),
        &json!({"pid":std::process::id(),"owner_id":"test-owner","host":"","created_at":Utc::now()}).to_string(),
    );
    let (output, envelope) = run_loom(root.path(), &["skill", "stats"]);
    assert!(!output.status.success());
    assert_eq!(envelope["error"]["code"], "LOCK_BUSY");
}

#[test]
fn missing_registry_fails_instead_of_reporting_source_only_lifecycle() {
    let root = TestDir::new("skill-stats-missing-registry");
    write_skill(
        root.path(),
        "source-only",
        "---\nname: source-only\ndescription: Use when testing missing registry state.\n---\n",
    );
    let (output, envelope) = run_loom(root.path(), &["skill", "stats"]);
    assert!(!output.status.success());
    assert_eq!(envelope["error"]["code"], "STATE_NOT_INITIALIZED");
}

#[test]
fn rule_referencing_missing_binding_fails_closed() {
    let root = TestDir::new("skill-stats-missing-binding");
    setup_registry(&root);
    write_file(
        &root.path().join("state/registry/rules.json"),
        &json!({
            "schema_version": 1,
            "rules": [{
                "binding_id": "missing",
                "skill_id": "active",
                "target_id": "target_codex",
                "method": "symlink",
                "watch_policy": "observe_only",
                "created_at": "2026-04-09T10:00:00Z"
            }]
        })
        .to_string(),
    );
    let (output, envelope) = run_loom(root.path(), &["skill", "stats"]);
    assert!(!output.status.success());
    assert_eq!(envelope["error"]["code"], "STATE_CORRUPT");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("missing binding"))
    );
}

#[test]
fn lifecycle_categories_are_exhaustive() {
    let root = TestDir::new("skill-stats-categories");
    setup_registry(&root);
    write_default_events(&root);
    let data = report(&root, &[]);
    assert_eq!(skill(&data, "active")["category"], "active");
    assert_eq!(skill(&data, "zombie")["category"], "zombie");
    assert_eq!(skill(&data, "unbound-used")["category"], "unbound_but_used");
    assert_eq!(skill(&data, "unbound-unused")["category"], "unbound_unused");
}

#[test]
fn unbound_category_is_independent_from_since() {
    let root = TestDir::new("skill-stats-unbound-since");
    setup_registry(&root);
    write_default_events(&root);
    let future = (Utc::now() + Duration::days(1)).to_rfc3339();
    let data = report(&root, &["--since", &future]);
    assert_eq!(skill(&data, "unbound-used")["category"], "unbound_but_used");
    assert_eq!(skill(&data, "unbound-used")["attempt_count"], 0);
}

#[test]
fn zombie_cutoff_is_independent_from_since() {
    let root = TestDir::new("skill-stats-zombie-cutoff");
    setup_registry(&root);
    write_default_events(&root);
    let broad_since = (Utc::now() - Duration::days(90)).to_rfc3339();
    let data = report(&root, &["--since", &broad_since, "--zombie-days", "30"]);
    assert_eq!(skill(&data, "zombie")["attempt_count"], 1);
    assert_eq!(skill(&data, "zombie")["category"], "zombie");
}

#[test]
fn agent_filter_scopes_bindings_but_single_runtime_is_global() {
    let root = TestDir::new("skill-stats-agent-filter");
    setup_registry(&root);
    write_default_events(&root);
    let data = report(&root, &["--agent", "claude"]);
    assert_eq!(skill(&data, "active")["category"], "unbound_unused");
    assert_eq!(skill(&data, "multi")["category"], "zombie");
    assert_eq!(skill(&data, "multi")["single_runtime"], true);
    assert_eq!(data["single_runtime_scope"], "all_agents");
}

#[test]
fn agentless_events_are_unfiltered_only() {
    let root = TestDir::new("skill-stats-agentless");
    setup_registry(&root);
    write_file(
        &root.path().join("state/telemetry/events.jsonl"),
        &(event(
            "evt_agentless",
            "skill.invocation",
            Some("active"),
            None,
            None,
            1,
            None,
        ) + "\n"),
    );
    let all = report(&root, &[]);
    assert_eq!(skill(&all, "active")["category"], "active");
    assert_eq!(all["agentless"]["attempt_count"], 1);
    assert!(
        skill(&all, "active")["by_agent"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let filtered = report(&root, &["--agent", "codex"]);
    assert_eq!(skill(&filtered, "active")["category"], "zombie");
    assert_eq!(filtered["window_events"], 0);
}

#[test]
fn error_events_count_as_recent_lifecycle_usage() {
    let root = TestDir::new("skill-stats-error-lifecycle");
    setup_registry(&root);
    write_default_events(&root);
    let data = report(&root, &[]);
    let row = skill(&data, "error-only");
    assert_eq!(row["category"], "active");
    assert_eq!(row["error_count"], 1);
    assert_eq!(row["failure_categories"]["timeout"], 1);
}

#[test]
fn durable_unmatched_events_become_orphans() {
    let root = TestDir::new("skill-stats-orphans");
    setup_registry(&root);
    write_default_events(&root);
    let data = report(&root, &[]);
    let names = data["orphans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(names.contains(&"retired"));
    assert!(names.contains(&"deleted"));
}

#[test]
fn metadata_only_deleted_skill_remains_an_orphan() {
    let root = TestDir::new("skill-stats-metadata-only-orphan");
    setup_registry(&root);
    write_file(
        &root.path().join("state/registry/trust.json"),
        r#"{"schema_version":1,"skills":[{"skill_id":"deleted","trust":"local-draft","quarantined":false,"updated_at":"2026-07-01T00:00:00Z","updated_by":"test"}]}
"#,
    );
    write_file(
        &root.path().join("state/telemetry/events.jsonl"),
        &(event(
            "evt_deleted_metadata",
            "skill.invocation",
            Some("deleted"),
            None,
            Some("codex"),
            1,
            None,
        ) + "\n"),
    );
    let data = report(&root, &[]);
    assert!(
        data["skills"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["skill"] != "deleted")
    );
    assert_eq!(data["orphans"][0]["name"], "deleted");
}

#[test]
fn invalid_telemetry_skill_id_is_unattributed_and_not_exposed() {
    let root = TestDir::new("skill-stats-invalid-telemetry-skill");
    setup_registry(&root);
    write_file(
        &root.path().join("state/telemetry/events.jsonl"),
        &(event(
            "evt_invalid_skill",
            "skill.invocation",
            Some("../escape"),
            None,
            Some("codex"),
            1,
            None,
        ) + "\n"),
    );
    let data = report(&root, &[]);
    assert_eq!(data["unattributed_window_events"], 1);
    assert!(data["orphans"].as_array().unwrap().is_empty());
    assert!(!data.to_string().contains("../escape"));
}

#[test]
fn orphans_share_since_and_agent_window() {
    let root = TestDir::new("skill-stats-orphan-window");
    setup_registry(&root);
    write_default_events(&root);
    let data = report(&root, &["--agent", "codex", "--since", &timestamp(3)]);
    let orphans = data["orphans"].as_array().unwrap();
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0]["name"], "retired");
    assert_eq!(orphans[0]["agent"], "codex");
}

#[test]
fn window_totals_reconcile_with_skill_and_orphan_attempts() {
    let root = TestDir::new("skill-stats-reconcile");
    setup_registry(&root);
    write_default_events(&root);
    let data = report(&root, &[]);
    let skills = data["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["attempt_count"].as_u64().unwrap())
        .sum::<u64>();
    let orphans = data["orphans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["attempt_count"].as_u64().unwrap())
        .sum::<u64>();
    assert_eq!(
        data["window_events"].as_u64().unwrap(),
        skills + orphans + data["unattributed_window_events"].as_u64().unwrap()
    );
}

#[test]
fn error_threshold_is_five() {
    let root = TestDir::new("skill-stats-threshold");
    setup_registry(&root);
    let rows = (0..5)
        .map(|index| {
            event(
                &format!("evt_{index}"),
                if index == 4 {
                    "skill.error"
                } else {
                    "skill.invocation"
                },
                Some("active"),
                None,
                Some("codex"),
                1,
                (index == 4).then_some("timeout"),
            )
        })
        .collect::<Vec<_>>();
    write_file(
        &root.path().join("state/telemetry/events.jsonl"),
        &(rows.join("\n") + "\n"),
    );
    let data = report(&root, &[]);
    assert_eq!(skill(&data, "active")["error_sample_size"], 5);
    assert_eq!(skill(&data, "active")["error_rate"], 0.2);
}

#[test]
fn ordering_is_stable() {
    let root = TestDir::new("skill-stats-ordering");
    setup_registry(&root);
    write_default_events(&root);
    let first = report(&root, &[])["skills"].clone();
    let second = report(&root, &[])["skills"].clone();
    assert_eq!(first, second);
    let names = first
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["skill"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(
        names.iter().position(|name| *name == "zombie").unwrap()
            < names
                .iter()
                .position(|name| *name == "unbound-unused")
                .unwrap()
    );
}

#[test]
fn unbound_but_used_sort_is_stable() {
    let root = TestDir::new("skill-stats-unbound-ordering");
    setup_registry(&root);
    write_default_events(&root);
    let data = report(&root, &[]);
    let names = data["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["skill"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(
        names
            .iter()
            .position(|name| *name == "unbound-used")
            .unwrap()
            < names.iter().position(|name| *name == "zombie").unwrap()
    );
}

#[test]
fn disabled_with_history_is_not_empty() {
    let root = TestDir::new("skill-stats-disabled-history");
    setup_registry(&root);
    write_default_events(&root);
    write_file(
        &root.path().join("state/telemetry/config.json"),
        r#"{"schema_version":1,"enabled":false,"mode":"local-only","redaction":"default","retention_days":90}
"#,
    );
    let data = report(&root, &[]);
    assert_eq!(data["telemetry_enabled"], false);
    assert_eq!(data["telemetry_empty"], false);
    assert_eq!(data["window_events"], 7);
}

#[test]
fn empty_and_error_contracts_are_explicit() {
    let root = TestDir::new("skill-stats-empty");
    setup_registry(&root);
    let data = report(&root, &[]);
    assert_eq!(data["telemetry_empty"], true);
    assert_eq!(data["window_events"], 0);
    assert!(data["orphans"].as_array().unwrap().is_empty());
    write_file(
        &root.path().join("state/telemetry/config.json"),
        "not-json\n",
    );
    let (output, error) = run_loom(root.path(), &["skill", "stats"]);
    assert!(!output.status.success());
    assert_eq!(error["error"]["code"], "SCHEMA_MISMATCH");

    let malformed = TestDir::new("skill-stats-all-malformed");
    setup_registry(&malformed);
    write_file(
        &malformed.path().join("state/telemetry/events.jsonl"),
        "not-json\n",
    );
    let malformed_data = report(&malformed, &[]);
    assert_eq!(malformed_data["telemetry_empty"], true);
    assert_eq!(malformed_data["persisted_events"], 0);
    assert_eq!(malformed_data["malformed_events"], 1);
}

#[test]
fn orphan_and_error_threshold_contract() {
    let root = TestDir::new("skill-stats-orphan-error-contract");
    setup_registry(&root);
    let rows = (0..4)
        .map(|index| {
            event(
                &format!("evt_o_{index}"),
                "skill.error",
                Some("retired"),
                None,
                Some("codex"),
                1,
                Some("timeout"),
            )
        })
        .collect::<Vec<_>>();
    write_file(
        &root.path().join("state/telemetry/events.jsonl"),
        &(rows.join("\n") + "\n"),
    );
    let data = report(&root, &[]);
    let orphan = &data["orphans"][0];
    assert_eq!(orphan["attempt_count"], 4);
    assert!(orphan["error_rate"].is_null());
    assert_eq!(
        orphan["failure_categories"]["timeout"], 4,
        "orphan={orphan}"
    );
}

#[test]
fn contract_surface_matches() {
    let contract = include_str!("../docs/LOOM_CLI_CONTRACT.md");
    for field in [
        "skill stats",
        "single_runtime_scope",
        "window_events",
        "unattributed_window_events",
        "failure_categories",
    ] {
        assert!(contract.contains(field), "contract missing {field}");
    }
}
