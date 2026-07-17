use std::fmt;

use clap::{
    ArgMatches, Command, CommandFactory, FromArgMatches, error::ErrorKind, parser::ValueSource,
};

use crate::cli::Cli;

mod contract_policy;
mod emitter_check;
mod inventory;
mod panel_check;
mod surface_check;
mod trace_check;

pub use contract_policy::check_contract_range_policy;
pub use emitter_check::check_next_action_emitters;
pub use inventory::{
    ExampleClassification, InventoryError, NextActionEmitter, NextActionShape, PanelBinding,
    PanelMutation, SurfaceExample, SurfaceInventory, SurfaceSpec, load_surface_inventory,
};
pub use panel_check::check_panel_mutations;
pub use surface_check::{SurfaceCheckReport, check_surface_inventory};
pub use trace_check::{NextActionTraceReport, check_next_action_trace};

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

pub fn contract_version_matches(
    requirement: &str,
    version: &str,
) -> Result<bool, ContractVersionError> {
    let version = parse_contract_version(version)?;
    if requirement.is_empty() {
        return Err(ContractVersionError(
            "CLI contract requirement must not be empty".to_string(),
        ));
    }
    requirement.split(',').try_fold(true, |matches, raw| {
        let comparator = raw.trim();
        let (operator, expected) = [">=", "<=", ">", "<", "="]
            .into_iter()
            .find_map(|operator| {
                comparator
                    .strip_prefix(operator)
                    .map(|value| (operator, value))
            })
            .ok_or_else(|| {
                ContractVersionError(format!(
                    "unsupported CLI contract comparator '{comparator}'"
                ))
            })?;
        let expected = parse_contract_version(expected)?;
        let current_matches = match operator {
            ">=" => version >= expected,
            "<=" => version <= expected,
            ">" => version > expected,
            "<" => version < expected,
            "=" => version == expected,
            _ => unreachable!("comparator allowlist is exhaustive"),
        };
        Ok(matches && current_matches)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicArgv {
    pub command_path: Vec<String>,
    pub explicit_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicArgvError {
    pub kind: PublicArgvErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicArgvErrorKind {
    Parse,
    HiddenCommand,
    HiddenArgument,
}

pub fn validate_public_argv<I, S>(argv: I) -> Result<PublicArgv, PublicArgvError>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    let command = Cli::command();
    let matches = match command.clone().try_get_matches_from(argv) {
        Ok(matches) => matches,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            return Ok(PublicArgv {
                command_path: vec!["loom".to_string()],
                explicit_args: Vec::new(),
            });
        }
        Err(error) => {
            return Err(PublicArgvError {
                kind: PublicArgvErrorKind::Parse,
                message: error.to_string(),
            });
        }
    };
    Cli::from_arg_matches(&matches).map_err(|error| PublicArgvError {
        kind: PublicArgvErrorKind::Parse,
        message: error.to_string(),
    })?;
    let mut result = PublicArgv {
        command_path: vec!["loom".to_string()],
        explicit_args: Vec::new(),
    };
    inspect_public_matches(&command, &matches, &mut result)?;
    Ok(result)
}

pub fn parser_error_kind<I, S>(argv: I) -> Option<ErrorKind>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    Cli::command()
        .try_get_matches_from(argv)
        .err()
        .map(|error| error.kind())
}

fn inspect_public_matches(
    command: &Command,
    matches: &ArgMatches,
    result: &mut PublicArgv,
) -> Result<(), PublicArgvError> {
    for argument in command.get_arguments() {
        if matches.value_source(argument.get_id().as_str()) != Some(ValueSource::CommandLine) {
            continue;
        }
        if argument.is_hide_set() {
            return Err(PublicArgvError {
                kind: PublicArgvErrorKind::HiddenArgument,
                message: format!(
                    "hidden argument '{}' is not part of the public CLI contract",
                    argument.get_id()
                ),
            });
        }
        result.explicit_args.push(argument.get_id().to_string());
    }
    let Some((name, sub_matches)) = matches.subcommand() else {
        return Ok(());
    };
    let subcommand = command
        .get_subcommands()
        .find(|candidate| candidate.get_name() == name)
        .ok_or_else(|| PublicArgvError {
            kind: PublicArgvErrorKind::Parse,
            message: format!("parsed command '{name}' is absent from the shared schema"),
        })?;
    if subcommand.is_hide_set() {
        return Err(PublicArgvError {
            kind: PublicArgvErrorKind::HiddenCommand,
            message: format!("hidden command '{name}' is not part of the public CLI contract"),
        });
    }
    result.command_path.push(name.to_string());
    inspect_public_matches(subcommand, sub_matches, result)
}
