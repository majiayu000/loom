use std::{collections::BTreeSet, ffi::OsString};

use clap::{
    Arg, ArgAction, Command, CommandFactory,
    builder::{PossibleValue, PossibleValuesParser},
    error::ErrorKind,
};

use crate::cli::Cli;

use super::{
    PublicArgv, PublicArgvError, PublicArgvErrorKind, command_schema_capabilities,
    command_tree_capabilities, contract_example_argv_variants, inspect_display_matches,
    inspect_public_matches, inspect_requested_visibility, public_command_paths,
    public_command_schema_capabilities, public_direct_command_paths, validate_public_argv,
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
    let help_result = inspect_requested_visibility(&command, &argv)?;
    let matches = match command.clone().try_get_matches_from(&argv) {
        Ok(matches) => matches,
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            inspect_display_matches(&command, &argv)?;
            return Ok(help_result);
        }
        Err(error) => panic!("fixture must remain valid Clap input: {error}"),
    };
    let mut result = PublicArgv {
        command_path: vec!["loom".to_string()],
        explicit_args: Vec::new(),
    };
    inspect_public_matches(&command, &matches, &mut result)?;
    Ok(result)
}

#[test]
fn command_schema_ignores_fixture_values() {
    let alpha =
        validate_public_argv(["loom", "skill", "inspect", "alpha"]).expect("first public command");
    let beta =
        validate_public_argv(["loom", "skill", "inspect", "beta"]).expect("second public command");
    assert_eq!(alpha.command_path, beta.command_path);
    assert_eq!(
        public_command_schema_capabilities(&alpha.command_path).expect("alpha schema"),
        public_command_schema_capabilities(&beta.command_path).expect("beta schema")
    );
}

fn typed_positional_action_paths() -> BTreeSet<String> {
    fn visit(command: &Command, prefix: &mut Vec<String>, paths: &mut BTreeSet<String>) {
        for subcommand in command
            .get_subcommands()
            .filter(|subcommand| !subcommand.is_hide_set() && subcommand.get_name() != "help")
        {
            prefix.push(subcommand.get_name().to_string());
            for action in subcommand
                .get_positionals()
                .filter(|argument| argument.get_id() == "action")
                .flat_map(Arg::get_possible_values)
                .filter(|value| !value.is_hide_set())
            {
                paths.insert(format!("{} {}", prefix.join(" "), action.get_name()));
            }
            visit(subcommand, prefix, paths);
            prefix.pop();
        }
    }

    let mut root = Cli::command();
    root.build();
    let mut paths = BTreeSet::new();
    visit(&root, &mut vec!["loom".to_string()], &mut paths);
    paths
}

#[test]
fn public_command_paths_derive_typed_positional_actions() {
    let typed_actions = typed_positional_action_paths();
    for parent in ["loom index", "loom active"] {
        assert!(
            typed_actions
                .iter()
                .any(|path| path.starts_with(&format!("{parent} "))),
            "{parent} must expose typed positional action metadata"
        );
    }
    let paths = public_command_paths().into_iter().collect::<BTreeSet<_>>();
    assert!(
        typed_actions.is_subset(&paths),
        "public command paths missing typed positional actions: {:?}",
        typed_actions.difference(&paths).collect::<Vec<_>>()
    );
    let direct = public_direct_command_paths()
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert!(typed_actions.is_subset(&direct));
    assert!(direct.iter().any(|path| path == "loom skill compile"));
    for parent in ["loom skill", "loom index", "loom active"] {
        assert!(!direct.contains(parent));
    }
}

fn contract_command_lines(contract: &str) -> Result<Vec<String>, String> {
    let mut commands = Vec::new();
    let mut continued: Option<String> = None;
    for line in contract.lines() {
        let trimmed = line.trim();
        if let Some(command) = continued.as_mut() {
            command.push(' ');
            command.push_str(trimmed.trim_end_matches('\\').trim_end());
            if !trimmed.ends_with('\\') {
                commands.push(continued.take().expect("continued command exists"));
            }
        } else if trimmed.starts_with("loom ") {
            let command = trimmed.trim_end_matches('\\').trim_end().to_string();
            if trimmed.ends_with('\\') {
                continued = Some(command);
            } else {
                commands.push(command);
            }
        }
    }
    continued.map_or(Ok(commands), |command| {
        Err(format!("unterminated command continuation: {command}"))
    })
}

