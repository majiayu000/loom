use serde::Serialize;

use crate::commands::CommandFailure;
use crate::commands::helpers::map_io;
use crate::sha256::{Sha256, to_hex};

pub(super) fn digest_json<T: Serialize>(value: &T) -> std::result::Result<String, CommandFailure> {
    let raw = serde_json::to_vec(value).map_err(map_io)?;
    Ok(digest_bytes(&raw))
}

pub(super) fn digest_str(raw: &str) -> String {
    digest_bytes(raw.as_bytes())
}

pub(super) fn digest_bytes(raw: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(raw);
    format!("sha256:{}", to_hex(&hash.finalize()))
}
