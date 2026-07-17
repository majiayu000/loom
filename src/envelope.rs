use serde::Serialize;

use crate::error_actions::{NextAction, default_next_actions};
use crate::types::{ErrorCode, SyncState};

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<NextAction>,
}

#[derive(Debug, Serialize, Default)]
pub struct Meta {
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_state: Option<SyncState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Envelope {
    pub ok: bool,
    pub cmd: String,
    pub request_id: String,
    pub version: String,
    pub cli_contract_version: String,
    pub data: serde_json::Value,
    pub error: Option<ErrorBody>,
    pub meta: Meta,
}

impl Envelope {
    pub fn ok(cmd: &str, request_id: String, data: serde_json::Value, meta: Meta) -> Self {
        Self {
            ok: true,
            cmd: cmd.to_string(),
            request_id,
            version: env!("CARGO_PKG_VERSION").to_string(),
            cli_contract_version: crate::cli_contract::CLI_CONTRACT_VERSION.to_string(),
            data,
            error: None,
            meta,
        }
    }

    pub fn err(
        cmd: &str,
        request_id: String,
        code: ErrorCode,
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self::err_with_next_actions(
            cmd,
            request_id,
            code,
            message,
            details,
            default_next_actions(code.as_str()),
        )
    }

    pub fn err_with_next_actions(
        cmd: &str,
        request_id: String,
        code: ErrorCode,
        message: impl Into<String>,
        details: serde_json::Value,
        next_actions: Vec<NextAction>,
    ) -> Self {
        Self {
            ok: false,
            cmd: cmd.to_string(),
            request_id,
            version: env!("CARGO_PKG_VERSION").to_string(),
            cli_contract_version: crate::cli_contract::CLI_CONTRACT_VERSION.to_string(),
            data: serde_json::json!({}),
            error: Some(ErrorBody {
                code: code.as_str().to_string(),
                message: message.into(),
                details,
                next_actions,
            }),
            meta: Meta::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Envelope;
    use crate::types::ErrorCode;

    #[test]
    fn error_envelope_includes_default_next_actions_for_not_found() {
        let env = Envelope::err(
            "skill.inspect",
            "req-1".to_string(),
            ErrorCode::SkillNotFound,
            "skill 'missing' not found",
            json!({}),
        );
        let value = match serde_json::to_value(env) {
            Ok(value) => value,
            Err(err) => panic!("serialize envelope: {err}"),
        };

        assert_eq!(
            value["error"]["next_actions"][0]["cmd"],
            json!("loom skill list --json")
        );
        assert_eq!(
            value["error"]["next_actions"][0]["reason"],
            json!("list available skills to find a valid skill name")
        );
    }

    #[test]
    fn error_envelope_omits_empty_next_actions() {
        let env = Envelope::err(
            "cli.parse",
            "req-1".to_string(),
            ErrorCode::ArgInvalid,
            "bad arg",
            json!({}),
        );
        let value = match serde_json::to_value(env) {
            Ok(value) => value,
            Err(err) => panic!("serialize envelope: {err}"),
        };

        assert!(value["error"].get("next_actions").is_none());
    }
}
