use serde_json::{Value, json};

use crate::common::{TestDir, run_loom, write_file};

#[test]
fn normalized_query_overflow_fails_without_partial_output() {
    let event = |id: &str, metrics: Value| {
        json!({
            "schema_version": 2,
            "event_id": id,
            "event_type": "skill.eval",
            "skill_id": "demo",
            "timestamp": "2026-07-01T00:00:00Z",
            "metrics": metrics,
            "privacy": {
                "raw_prompt_stored": false,
                "raw_code_stored": false,
                "redacted": true
            }
        })
    };
    for (case, first, second) in [
        (
            "integer",
            json!({"tokens_in": u64::MAX}),
            json!({"tokens_in": 1}),
        ),
        (
            "float",
            json!({"baseline_delta": 1e308}),
            json!({"baseline_delta": 1e308}),
        ),
    ] {
        let root = TestDir::new(&format!("telemetry-report-{case}-overflow"));
        write_file(
            &root.path().join("state/telemetry/events.jsonl"),
            &format!(
                "{}\n{}\n",
                event("evt_max", first),
                event("evt_one", second)
            ),
        );
        let (output, envelope) = run_loom(root.path(), &["telemetry", "report"]);
        assert!(!output.status.success());
        assert_eq!(envelope["error"]["code"], json!("INTERNAL_ERROR"));
        assert!(
            envelope["data"].get("summary").is_none(),
            "overflow must not return a partial report: {envelope}"
        );
    }
}
