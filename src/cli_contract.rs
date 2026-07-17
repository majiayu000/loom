use std::{collections::BTreeSet, fmt};

use clap::{
    ArgMatches, Command, CommandFactory, FromArgMatches, error::ErrorKind, parser::ValueSource,
};

use crate::{
    cli::Cli,
    sha256::{Sha256, to_hex},
};

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
    let mut result = PublicArgv {
        command_path: vec!["loom".to_string()],
        explicit_args: Vec::new(),
    };
    for raw in argv.iter().skip(1) {
        let Some(token) = raw.to_str() else {
            continue;
        };
        if let Some(long) = token.strip_prefix("--") {
            let name = long.split('=').next().unwrap_or(long);
            if let Some(argument) = current.get_arguments().find(|argument| {
                argument.get_long() == Some(name)
                    || argument
                        .get_all_aliases()
                        .unwrap_or_default()
                        .contains(&name)
            }) && (argument.is_hide_set()
                || argument.get_aliases().unwrap_or_default().contains(&name))
            {
                return Err(PublicArgvError {
                    kind: PublicArgvErrorKind::HiddenArgument,
                    message: format!(
                        "hidden argument '{}' is not part of the public CLI contract",
                        argument.get_id()
                    ),
                });
            }
            continue;
        }
        if token.starts_with('-') && token.len() == 2 {
            let short = token.chars().nth(1).expect("length checked");
            if let Some(argument) = current.get_arguments().find(|argument| {
                argument.get_short() == Some(short)
                    || argument
                        .get_all_short_aliases()
                        .unwrap_or_default()
                        .contains(&short)
            }) && (argument.is_hide_set()
                || (argument
                    .get_all_short_aliases()
                    .unwrap_or_default()
                    .contains(&short)
                    && !argument
                        .get_visible_short_aliases()
                        .unwrap_or_default()
                        .contains(&short)))
            {
                return Err(PublicArgvError {
                    kind: PublicArgvErrorKind::HiddenArgument,
                    message: format!(
                        "hidden argument '{}' is not part of the public CLI contract",
                        argument.get_id()
                    ),
                });
            }
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
                if argument.get_possible_values().iter().any(|possible| {
                    possible
                        .get_name_and_aliases()
                        .skip(1)
                        .any(|alias| alias == value)
                }) {
                    return Err(PublicArgvError {
                        kind: PublicArgvErrorKind::HiddenArgument,
                        message: format!(
                            "hidden value alias for '{}' is not part of the public CLI contract",
                            argument.get_id()
                        ),
                    });
                }
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
mod tests {
    use std::ffi::OsString;

    use clap::{Arg, ArgAction, Command, builder::PossibleValue};

    use super::{
        PublicArgv, PublicArgvError, PublicArgvErrorKind, command_schema_capabilities,
        inspect_public_matches, inspect_requested_visibility, public_command_schema_capabilities,
        validate_public_argv,
    };

    fn fixture_capabilities(command: Command) -> std::collections::BTreeSet<String> {
        let mut command = command;
        command.build();
        command_schema_capabilities(&command, &["loom".to_string(), "demo".to_string()])
            .expect("fixture schema")
    }

    fn validate_fixture_argv(
        mut command: Command,
        argv: &[&str],
    ) -> Result<PublicArgv, PublicArgvError> {
        command.build();
        let argv = argv.iter().map(OsString::from).collect::<Vec<_>>();
        inspect_requested_visibility(&command, &argv)?;
        let matches = command
            .clone()
            .try_get_matches_from(&argv)
            .expect("fixture must remain valid Clap input");
        let mut result = PublicArgv {
            command_path: vec!["loom".to_string()],
            explicit_args: Vec::new(),
        };
        inspect_public_matches(&command, &matches, &mut result)?;
        Ok(result)
    }

    #[test]
    fn command_schema_ignores_fixture_values() {
        let alpha = validate_public_argv(["loom", "skill", "inspect", "alpha"])
            .expect("first public command");
        let beta = validate_public_argv(["loom", "skill", "inspect", "beta"])
            .expect("second public command");
        assert_eq!(alpha.command_path, beta.command_path);
        assert_eq!(
            public_command_schema_capabilities(&alpha.command_path).expect("alpha schema"),
            public_command_schema_capabilities(&beta.command_path).expect("beta schema")
        );
    }

    #[test]
    fn command_schema_optional_additions_are_additive() {
        let base = fixture_capabilities(Command::new("loom").subcommand(Command::new("demo")));
        let with_flag = fixture_capabilities(
            Command::new("loom").subcommand(
                Command::new("demo").arg(
                    Arg::new("fixture_flag")
                        .long("fixture-flag")
                        .action(ArgAction::SetTrue),
                ),
            ),
        );
        assert!(base.is_subset(&with_flag));
        assert!(with_flag.len() > base.len());
    }

    #[test]
    fn command_schema_tracks_enum_alias_default_conflict_and_delimiter_semantics() {
        let base_command = || {
            Command::new("loom").subcommand(
                Command::new("demo")
                    .arg(
                        Arg::new("mode")
                            .long("mode")
                            .value_parser(["safe"])
                            .default_value("safe")
                            .value_delimiter(','),
                    )
                    .arg(Arg::new("other").long("other")),
            )
        };
        let base = fixture_capabilities(base_command());
        let additive = fixture_capabilities(
            Command::new("loom").subcommand(
                Command::new("demo")
                    .arg(
                        Arg::new("mode")
                            .long("mode")
                            .visible_alias("mode-alias")
                            .value_parser(["safe", "fast"])
                            .default_value("safe")
                            .value_delimiter(','),
                    )
                    .arg(Arg::new("other").long("other")),
            ),
        );
        assert!(base.is_subset(&additive));
        let breaking = fixture_capabilities(
            Command::new("loom").subcommand(
                Command::new("demo")
                    .arg(
                        Arg::new("mode")
                            .long("mode")
                            .value_parser(["safe"])
                            .default_value("fast")
                            .value_delimiter(';')
                            .conflicts_with("other"),
                    )
                    .arg(Arg::new("other").long("other")),
            ),
        );
        assert!(!base.is_subset(&breaking));
        assert!(!breaking.is_subset(&base));
    }

    #[test]
    fn hidden_aliases_are_not_public_cli_spellings() {
        let hidden_long = Command::new("loom").arg(
            Arg::new("mode")
                .long("mode")
                .alias("secret-mode")
                .action(ArgAction::SetTrue),
        );
        let error = validate_fixture_argv(hidden_long, &["loom", "--secret-mode"])
            .expect_err("hidden long alias must fail closed");
        assert_eq!(error.kind, PublicArgvErrorKind::HiddenArgument);

        let hidden_short = Command::new("loom").arg(
            Arg::new("mode")
                .short('m')
                .short_alias('x')
                .action(ArgAction::SetTrue),
        );
        let error = validate_fixture_argv(hidden_short, &["loom", "-x"])
            .expect_err("hidden short alias must fail closed");
        assert_eq!(error.kind, PublicArgvErrorKind::HiddenArgument);

        let hidden_command =
            Command::new("loom").subcommand(Command::new("demo").alias("secret-demo"));
        let error = validate_fixture_argv(hidden_command, &["loom", "secret-demo"])
            .expect_err("hidden command alias must fail closed");
        assert_eq!(error.kind, PublicArgvErrorKind::HiddenCommand);
    }

    #[test]
    fn hidden_possible_value_aliases_are_not_public_cli_spellings() {
        let command = Command::new("loom").arg(
            Arg::new("mode")
                .long("mode")
                .value_parser([PossibleValue::new("safe").alias("secret-safe")]),
        );
        let error = validate_fixture_argv(command, &["loom", "--mode", "secret-safe"])
            .expect_err("hidden possible-value alias must fail closed");
        assert_eq!(error.kind, PublicArgvErrorKind::HiddenArgument);
    }

    #[test]
    fn command_schema_contains_public_leaf_arguments() {
        let path = vec![
            "loom".to_string(),
            "skill".to_string(),
            "inspect".to_string(),
        ];
        let capabilities = public_command_schema_capabilities(&path).expect("inspect schema");
        assert!(
            capabilities
                .iter()
                .any(|value| value.starts_with("argument-core:loom/skill/inspect:skill:"))
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value.starts_with("argument-core:loom/skill/inspect:brief:"))
        );
        assert!(
            capabilities
                .iter()
                .any(|value| value.starts_with("argument-core:loom/skill/inspect:agent:"))
        );
    }
}
