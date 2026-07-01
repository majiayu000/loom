use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::SkillLintFrontmatter;

pub(crate) struct FrontmatterParseResult {
    pub(crate) frontmatter: SkillLintFrontmatter,
    pub(crate) schema_issues: Vec<FrontmatterSchemaIssue>,
}

pub(crate) struct FrontmatterSchemaIssue {
    pub(crate) id: String,
    pub(crate) message: String,
    pub(crate) suggested_action: String,
    pub(crate) details: Value,
}

pub(crate) fn parse_skill_frontmatter(entrypoint: &Path) -> Result<FrontmatterParseResult, String> {
    let raw = fs::read_to_string(entrypoint).map_err(|err| err.to_string())?;
    let mut lines = raw.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Ok(FrontmatterParseResult {
            frontmatter: SkillLintFrontmatter::default(),
            schema_issues: Vec::new(),
        });
    }

    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err("frontmatter is missing a closing --- marker".to_string());
    }

    let value: yaml_serde::Value = yaml_serde::from_str(&yaml).map_err(|err| err.to_string())?;
    let mapping = value
        .as_mapping()
        .ok_or_else(|| "frontmatter root must be a YAML mapping".to_string())?;

    let mut frontmatter = SkillLintFrontmatter {
        present: true,
        parsed: true,
        ..SkillLintFrontmatter::default()
    };
    let mut issues = Vec::new();

    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            issues.push(schema_issue(
                "frontmatter_key_not_string",
                "frontmatter contains a non-string key",
                "use string keys in YAML frontmatter",
                json!({ "key": yaml_summary(key) }),
            ));
            continue;
        };
        if !valid_key(key) {
            issues.push(schema_issue(
                "frontmatter_key_invalid",
                "frontmatter key uses unsupported characters",
                "use alphanumeric, hyphen, or underscore keys",
                json!({ "key": key }),
            ));
            continue;
        }
        match key {
            "name" => frontmatter.name = parse_optional_string(key, value, &mut issues),
            "description" => {
                frontmatter.description = parse_optional_string(key, value, &mut issues)
            }
            "license" => frontmatter.license = parse_optional_string(key, value, &mut issues),
            "allowed-tools" => {
                frontmatter.allowed_tools = parse_optional_string(key, value, &mut issues);
                frontmatter.agent_fields.push(key.to_string());
            }
            "compatibility" => frontmatter.compatibility = Some(yaml_to_json(value)),
            "metadata" => frontmatter.metadata = parse_metadata_map(value, &mut issues),
            _ => {
                if claude_field(key) {
                    frontmatter.agent_fields.push(key.to_string());
                }
            }
        }
    }
    frontmatter.agent_fields.sort();
    frontmatter.agent_fields.dedup();

    Ok(FrontmatterParseResult {
        frontmatter,
        schema_issues: issues,
    })
}

fn parse_optional_string(
    key: &str,
    value: &yaml_serde::Value,
    issues: &mut Vec<FrontmatterSchemaIssue>,
) -> Option<String> {
    match value {
        yaml_serde::Value::Null => None,
        yaml_serde::Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        yaml_serde::Value::Number(number) => Some(number.to_string()),
        yaml_serde::Value::Bool(flag) => Some(flag.to_string()),
        _ => {
            issues.push(schema_issue(
                "frontmatter_scalar_expected",
                "frontmatter field must be a scalar string",
                "replace nested YAML with a scalar string for this field",
                json!({ "field": key, "actual": yaml_summary(value) }),
            ));
            None
        }
    }
}

fn parse_metadata_map(
    value: &yaml_serde::Value,
    issues: &mut Vec<FrontmatterSchemaIssue>,
) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();
    let Some(mapping) = value.as_mapping() else {
        issues.push(schema_issue(
            "frontmatter_metadata_invalid",
            "metadata frontmatter must be a string map",
            "use metadata keys with scalar string values",
            json!({ "actual": yaml_summary(value) }),
        ));
        return metadata;
    };
    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            issues.push(schema_issue(
                "frontmatter_metadata_key_invalid",
                "metadata keys must be strings",
                "use string keys under metadata",
                json!({ "key": yaml_summary(key) }),
            ));
            continue;
        };
        if let Some(value) = parse_optional_string(&format!("metadata.{key}"), value, issues) {
            metadata.insert(key.to_string(), value);
        }
    }
    metadata
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn claude_field(key: &str) -> bool {
    matches!(
        key,
        "allowed-tools"
            | "disallowed-tools"
            | "disable-model-invocation"
            | "user-invocable"
            | "argument-hint"
            | "paths"
            | "model"
            | "effort"
            | "context"
            | "agent"
    )
}

fn yaml_to_json(value: &yaml_serde::Value) -> Value {
    serde_json::to_value(value).unwrap_or_else(|_| json!({ "yaml": yaml_summary(value) }))
}

fn yaml_summary(value: &yaml_serde::Value) -> String {
    match value {
        yaml_serde::Value::Null => "null".to_string(),
        yaml_serde::Value::Bool(_) => "bool".to_string(),
        yaml_serde::Value::Number(_) => "number".to_string(),
        yaml_serde::Value::String(_) => "string".to_string(),
        yaml_serde::Value::Sequence(_) => "sequence".to_string(),
        yaml_serde::Value::Mapping(_) => "mapping".to_string(),
        yaml_serde::Value::Tagged(_) => "tagged".to_string(),
    }
}

fn schema_issue(
    id: &str,
    message: &str,
    suggested_action: &str,
    details: Value,
) -> FrontmatterSchemaIssue {
    FrontmatterSchemaIssue {
        id: id.to_string(),
        message: message.to_string(),
        suggested_action: suggested_action.to_string(),
        details,
    }
}
