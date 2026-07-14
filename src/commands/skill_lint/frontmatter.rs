use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};
use yaml_rust2::{Yaml, YamlLoader};

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

    let docs = YamlLoader::load_from_str(&yaml).map_err(|err| err.to_string())?;
    let value = docs
        .first()
        .ok_or_else(|| "frontmatter root must be a YAML mapping".to_string())?;
    let Yaml::Hash(mapping) = value else {
        return Err("frontmatter root must be a YAML mapping".to_string());
    };

    let mut frontmatter = SkillLintFrontmatter {
        present: true,
        parsed: true,
        ..SkillLintFrontmatter::default()
    };
    let mut issues = Vec::new();

    for (key, value) in mapping {
        let Some(key) = yaml_as_str(key) else {
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
            "name" => frontmatter.name = parse_portable_string(key, value, &mut issues),
            "description" => {
                frontmatter.description = parse_portable_string(key, value, &mut issues)
            }
            "license" => frontmatter.license = parse_optional_string(key, value, &mut issues),
            "allowed-tools" => {
                frontmatter.allowed_tools = Some(yaml_to_json(value));
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

fn parse_portable_string(
    key: &str,
    value: &Yaml,
    issues: &mut Vec<FrontmatterSchemaIssue>,
) -> Option<String> {
    match value {
        Yaml::String(text) => {
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
    value: &Yaml,
    issues: &mut Vec<FrontmatterSchemaIssue>,
) -> Option<String> {
    match value {
        Yaml::Null | Yaml::BadValue => None,
        Yaml::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Yaml::Integer(number) => Some(number.to_string()),
        Yaml::Real(number) => Some(number.to_string()),
        Yaml::Boolean(flag) => Some(flag.to_string()),
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
    value: &Yaml,
    issues: &mut Vec<FrontmatterSchemaIssue>,
) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();
    let Yaml::Hash(mapping) = value else {
        issues.push(schema_issue(
            "frontmatter_metadata_invalid",
            "metadata frontmatter must be a string map",
            "use metadata keys with scalar string values",
            json!({ "actual": yaml_summary(value) }),
        ));
        return metadata;
    };
    for (key, value) in mapping {
        let Some(key) = yaml_as_str(key) else {
            issues.push(schema_issue(
                "frontmatter_metadata_key_invalid",
                "metadata keys must be strings",
                "use string keys under metadata",
                json!({ "key": yaml_summary(key) }),
            ));
            continue;
        };
        parse_metadata_entry(key, value, &mut metadata, issues);
    }
    metadata
}

fn parse_metadata_entry(
    key: &str,
    value: &Yaml,
    metadata: &mut BTreeMap<String, String>,
    issues: &mut Vec<FrontmatterSchemaIssue>,
) {
    if let Yaml::Hash(mapping) = value {
        for (child_key, child_value) in mapping {
            let Some(child_key) = yaml_as_str(child_key) else {
                issues.push(schema_issue(
                    "frontmatter_metadata_key_invalid",
                    "metadata keys must be strings",
                    "use string keys under metadata",
                    json!({ "key": yaml_summary(child_key) }),
                ));
                continue;
            };
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

fn yaml_as_str(value: &Yaml) -> Option<&str> {
    match value {
        Yaml::String(text) => Some(text),
        _ => None,
    }
}

fn yaml_to_json(value: &Yaml) -> Value {
    match value {
        Yaml::Null | Yaml::BadValue => Value::Null,
        Yaml::Boolean(flag) => Value::Bool(*flag),
        Yaml::Integer(number) => Value::from(*number),
        Yaml::Real(number) => number
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(number.clone())),
        Yaml::String(text) => Value::String(text.clone()),
        Yaml::Array(items) => Value::Array(items.iter().map(yaml_to_json).collect()),
        Yaml::Hash(mapping) => {
            let mut object = Map::new();
            for (key, value) in mapping {
                let key = yaml_as_str(key)
                    .map(str::to_string)
                    .unwrap_or_else(|| yaml_summary(key));
                object.insert(key, yaml_to_json(value));
            }
            Value::Object(object)
        }
        Yaml::Alias(alias) => json!({ "alias": alias }),
    }
}

fn yaml_summary(value: &Yaml) -> String {
    match value {
        Yaml::Null => "null".to_string(),
        Yaml::BadValue => "invalid".to_string(),
        Yaml::Boolean(_) => "bool".to_string(),
        Yaml::Integer(_) | Yaml::Real(_) => "number".to_string(),
        Yaml::String(_) => "string".to_string(),
        Yaml::Array(_) => "sequence".to_string(),
        Yaml::Hash(_) => "mapping".to_string(),
        Yaml::Alias(_) => "alias".to_string(),
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
