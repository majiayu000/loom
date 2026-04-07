use serde::Serialize;

use crate::types::{ErrorCode, SyncState};

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
}

#[derive(Debug, Serialize, Default)]
pub struct Meta {
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_state: Option<SyncState>,
}

#[derive(Debug, Serialize)]
pub struct Envelope {
    pub ok: bool,
    pub cmd: String,
    pub request_id: String,
    pub version: String,
    pub data: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
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
        Self {
            ok: false,
            cmd: cmd.to_string(),
            request_id,
            version: env!("CARGO_PKG_VERSION").to_string(),
            data: serde_json::json!({}),
            error: Some(ErrorBody {
                code: code.as_str().to_string(),
                message: message.into(),
                details,
            }),
            meta: Meta::default(),
        }
    }
}
