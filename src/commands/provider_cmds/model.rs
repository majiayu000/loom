#[path = "install.rs"]
mod install;
#[path = "locator.rs"]
mod locator;
#[path = "store.rs"]
mod store;

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::{InstallTrustArg, ProviderKindArg};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProviderKind {
    Github,
    Local,
}

impl ProviderKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Local => "local",
        }
    }

    pub(crate) fn capabilities(self) -> Vec<String> {
        ["search", "preview", "fetch", "provenance"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    pub(crate) fn requires_network(self) -> bool {
        matches!(self, Self::Github)
    }
}

impl From<ProviderKindArg> for ProviderKind {
    fn from(value: ProviderKindArg) -> Self {
        match value {
            ProviderKindArg::Github => Self::Github,
            ProviderKindArg::Local => Self::Local,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProvidersFile {
    pub schema_version: u32,
    #[serde(default)]
    pub providers: Vec<ProviderRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProviderRecord {
    pub id: String,
    pub kind: ProviderKind,
    pub url: String,
    pub capabilities: Vec<String>,
    pub trust_default: String,
    pub requires_network: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedProvider {
    pub record: ProviderRecord,
    pub persisted: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProviderListItem {
    pub id: String,
    pub kind: ProviderKind,
    pub url: String,
    pub capabilities: Vec<String>,
    pub trust_default: String,
    pub requires_network: bool,
    pub persisted: bool,
}

impl ProviderListItem {
    pub(crate) fn from_resolved(provider: ResolvedProvider) -> Self {
        Self {
            id: provider.record.id,
            kind: provider.record.kind,
            url: provider.record.url,
            capabilities: provider.record.capabilities,
            trust_default: provider.record.trust_default,
            requires_network: provider.record.requires_network,
            persisted: provider.persisted,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedLocator {
    pub raw: String,
    pub provider: ResolvedProvider,
    pub source: LocatorSource,
    pub subdir: String,
    pub requested_ref: Option<String>,
    pub pinned: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum LocatorSource {
    Github {
        repository: String,
        clone_url: String,
    },
    Local {
        base_path: PathBuf,
    },
}

impl ParsedLocator {
    pub(crate) fn provider_id(&self) -> &str {
        &self.provider.record.id
    }

    pub(crate) fn provider_kind(&self) -> ProviderKind {
        self.provider.record.kind
    }

    pub(crate) fn source_path(&self) -> Option<PathBuf> {
        match &self.source {
            LocatorSource::Local { base_path } => {
                if self.subdir.is_empty() {
                    Some(base_path.clone())
                } else {
                    Some(base_path.join(&self.subdir))
                }
            }
            LocatorSource::Github { .. } => None,
        }
    }

    pub(crate) fn source_json(&self) -> Value {
        match &self.source {
            LocatorSource::Github {
                repository,
                clone_url,
            } => json!({
                "provider": self.provider_id(),
                "kind": self.provider_kind().as_str(),
                "repo": repository,
                "clone_url": clone_url,
                "subdir": self.subdir,
                "ref": self.requested_ref,
                "pinned": self.pinned,
            }),
            LocatorSource::Local { base_path } => json!({
                "provider": self.provider_id(),
                "kind": self.provider_kind().as_str(),
                "path": base_path.display().to_string(),
                "subdir": self.subdir,
                "ref": self.requested_ref,
                "pinned": self.pinned,
            }),
        }
    }
}

pub(crate) fn trust_arg_as_str(value: InstallTrustArg) -> &'static str {
    match value {
        InstallTrustArg::ThirdPartyUnreviewed => "third-party-unreviewed",
        InstallTrustArg::Reviewed => "reviewed",
    }
}
