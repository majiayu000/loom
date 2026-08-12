use super::*;
use serde_json::{Value, json};

#[test]
fn registry_batch_journal_completes_all_files_before_a_read() {
    let root = std::env::temp_dir().join(format!("loom-json-batch-{}", uuid::Uuid::new_v4()));
    let registry = root.join("state/registry");
    fs::create_dir_all(&registry).expect("create registry fixture");
    let first = registry.join("bindings.json");
    let second = registry.join("rules.json");
    fs::write(&first, "{\"generation\":0}\n").expect("write first preimage");
    fs::write(&second, "{\"generation\":0}\n").expect("write second preimage");
    let journal = BatchJournal {
        version: 1,
        entries: vec![
            BatchJournalEntry {
                target: "bindings.json".into(),
                contents: "{\"generation\":1}\n".into(),
            },
            BatchJournalEntry {
                target: "rules.json".into(),
                contents: "{\"generation\":1}\n".into(),
            },
        ],
    };
    fs::write(
        registry.join(BATCH_JOURNAL_FILE),
        serde_json::to_vec(&journal).expect("serialize batch journal"),
    )
    .expect("write batch journal");

    assert_eq!(
        read_json_file::<Value>(&first).unwrap(),
        json!({"generation": 1})
    );
    assert_eq!(
        read_json_file::<Value>(&second).unwrap(),
        json!({"generation": 1})
    );
    assert!(!registry.join(BATCH_JOURNAL_FILE).exists());
    fs::remove_dir_all(root).expect("remove batch fixture");
}

#[test]
fn torn_jsonl_tail_is_ignored_and_repaired_before_append() {
    let root = std::env::temp_dir().join(format!("loom-jsonl-tail-{}", uuid::Uuid::new_v4()));
    let path = root.join("state/registry/ops/operations.jsonl");
    fs::create_dir_all(path.parent().unwrap()).expect("create JSONL fixture");
    fs::write(&path, b"{\"id\":1}\n{\"id\":").expect("write torn JSONL");

    assert_eq!(
        read_json_lines::<Value>(&path).unwrap(),
        vec![json!({"id": 1})]
    );
    append_json_line(&path, &json!({"id": 2})).expect("append after torn tail");
    assert_eq!(
        read_json_lines::<Value>(&path).unwrap(),
        vec![json!({"id": 1}), json!({"id": 2})]
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "{\"id\":1}\n{\"id\":2}\n"
    );
    fs::remove_dir_all(root).expect("remove JSONL fixture");
}

