use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};

use super::SkillLintFrontmatter;
use crate::commands::skill_yaml::{SkillYaml, parse_skill_yaml};

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

    let value = parse_skill_yaml(&yaml)?;
    let SkillYaml::Mapping(mapping) = &value else {
        return Err("frontmatter root must be a YAML mapping".to_string());
    };

    let mut frontmatter = SkillLintFrontmatter {
        present: true,
        parsed: true,
        ..SkillLintFrontmatter::default()
    };
    let mut issues = Vec::new();

    for (key, value) in mapping {
        if !valid_key(key) {
            issues.push(schema_issue(
                "frontmatter_key_invalid",
                "frontmatter key uses unsupported characters",
                "use alphanumeric, hyphen, or underscore keys",
                json!({ "key": key }),
            ));
            continue;
        }
        match key.as_str() {
            "name" => frontmatter.name = parse_portable_string(key, value, &mut issues),
            "description" => {
                frontmatter.description = parse_portable_string(key, value, &mut issues)
            }
            "license" => frontmatter.license = parse_optional_string(key, value, &mut issues),
            "allowed-tools" => {
                frontmatter.allowed_tools = parse_allowed_tools(value, &mut issues);
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

fn parse_allowed_tools(
    value: &SkillYaml,
    issues: &mut Vec<FrontmatterSchemaIssue>,
) -> Option<Value> {
    match value {
        SkillYaml::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                push_allowed_tools_issue(value, issues);
                None
            } else {
                Some(Value::String(trimmed.to_string()))
            }
        }
        SkillYaml::Sequence(items) if items.is_empty() => {
            push_allowed_tools_issue(value, issues);
            None
        }
        SkillYaml::Sequence(items) => {
            let tools = items
                .iter()
                .map(|item| match item {
                    SkillYaml::String(text) if !text.trim().is_empty() => {
                        Some(Value::String(text.trim().to_string()))
                    }
                    _ => None,
                })
                .collect::<Option<Vec<_>>>();
            match tools {
                Some(tools) => Some(Value::Array(tools)),
                None => {
                    push_allowed_tools_issue(value, issues);
                    None
                }
            }
        }
        _ => {
            push_allowed_tools_issue(value, issues);
            None
        }
    }
}

fn push_allowed_tools_issue(value: &SkillYaml, issues: &mut Vec<FrontmatterSchemaIssue>) {
    issues.push(schema_issue(
        "frontmatter_allowed_tools_invalid",
        "allowed-tools must be a non-empty string or a sequence of non-empty strings",
        "use a space-separated string or an agent-supported YAML string sequence",
        json!({ "field": "allowed-tools", "actual": yaml_summary(value) }),
    ));
}

fn parse_portable_string(
    key: &str,
    value: &SkillYaml,
    issues: &mut Vec<FrontmatterSchemaIssue>,
) -> Option<String> {
    match value {
        SkillYaml::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        _ => {
            issues.push(schema_issue(
                "frontmatter_scalar_expected",
                "frontmatter field must be a scalar string",
                "replace the field value with a YAML string",
                json!({ "field": key, "actual": yaml_summary(value) }),
            ));
            None
        }
    }
}

fn parse_optional_string(
    key: &str,
    value: &SkillYaml,
    issues: &mut Vec<FrontmatterSchemaIssue>,
) -> Option<String> {
    match value {
        SkillYaml::Null => None,
        SkillYaml::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        SkillYaml::Integer(number) => Some(number.to_string()),
        SkillYaml::Real(number) => Some(number.to_string()),
        SkillYaml::Bool(flag) => Some(flag.to_string()),
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
    value: &SkillYaml,
    issues: &mut Vec<FrontmatterSchemaIssue>,
) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();
    let SkillYaml::Mapping(mapping) = value else {
        issues.push(schema_issue(
            "frontmatter_metadata_invalid",
            "metadata frontmatter must be a string map",
            "use metadata keys with scalar string values",
            json!({ "actual": yaml_summary(value) }),
        ));
        return metadata;
    };
    for (key, value) in mapping {
        parse_metadata_entry(key, value, &mut metadata, issues);
    }
    metadata
}

fn parse_metadata_entry(
    key: &str,
    value: &SkillYaml,
    metadata: &mut BTreeMap<String, String>,
    issues: &mut Vec<FrontmatterSchemaIssue>,
) {
    if let SkillYaml::Mapping(mapping) = value {
        for (child_key, child_value) in mapping {
            parse_metadata_entry(&format!("{key}.{child_key}"), child_value, metadata, issues);
        }
        return;
    }
    if let Some(value) = parse_optional_string(&format!("metadata.{key}"), value, issues) {
        metadata.insert(key.to_string(), value);
    }
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

fn yaml_to_json(value: &SkillYaml) -> Value {
    match value {
        SkillYaml::Null => Value::Null,
        SkillYaml::Bool(flag) => Value::Bool(*flag),
        SkillYaml::Integer(number) => Value::from(*number),
        SkillYaml::Real(number) => number
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(number.clone())),
        SkillYaml::String(text) => Value::String(text.clone()),
        SkillYaml::Sequence(items) => Value::Array(items.iter().map(yaml_to_json).collect()),
        SkillYaml::Mapping(mapping) => {
            let mut object = Map::new();
            for (key, value) in mapping {
                object.insert(key.clone(), yaml_to_json(value));
            }
            Value::Object(object)
        }
    }
}

fn yaml_summary(value: &SkillYaml) -> String {
    match value {
        SkillYaml::Null => "null".to_string(),
        SkillYaml::Bool(_) => "bool".to_string(),
        SkillYaml::Integer(_) | SkillYaml::Real(_) => "number".to_string(),
        SkillYaml::String(_) => "string".to_string(),
        SkillYaml::Sequence(_) => "sequence".to_string(),
        SkillYaml::Mapping(_) => "mapping".to_string(),
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