fn inline_contract_commands(contract: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut rejected_shapes = false;
    for line in contract.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            rejected_shapes = trimmed == "## 18. Rejected CLI Shapes";
        }
        if !rejected_shapes {
            commands.extend(
                line.split('`')
                    .skip(1)
                    .step_by(2)
                    .filter(|code| code.starts_with("loom ") && !code.contains("..."))
                    .map(str::to_string),
            );
        }
    }
    commands
}

fn parsed_contract_path(argv: Vec<String>) -> Result<String, String> {
    let mut path = validate_public_argv(argv.clone())
        .map_err(|error| format!("{error:?}"))?
        .command_path;
    if argv.iter().any(|argument| argument == "--help") {
        return Ok(path.join(" "));
    }
    if let Some(action) = parsed_positional_action(&argv, &path)? {
        path.push(action);
    }
    Ok(path.join(" "))
}

fn parsed_positional_action(
    argv: &[String],
    command_path: &[String],
) -> Result<Option<String>, String> {
    let mut root = Cli::command();
    root.build();
    let root_matches = root
        .clone()
        .try_get_matches_from(argv)
        .map_err(|error| error.to_string())?;
    let mut command = &root;
    let mut matches = &root_matches;
    for segment in command_path.iter().skip(1) {
        command = command
            .get_subcommands()
            .find(|candidate| candidate.get_name() == segment)
            .ok_or_else(|| format!("parsed command segment {segment:?} is absent from schema"))?;
        matches = matches
            .subcommand_matches(segment)
            .ok_or_else(|| format!("parsed command segment {segment:?} is absent from matches"))?;
    }
    let Some(action_argument) = command
        .get_positionals()
        .find(|argument| argument.get_id() == "action")
    else {
        return Ok(None);
    };
    let possible_values = action_argument.get_possible_values();
    if possible_values.is_empty() {
        return Ok(None);
    }
    let action = matches
        .get_one::<String>("action")
        .ok_or_else(|| format!("documented positional action is absent in {argv:?}"))?;
    possible_values
        .into_iter()
        .any(|value| value.matches(action, false))
        .then(|| action.clone())
        .ok_or_else(|| format!("invalid documented positional action in {argv:?}"))
        .map(Some)
}

fn documented_contract_paths(contract: &str) -> Result<BTreeSet<String>, String> {
    let mut documented = BTreeSet::new();
    for command in contract_command_lines(contract)? {
        let mut parsed_any = false;
        let mut errors = Vec::new();
        let required_choices = if command.contains("<build|status>") {
            vec![
                command.replace("<build|status>", "build"),
                command.replace("<build|status>", "status"),
            ]
        } else {
            vec![command.clone()]
        };
        for choice in required_choices {
            for argv in contract_example_argv_variants(&choice) {
                match parsed_contract_path(argv) {
                    Ok(path) => {
                        documented.insert(path);
                        parsed_any = true;
                    }
                    Err(error) => errors.push(error),
                }
            }
        }
        if !parsed_any {
            return Err(format!(
                "all variants of documented command {command:?} failed to parse: {errors:?}"
            ));
        }
    }
    for command in inline_contract_commands(contract) {
        for argv in contract_example_argv_variants(&command) {
            if let Ok(path) = parsed_contract_path(argv) {
                documented.insert(path);
            }
        }
    }
    Ok(documented)
}

fn missing_contract_paths<'a>(
    paths: &'a [String],
    contract: &str,
) -> Result<Vec<&'a String>, String> {
    let documented = documented_contract_paths(contract)?;
    let direct = public_direct_command_paths()
        .into_iter()
        .collect::<BTreeSet<_>>();
    Ok(paths
        .iter()
        .filter(|path| {
            if direct.contains(path.as_str()) {
                !documented.contains(path.as_str())
            } else {
                let descendant = format!("{path} ");
                !documented
                    .iter()
                    .any(|candidate| candidate.starts_with(&descendant))
            }
        })
        .collect())
}

