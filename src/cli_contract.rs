use std::fmt;

pub const CLI_CONTRACT_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContractVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractVersionError(String);

impl fmt::Display for ContractVersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ContractVersionError {}

pub fn parse_contract_version(raw: &str) -> Result<ContractVersion, ContractVersionError> {
    if raw.is_empty() || raw.trim() != raw || raw.contains(['+', '-']) {
        return Err(ContractVersionError(
            "CLI contract version must be a non-empty release SemVer".to_string(),
        ));
    }
    let parts = raw.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(ContractVersionError(
            "CLI contract version must contain major.minor.patch".to_string(),
        ));
    }
    let parse = |part: &str| {
        if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
            return Err(ContractVersionError(
                "CLI contract version components must be canonical integers".to_string(),
            ));
        }
        part.parse::<u64>().map_err(|_| {
            ContractVersionError("CLI contract version components must be integers".to_string())
        })
    };
    Ok(ContractVersion {
        major: parse(parts[0])?,
        minor: parse(parts[1])?,
        patch: parse(parts[2])?,
    })
}

pub fn current_contract_version() -> ContractVersion {
    parse_contract_version(CLI_CONTRACT_VERSION)
        .expect("CLI_CONTRACT_VERSION must remain a valid release SemVer")
}
