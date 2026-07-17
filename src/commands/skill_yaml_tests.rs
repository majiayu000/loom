use super::{SkillYaml, parse_skill_yaml};

fn mapping(value: &SkillYaml) -> &std::collections::BTreeMap<String, SkillYaml> {
    match value {
        SkillYaml::Mapping(mapping) => mapping,
        other => panic!("expected mapping, got {other:?}"),
    }
}

#[test]
fn parses_supported_scalars_quotes_and_comments() {
    let parsed = parse_skill_yaml(concat!(
        "name: demo # comment\n",
        "description: 'It''s a # literal'\n",
        "escaped: \"line\\nnext\"\n",
        "enabled: true\n",
        "count: 7\n",
        "ratio: 1.25\n",
        "empty: null\n",
    ))
    .expect("supported scalar document");
    let root = mapping(&parsed);
    assert_eq!(root["name"], SkillYaml::String("demo".to_string()));
    assert_eq!(
        root["description"],
        SkillYaml::String("It's a # literal".to_string())
    );
    assert_eq!(root["escaped"], SkillYaml::String("line\nnext".to_string()));
    assert_eq!(root["enabled"], SkillYaml::Bool(true));
    assert_eq!(root["count"], SkillYaml::Integer(7));
    assert_eq!(root["ratio"], SkillYaml::Real("1.25".to_string()));
    assert_eq!(root["empty"], SkillYaml::Null);
}

#[test]
fn parses_block_and_flow_collections() {
    let parsed = parse_skill_yaml(concat!(
        "metadata:\n",
        "  owner:\n",
        "    team: platform\n",
        "tools:\n",
        "  - Bash\n",
        "  - Read\n",
        "compatibility: { runtimes: [codex, claude], stable: true }\n",
    ))
    .expect("supported collection document");
    let root = mapping(&parsed);
    let metadata = mapping(&root["metadata"]);
    assert_eq!(
        mapping(&metadata["owner"])["team"],
        SkillYaml::String("platform".to_string())
    );
    assert_eq!(
        root["tools"],
        SkillYaml::Sequence(vec![
            SkillYaml::String("Bash".to_string()),
            SkillYaml::String("Read".to_string()),
        ])
    );
    let compatibility = mapping(&root["compatibility"]);
    assert_eq!(compatibility["stable"], SkillYaml::Bool(true));
    assert_eq!(
        compatibility["runtimes"],
        SkillYaml::Sequence(vec![
            SkillYaml::String("codex".to_string()),
            SkillYaml::String("claude".to_string()),
        ])
    );
}

#[test]
fn parses_folded_literal_and_quoted_multiline_values() {
    let parsed = parse_skill_yaml(concat!(
        "folded: >-\n",
        "  first line\n",
        "  second line\n",
        "literal: |-\n",
        "  first line\n",
        "  second line\n",
        "quoted: 'first line\n",
        "  second line'\n",
    ))
    .expect("supported multiline document");
    let root = mapping(&parsed);
    assert_eq!(
        root["folded"],
        SkillYaml::String("first line second line".to_string())
    );
    assert_eq!(
        root["literal"],
        SkillYaml::String("first line\nsecond line".to_string())
    );
    assert_eq!(
        root["quoted"],
        SkillYaml::String("first line second line".to_string())
    );
}

#[test]
fn rejects_unsupported_or_ambiguous_yaml() {
    let cases = [
        ("duplicate", "name: one\nname: two\n"),
        ("alias", "name: *shared\n"),
        ("anchor", "name: &shared demo\n"),
        ("tag", "name: !custom demo\n"),
        ("tab indentation", "metadata:\n\towner: team\n"),
        ("non-string key", "7: value\n"),
        ("ambiguous colon", "description: needs: quoting\n"),
        ("unterminated flow", "tools: [Bash, Read\n"),
        ("explicit block indent", "description: >2\n  text\n"),
    ];
    for (label, raw) in cases {
        assert!(parse_skill_yaml(raw).is_err(), "{label} must be rejected");
    }
}

#[test]
fn supports_indentless_sequences_used_by_skill_frontmatter() {
    let parsed = parse_skill_yaml("allowed-tools:\n- Read\n- Write\nmetadata:\n  owner: core\n")
        .expect("indentless sequence");
    let root = mapping(&parsed);
    assert_eq!(
        root["allowed-tools"],
        SkillYaml::Sequence(vec![
            SkillYaml::String("Read".to_string()),
            SkillYaml::String("Write".to_string()),
        ])
    );
    assert_eq!(
        mapping(&root["metadata"])["owner"],
        SkillYaml::String("core".to_string())
    );
}