#[test]
fn contract_docs_track_exact_public_command_paths() {
    let contract = concat!(
        include_str!("../../docs/LOOM_CLI_CONTRACT.md"),
        include_str!("../../docs/LOOM_CLI_CONTRACT_OPERATIONS.md")
    );
    let paths = public_command_paths();
    assert!(
        paths.len() > 100,
        "public command path enumeration looks truncated: {} paths",
        paths.len()
    );
    let missing = missing_contract_paths(&paths, contract).expect("contract examples must parse");
    assert!(
        missing.is_empty(),
        "CLI contract docs are missing exact command paths:\n{}",
        missing
            .iter()
            .map(|path| format!("  {path}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn longer_command_does_not_document_direct_parent_path() {
    let paths = vec!["loom skill compile".to_string()];
    let missing =
        missing_contract_paths(&paths, "loom skill compile list demo").expect("parse fixture");
    assert_eq!(missing, paths.iter().collect::<Vec<_>>());
}

#[test]
fn prose_inline_code_does_not_document_an_executable_command() {
    let paths = vec!["loom init".to_string()];
    let missing = missing_contract_paths(&paths, "Run `init` before continuing.")
        .expect("inline prose fixture");
    assert_eq!(missing, paths.iter().collect::<Vec<_>>());
}

#[test]
fn typed_action_metadata_preserves_runtime_validation() {
    for argv in [
        vec!["loom", "index", "unsupported"],
        vec!["loom", "active", "unsupported", "task", "--agent", "codex"],
    ] {
        validate_public_argv(argv)
            .expect("unknown positional action must reach runtime validation");
    }
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
fn positional_action_values_change_schema_snapshot() {
    let snapshot = |values| {
        fixture_capabilities(
            Command::new("loom").subcommand(
                Command::new("demo").arg(
                    Arg::new("action")
                        .index(1)
                        .value_parser(PossibleValuesParser::new(values)),
                ),
            ),
        )
    };
    let original = snapshot(["build", "status"]);
    let renamed = snapshot(["build", "inspect"]);
    let removed = fixture_capabilities(
        Command::new("loom").subcommand(Command::new("demo").arg(Arg::new("action").index(1))),
    );
    assert_ne!(original, renamed, "renaming an action must change snapshot");
    assert_ne!(
        original, removed,
        "removing typed actions must change snapshot"
    );
    assert!(original.contains("argument-value:loom/demo:action:status"));
    assert!(renamed.contains("argument-value:loom/demo:action:inspect"));
    assert!(
        !removed
            .iter()
            .any(|capability| capability.starts_with("argument-value:loom/demo:action:"))
    );
}

#[test]
fn command_tree_tracks_unexampled_visible_commands_but_not_hidden_subtrees() {
    let mut base = Command::new("loom").subcommand(Command::new("documented"));
    base.build();
    let base_capabilities = command_tree_capabilities(&base).expect("base command tree");
    let mut extended = Command::new("loom")
        .subcommand(Command::new("documented"))
        .subcommand(Command::new("unexampled").subcommand(Command::new("nested")))
        .subcommand(
            Command::new("internal")
                .hide(true)
                .subcommand(Command::new("hidden-nested")),
        );
    extended.build();
    let extended_capabilities =
        command_tree_capabilities(&extended).expect("extended command tree");
    assert!(base_capabilities.is_subset(&extended_capabilities));
    assert!(extended_capabilities.len() > base_capabilities.len());

    let mut without_hidden = Command::new("loom")
        .subcommand(Command::new("documented"))
        .subcommand(Command::new("unexampled").subcommand(Command::new("nested")));
    without_hidden.build();
    assert_eq!(
        extended_capabilities,
        command_tree_capabilities(&without_hidden).expect("tree without hidden commands")
    );
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

    let hidden_bundle = Command::new("loom")
        .arg(Arg::new("verbose").short('v').action(ArgAction::SetTrue))
        .arg(
            Arg::new("mode")
                .short('m')
                .short_alias('x')
                .action(ArgAction::SetTrue),
        );
    let error = validate_fixture_argv(hidden_bundle, &["loom", "-vx", "--help"])
        .expect_err("bundled hidden short alias must fail closed");
    assert_eq!(error.kind, PublicArgvErrorKind::HiddenArgument);

    let hidden_attached = Command::new("loom").arg(
        Arg::new("mode")
            .short('m')
            .short_alias('x')
            .value_parser(["safe"]),
    );
    let error = validate_fixture_argv(hidden_attached, &["loom", "-xsafe", "--help"])
        .expect_err("attached hidden short alias must fail closed");
    assert_eq!(error.kind, PublicArgvErrorKind::HiddenArgument);

    let hidden_command = Command::new("loom").subcommand(Command::new("demo").alias("secret-demo"));
    let error = validate_fixture_argv(hidden_command, &["loom", "secret-demo"])
        .expect_err("hidden command alias must fail closed");
    assert_eq!(error.kind, PublicArgvErrorKind::HiddenCommand);
}

#[test]
fn option_values_that_match_hidden_commands_remain_values() {
    let command = Command::new("loom")
        .arg(Arg::new("request_id").long("request-id").global(true))
        .subcommand(Command::new("workflow").subcommand(Command::new("run").hide(true)));
    let parsed = validate_fixture_argv(
        command,
        &["loom", "workflow", "--request-id", "run", "--help"],
    )
    .expect("hidden command spelling used as an option value must remain public");
    assert_eq!(parsed.command_path, ["loom", "workflow"]);
}

#[test]
fn unbounded_option_values_follow_clap_subcommand_precedence() {
    let command = || {
        Command::new("loom")
            .arg(Arg::new("values").long("values").short('v').num_args(1..))
            .subcommand(Command::new("run").hide(true))
    };
    for argv in [
        ["loom", "--values", "safe", "run", "--help"],
        ["loom", "-v", "safe", "run", "--help"],
    ] {
        let parsed = validate_fixture_argv(command(), &argv)
            .expect("greedy option values must not be mistaken for a hidden subcommand");
        assert_eq!(parsed.command_path, ["loom"]);
    }

    let error = validate_fixture_argv(
        command().subcommand_precedence_over_arg(true),
        &["loom", "--values", "safe", "run", "--help"],
    )
    .expect_err("subcommand precedence must still reject a hidden subcommand");
    assert_eq!(error.kind, PublicArgvErrorKind::HiddenCommand);
}

#[test]
fn hidden_possible_values_fail_with_and_without_help() {
    let command = || {
        Command::new("loom").arg(Arg::new("mode").long("mode").value_parser([
            PossibleValue::new("safe").alias("secret-safe"),
            PossibleValue::new("classified").hide(true),
        ]))
    };
    for value in ["secret-safe", "classified"] {
        for argv in [
            vec!["loom", "--mode", value],
            vec!["loom", "--mode", value, "--help"],
        ] {
            let error = validate_fixture_argv(command(), &argv)
                .expect_err("hidden possible value must fail closed");
            assert_eq!(error.kind, PublicArgvErrorKind::HiddenArgument);
        }
    }

    let delimiter = Command::new("loom").arg(
        Arg::new("mode")
            .long("mode")
            .value_delimiter(',')
            .value_parser([
                PossibleValue::new("safe"),
                PossibleValue::new("classified").hide(true),
            ]),
    );
    let error = validate_fixture_argv(delimiter, &["loom", "--mode", "safe,classified", "--help"])
        .expect_err("delimiter-packed hidden value must fail closed");
    assert_eq!(error.kind, PublicArgvErrorKind::HiddenArgument);

    let multiple =
        Command::new("loom").arg(Arg::new("mode").long("mode").num_args(2).value_parser([
            PossibleValue::new("safe"),
            PossibleValue::new("classified").hide(true),
        ]));
    let error = validate_fixture_argv(
        multiple,
        &["loom", "--mode", "safe", "classified", "--help"],
    )
    .expect_err("second hidden option value must fail closed");
    assert_eq!(error.kind, PublicArgvErrorKind::HiddenArgument);

    let positional = Command::new("loom").arg(Arg::new("mode").index(1).value_parser([
        PossibleValue::new("safe"),
        PossibleValue::new("classified").hide(true),
    ]));
    let error = validate_fixture_argv(positional, &["loom", "classified", "--help"])
        .expect_err("hidden positional value must fail closed");
    assert_eq!(error.kind, PublicArgvErrorKind::HiddenArgument);

    let attached = Command::new("loom").arg(Arg::new("mode").short('m').value_parser([
        PossibleValue::new("safe"),
        PossibleValue::new("classified").hide(true),
    ]));
    let error = validate_fixture_argv(attached, &["loom", "-mclassified", "--help"])
        .expect_err("attached hidden option value must fail closed");
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
