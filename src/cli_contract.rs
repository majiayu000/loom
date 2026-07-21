use std::{collections::BTreeSet, fmt};

use clap::{
    Arg, ArgMatches, Command, CommandFactory, FromArgMatches, error::ErrorKind, parser::ValueSource,
};

use crate::{
    cli::Cli,
    sha256::{Sha256, to_hex},
};

mod agent_capabilities;
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

pub const CLI_CONTRACT_VERSION: &str = "1.8.0";

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
    let argv = argv.into_iter().map(Into::into).collect::<Vec<_>>();
    let help_result = inspect_requested_visibility(&command, &argv)?;
    let matches = match command.clone().try_get_matches_from(&argv) {
        Ok(matches) => matches,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            inspect_display_matches(&command, &argv)?;
            return Ok(help_result);
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

pub(crate) fn public_command_schema_capabilities(
    command_path: &[String],
) -> Result<BTreeSet<String>, PublicArgvError> {
    if command_path.first().map(String::as_str) != Some("loom") {
        return Err(PublicArgvError {
            kind: PublicArgvErrorKind::Parse,
            message: "public command path must start with 'loom'".to_string(),
        });
    }
    let mut root = Cli::command();
    root.build();
    command_schema_capabilities(&root, command_path)
}

pub(crate) fn public_command_tree_capabilities() -> Result<BTreeSet<String>, PublicArgvError> {
    let mut root = Cli::command();
    root.build();
    command_tree_capabilities(&root)
}

fn command_tree_capabilities(root: &Command) -> Result<BTreeSet<String>, PublicArgvError> {
    fn visit(
        root: &Command,
        command: &Command,
        path: &mut Vec<String>,
        capabilities: &mut BTreeSet<String>,
    ) -> Result<(), PublicArgvError> {
        capabilities.extend(command_schema_capabilities(root, path)?);
        for subcommand in command
            .get_subcommands()
            .filter(|subcommand| !subcommand.is_hide_set() && subcommand.get_name() != "help")
        {
            path.push(subcommand.get_name().to_string());
            visit(root, subcommand, path, capabilities)?;
            path.pop();
        }
        Ok(())
    }

    let mut capabilities = BTreeSet::new();
    visit(root, root, &mut vec!["loom".to_string()], &mut capabilities)?;
    Ok(capabilities)
}

fn command_schema_capabilities(
    root: &Command,
    command_path: &[String],
) -> Result<BTreeSet<String>, PublicArgvError> {
    let mut command = root;
    for name in command_path.iter().skip(1) {
        command = command
            .get_subcommands()
            .find(|candidate| candidate.get_name() == name)
            .ok_or_else(|| PublicArgvError {
                kind: PublicArgvErrorKind::Parse,
                message: format!("public command schema is missing path segment '{name}'"),
            })?;
        if command.is_hide_set() {
            return Err(PublicArgvError {
                kind: PublicArgvErrorKind::HiddenCommand,
                message: format!("hidden command '{name}' is not part of the public CLI contract"),
            });
        }
    }
    let path = command_path.join("/");
    let mut capabilities = BTreeSet::from([format!("command:{path}")]);
    capabilities.extend(
        command
            .get_visible_aliases()
            .map(|alias| format!("command-alias:{path}:{alias}")),
    );
    capabilities.extend(
        command
            .get_subcommands()
            .filter(|subcommand| !subcommand.is_hide_set())
            .map(|subcommand| format!("subcommand:{path}/{}", subcommand.get_name())),
    );
    capabilities.insert(format!(
        "command-core:{path}:{}",
        schema_digest(&[
            format!(
                "allow_external_subcommands={}",
                command.is_allow_external_subcommands_set()
            ),
            format!(
                "allow_missing_positional={}",
                command.is_allow_missing_positional_set()
            ),
            format!(
                "arg_required_else_help={}",
                command.is_arg_required_else_help_set()
            ),
            format!(
                "dont_delimit_trailing_values={}",
                command.is_dont_delimit_trailing_values_set()
            ),
            format!(
                "subcommand_required={}",
                command.is_subcommand_required_set()
            ),
            format!("trailing_var_arg={}", command.is_trailing_var_arg_set()),
        ])
    ));
    let public_arguments = command
        .get_arguments()
        .filter(|argument| {
            !argument.is_hide_set()
                && !matches!(argument.get_id().as_str(), "help" | "version")
                && (command_path.len() == 1 || !argument.is_global_set())
        })
        .collect::<Vec<_>>();
    let mut required = public_arguments
        .iter()
        .filter(|argument| argument.is_required_set())
        .map(|argument| argument.get_id().to_string())
        .collect::<Vec<_>>();
    required.sort();
    capabilities.insert(format!("required-arguments:{path}:{}", required.join(",")));
    for argument in public_arguments {
        let id = argument.get_id();
        let mut conflicts = command
            .get_arg_conflicts_with(argument)
            .into_iter()
            .map(|conflict| conflict.get_id().to_string())
            .collect::<Vec<_>>();
        conflicts.sort();
        let mut aliases = argument
            .get_visible_aliases()
            .unwrap_or_default()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        aliases.sort();
        let mut short_aliases = argument.get_visible_short_aliases().unwrap_or_default();
        short_aliases.sort();
        let mut defaults = argument
            .get_default_values()
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        defaults.sort();
        let core = [
            format!("action={:?}", argument.get_action()),
            format!(
                "allow_hyphen_values={}",
                argument.is_allow_hyphen_values_set()
            ),
            format!(
                "allow_negative_numbers={}",
                argument.is_allow_negative_numbers_set()
            ),
            format!("conflicts={}", conflicts.join(",")),
            format!("defaults={}", defaults.join(",")),
            format!("exclusive={}", argument.is_exclusive_set()),
            format!("global={}", argument.is_global_set()),
            format!("ignore_case={}", argument.is_ignore_case_set()),
            format!(
                "index={}",
                argument
                    .get_index()
                    .map(|value| value.to_string())
                    .unwrap_or_default()
            ),
            format!("last={}", argument.is_last_set()),
            format!("long={}", argument.get_long().unwrap_or("")),
            format!(
                "arity={}",
                argument
                    .get_num_args()
                    .map(|value| value.to_string())
                    .unwrap_or_default()
            ),
            format!(
                "parser={}",
                if argument.get_possible_values().is_empty() {
                    format!("{:?}", argument.get_value_parser())
                } else {
                    "enumerated".to_string()
                }
            ),
            format!("require_equals={}", argument.is_require_equals_set()),
            format!(
                "short={}",
                argument
                    .get_short()
                    .map(|value| value.to_string())
                    .unwrap_or_default()
            ),
            format!("trailing_var_arg={}", argument.is_trailing_var_arg_set()),
            format!(
                "value_delimiter={}",
                argument
                    .get_value_delimiter()
                    .map(|value| value.to_string())
                    .unwrap_or_default()
            ),
            format!("value_hint={:?}", argument.get_value_hint()),
            format!(
                "value_names={}",
                argument
                    .get_value_names()
                    .unwrap_or_default()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            format!(
                "value_terminator={}",
                argument
                    .get_value_terminator()
                    .map(ToString::to_string)
                    .unwrap_or_default()
            ),
        ];
        capabilities.insert(format!(
            "argument-core:{path}:{id}:{}",
            schema_digest(&core)
        ));
        capabilities.extend(
            aliases
                .into_iter()
                .map(|alias| format!("argument-alias:{path}:{id}:{alias}")),
        );
        capabilities.extend(
            short_aliases
                .into_iter()
                .map(|alias| format!("argument-short-alias:{path}:{id}:{alias}")),
        );
        for possible in argument.get_possible_values() {
            if possible.is_hide_set() {
                continue;
            }
            capabilities.insert(format!(
                "argument-value:{path}:{id}:{}",
                possible.get_name()
            ));
        }
    }
    for group in command.get_groups() {
        let mut group = group.clone();
        let group_id = group.get_id().to_string();
        let required = group.is_required_set();
        let multiple = group.is_multiple();
        let mut arguments = group
            .get_args()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        arguments.sort();
        capabilities.insert(format!(
            "argument-group:{path}:{}:{}",
            group_id,
            schema_digest(&[
                format!("arguments={}", arguments.join(",")),
                format!("multiple={multiple}"),
                format!("required={required}"),
            ])
        ));
    }
    Ok(capabilities)
}

fn schema_digest(fields: &[String]) -> String {
    let mut hasher = Sha256::new();
    for field in fields {
        hasher.update(field.len().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(field.as_bytes());
    }
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

fn inspect_requested_visibility(
    command: &Command,
    argv: &[std::ffi::OsString],
) -> Result<PublicArgv, PublicArgvError> {
    let mut current = command;
    let mut options_terminated = false;
    let mut pending_option_values: Option<PendingOptionValues> = None;
    let mut result = PublicArgv {
        command_path: vec!["loom".to_string()],
        explicit_args: Vec::new(),
    };
    for raw in argv.iter().skip(1) {
        let Some(token) = raw.to_str() else {
            continue;
        };
        if token == "--" {
            options_terminated = true;
            continue;
        }
        if let Some(mut pending) = pending_option_values.take()
            && pending.should_consume(token, current)
        {
            pending.consume_one();
            if pending.remaining > 0 {
                pending_option_values = Some(pending);
            }
            continue;
        }
        if !options_terminated && let Some(long) = token.strip_prefix("--") {
            let has_inline_value = long.contains('=');
            let name = long.split('=').next().unwrap_or(long);
            let argument = current.get_arguments().find(|argument| {
                argument.get_long() == Some(name)
                    || argument
                        .get_all_aliases()
                        .unwrap_or_default()
                        .contains(&name)
            });
            if let Some(argument) = argument
                && (argument.is_hide_set()
                    || argument.get_aliases().unwrap_or_default().contains(&name))
            {
                return Err(hidden_argument_error(argument));
            }
            if let Some(argument) = argument
                && !has_inline_value
            {
                pending_option_values = following_option_values(argument, has_inline_value);
            }
            continue;
        }
        if !options_terminated
            && let Some(shorts) = token.strip_prefix('-').filter(|shorts| !shorts.is_empty())
        {
            for (offset, short) in shorts.char_indices() {
                let argument = current.get_arguments().find(|argument| {
                    argument.get_short() == Some(short)
                        || argument
                            .get_all_short_aliases()
                            .unwrap_or_default()
                            .contains(&short)
                });
                let Some(argument) = argument else {
                    continue;
                };
                let hidden_alias = argument
                    .get_all_short_aliases()
                    .unwrap_or_default()
                    .contains(&short)
                    && !argument
                        .get_visible_short_aliases()
                        .unwrap_or_default()
                        .contains(&short);
                if argument.is_hide_set() || hidden_alias {
                    return Err(hidden_argument_error(argument));
                }
                if argument.get_action().takes_values() {
                    let has_attached_value = offset + short.len_utf8() < shorts.len();
                    if !has_attached_value {
                        pending_option_values =
                            following_option_values(argument, has_attached_value);
                    }
                    break;
                }
            }
            continue;
        }
        if options_terminated {
            continue;
        }
        let Some(subcommand) = current.get_subcommands().find(|candidate| {
            candidate.get_name() == token || candidate.get_all_aliases().any(|alias| alias == token)
        }) else {
            continue;
        };
        if subcommand.is_hide_set() || subcommand.get_aliases().any(|alias| alias == token) {
            return Err(PublicArgvError {
                kind: PublicArgvErrorKind::HiddenCommand,
                message: format!("hidden command '{token}' is not part of the public CLI contract"),
            });
        }
        result.command_path.push(subcommand.get_name().to_string());
        current = subcommand;
    }
    Ok(result)
}

#[derive(Clone, Copy)]
struct PendingOptionValues {
    remaining: usize,
    allow_hyphen_values: bool,
}

impl PendingOptionValues {
    fn should_consume(&self, token: &str, command: &Command) -> bool {
        let subcommand_takes_precedence = command.is_subcommand_precedence_over_arg_set()
            && command.get_subcommands().any(|candidate| {
                candidate.get_name() == token
                    || candidate.get_all_aliases().any(|alias| alias == token)
            });
        if subcommand_takes_precedence {
            return false;
        }
        self.allow_hyphen_values || (token != "--" && !token.starts_with('-'))
    }

    fn consume_one(&mut self) {
        if self.remaining != usize::MAX {
            self.remaining -= 1;
        }
    }
}

fn following_option_values(
    argument: &Arg,
    has_attached_value: bool,
) -> Option<PendingOptionValues> {
    if !argument.get_action().takes_values() {
        return None;
    }
    let maximum = argument
        .get_num_args()
        .map(|range| range.max_values())
        .unwrap_or(1);
    let remaining = if maximum == usize::MAX {
        usize::MAX
    } else {
        maximum.saturating_sub(usize::from(has_attached_value))
    };
    (remaining > 0).then_some(PendingOptionValues {
        remaining,
        allow_hyphen_values: argument.is_allow_hyphen_values_set(),
    })
}

fn inspect_display_matches(
    command: &Command,
    argv: &[std::ffi::OsString],
) -> Result<(), PublicArgvError> {
    let mut probe_argv = argv.to_vec();
    if probe_argv
        .last()
        .and_then(|value| value.to_str())
        .is_some_and(|value| matches!(value, "--help" | "-h" | "--version" | "-V"))
    {
        probe_argv.pop();
    }
    let matches = command
        .clone()
        .ignore_errors(true)
        .try_get_matches_from(&probe_argv)
        .map_err(|error| PublicArgvError {
            kind: PublicArgvErrorKind::Parse,
            message: error.to_string(),
        })?;
    let mut result = PublicArgv {
        command_path: vec!["loom".to_string()],
        explicit_args: Vec::new(),
    };
    inspect_public_matches(command, &matches, &mut result)
}

fn hidden_argument_error(argument: &Arg) -> PublicArgvError {
    PublicArgvError {
        kind: PublicArgvErrorKind::HiddenArgument,
        message: format!(
            "hidden argument '{}' is not part of the public CLI contract",
            argument.get_id()
        ),
    }
}

fn reject_hidden_possible_value(argument: &Arg, raw: &str) -> Result<(), PublicArgvError> {
    let values = argument
        .get_value_delimiter()
        .map_or_else(|| vec![raw], |delimiter| raw.split(delimiter).collect());
    let equals = |left: &str, right: &str| {
        if argument.is_ignore_case_set() {
            left.eq_ignore_ascii_case(right)
        } else {
            left == right
        }
    };
    let hidden = values.into_iter().any(|value| {
        argument.get_possible_values().iter().any(|possible| {
            (possible.is_hide_set() && equals(possible.get_name(), value))
                || possible
                    .get_name_and_aliases()
                    .skip(1)
                    .any(|alias| equals(alias, value))
        })
    });
    if hidden {
        return Err(hidden_argument_error(argument));
    }
    Ok(())
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
        if let Some(values) = matches.get_raw(argument.get_id().as_str()) {
            for value in values.filter_map(|value| value.to_str()) {
                reject_hidden_possible_value(argument, value)?;
            }
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

#[cfg(test)]
mod tests;
