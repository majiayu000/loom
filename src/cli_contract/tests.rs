use std::ffi::OsString;

use clap::{Arg, ArgAction, Command, builder::PossibleValue, error::ErrorKind};

use super::{
    PublicArgv, PublicArgvError, PublicArgvErrorKind, command_schema_capabilities,
    inspect_display_matches, inspect_public_matches, inspect_requested_visibility,
    public_command_schema_capabilities, validate_public_argv,
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