#[test]
fn json_compare_exchange_installs_only_over_the_reviewed_value() {
    let root = std::env::temp_dir().join(format!("loom-json-cas-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create CAS fixture");
    let path = root.join("state.json");
    let reviewed = json!({"value": "reviewed"});
    let replacement = json!({"value": "replacement"});
    let external = json!({"value": "external"});
    write_json_file(&path, &reviewed).expect("write reviewed value");

    assert!(compare_exchange_json_file(&path, &reviewed, &replacement).expect("matching CAS"));
    assert_eq!(read_json_file::<Value>(&path).unwrap(), replacement);

    let candidate = path.with_extension("loom-cas-candidate");
    let journal = path.with_extension("loom-cas-journal");
    assert!(
        compare_exchange_json_file(&path, &replacement, &replacement).expect("semantic no-op CAS")
    );
    assert_eq!(read_json_file::<Value>(&path).unwrap(), replacement);
    assert!(!candidate.exists());
    assert!(!journal.exists());

    fs::write(&candidate, b"untracked\n").expect("write stray CAS candidate");
    assert!(
        compare_exchange_json_file(&path, &replacement, &replacement).is_err(),
        "semantic no-op must not hide untracked CAS evidence"
    );
    assert!(candidate.exists());
    fs::remove_file(&candidate).expect("remove stray candidate");

    write_json_file(&path, &external).expect("write external value");
    assert!(!compare_exchange_json_file(&path, &reviewed, &replacement).expect("mismatching CAS"));
    assert_eq!(read_json_file::<Value>(&path).unwrap(), external);
    fs::remove_dir_all(root).expect("remove CAS fixture");
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn cas_recovery_handles_each_unix_crash_boundary() {
    let root = std::env::temp_dir().join(format!("loom-json-cas-restore-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create CAS restore fixture");
    let path = root.join("state.json");
    let expected = serialize_json_file(&json!({"value": "reviewed"})).unwrap();
    let replacement = serialize_json_file(&json!({"value": "replacement"})).unwrap();
    let external = serialize_json_file(&json!({"value": "external"})).unwrap();
    let candidate = path.with_extension("loom-cas-candidate");
    let journal = path.with_extension("loom-cas-journal");

    let stage = |live: &str, staged: Option<&str>| {
        fs::write(&path, live).unwrap();
        fs::write(
            &journal,
            encode_cas_journal(0, expected.as_bytes(), replacement.as_bytes()),
        )
        .unwrap();
        if let Some(staged) = staged {
            fs::write(&candidate, staged).unwrap();
        }
    };

    stage(&expected, Some(&replacement));
    assert_eq!(
        read_json_file::<Value>(&path).unwrap(),
        json!({"value": "reviewed"})
    );
    assert!(!journal.exists() && !candidate.exists());

    stage(&replacement, Some(&expected));
    assert_eq!(
        read_json_file::<Value>(&path).unwrap(),
        json!({"value": "replacement"})
    );
    assert!(!journal.exists() && !candidate.exists());

    stage(&replacement, Some(&external));
    let retained_journal = fs::read(&journal).unwrap();
    let error = read_json_file::<Value>(&path).expect_err("unknown candidate must fail closed");
    assert!(error.to_string().contains("ambiguous JSON CAS retained"));
    assert_eq!(fs::read(&path).unwrap(), replacement.as_bytes());
    assert_eq!(fs::read(&candidate).unwrap(), external.as_bytes());
    assert_eq!(fs::read(&journal).unwrap(), retained_journal);
    assert!(read_json_file::<Value>(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), replacement.as_bytes());
    assert_eq!(fs::read(&candidate).unwrap(), external.as_bytes());
    assert_eq!(fs::read(&journal).unwrap(), retained_journal);

    fs::remove_file(&candidate).unwrap();
    fs::remove_file(&journal).unwrap();

    stage(&external, Some(&expected));
    let error = read_json_file::<Value>(&path).expect_err("ambiguous evidence must fail closed");
    assert!(error.to_string().contains("ambiguous JSON CAS retained"));
    assert_eq!(fs::read(&path).unwrap(), external.as_bytes());
    assert_eq!(fs::read(&candidate).unwrap(), expected.as_bytes());
    assert!(journal.exists());

    fs::remove_file(&candidate).unwrap();
    fs::write(
        &journal,
        encode_cas_journal(1, expected.as_bytes(), replacement.as_bytes()),
    )
    .unwrap();
    fs::write(&path, &replacement).unwrap();
    assert!(read_json_file::<Value>(&path).is_ok());
    assert!(!journal.exists());

    fs::write(
        &journal,
        encode_cas_journal(2, expected.as_bytes(), replacement.as_bytes()),
    )
    .unwrap();
    fs::write(&path, &external).unwrap();
    assert!(read_json_file::<Value>(&path).is_ok());
    assert!(!journal.exists());
    fs::remove_dir_all(root).expect("remove CAS restore fixture");
}

#[cfg(windows)]
#[test]
fn cas_recovery_retains_untrusted_windows_backup() {
    let root = std::env::temp_dir().join(format!("loom-json-cas-restore-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create CAS restore fixture");
    let path = root.join("state.json");
    let expected = serialize_json_file(&json!({"value": "reviewed"})).unwrap();
    let replacement = serialize_json_file(&json!({"value": "replacement"})).unwrap();
    let external = serialize_json_file(&json!({"value": "external"})).unwrap();
    let candidate = path.with_extension("loom-cas-candidate");
    let backup = path.with_extension("loom-cas-backup");
    let journal = path.with_extension("loom-cas-journal");

    fs::write(&path, &replacement).unwrap();
    fs::write(&backup, &external).unwrap();
    fs::write(
        &journal,
        encode_cas_journal(0, expected.as_bytes(), replacement.as_bytes()),
    )
    .unwrap();
    let retained_journal = fs::read(&journal).unwrap();

    for _ in 0..2 {
        let error = read_json_file::<Value>(&path).expect_err("untrusted backup must fail closed");
        assert!(error.to_string().contains("ambiguous JSON CAS retained"));
        assert_eq!(fs::read(&path).unwrap(), replacement.as_bytes());
        assert!(!candidate.exists());
        assert_eq!(fs::read(&backup).unwrap(), external.as_bytes());
        assert_eq!(fs::read(&journal).unwrap(), retained_journal);
    }

    fs::remove_dir_all(root).expect("remove CAS restore fixture");
}

#[test]
fn corrupt_cas_journal_is_retained_and_blocks_reads() {
    let root = std::env::temp_dir().join(format!("loom-json-cas-bad-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("state.json");
    fs::write(&path, "{}\n").unwrap();
    let journal = path.with_extension("loom-cas-journal");
    fs::write(&journal, b"partial").unwrap();

    let error = read_json_file::<Value>(&path).expect_err("corrupt journal must fail closed");
    assert!(error.to_string().contains("invalid JSON CAS journal"));
    assert_eq!(fs::read(&path).unwrap(), b"{}\n");
    assert_eq!(fs::read(&journal).unwrap(), b"partial");
    fs::remove_dir_all(root).unwrap();
}
