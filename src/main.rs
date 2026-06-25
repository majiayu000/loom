mod cli;
mod commands;
mod envelope;
mod fs_util;
mod gitops;
mod panel;
mod sha256;
mod state;
mod state_model;
mod types;

use std::ffi::OsString;

use clap::{Parser, error::ErrorKind};
use serde_json::json;

use crate::cli::{Cli, Command};
use crate::commands::App;
use crate::envelope::Envelope;
use crate::types::ErrorCode;

#[tokio::main]
async fn main() {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let json_requested = has_flag(&raw_args, "--json");
    let pretty_requested = has_flag(&raw_args, "--pretty");
    let parse_request_id =
        extract_request_id(&raw_args).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut cli = match Cli::try_parse_from(&raw_args) {
        Ok(cli) => cli,
        Err(err) => {
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                let _ = err.print();
                std::process::exit(0);
            }
            if json_requested {
                let code = ErrorCode::ArgInvalid;
                let env = Envelope::err(
                    "cli.parse",
                    parse_request_id,
                    code,
                    err.to_string(),
                    json!({ "kind": format!("{:?}", err.kind()) }),
                );
                print_envelope(&env, true, pretty_requested);
                std::process::exit(code.exit_code());
            }
            err.exit();
        }
    };
    cli.request_id = cli.request_id.and_then(valid_request_id);

    let app = match App::new(cli.root.clone()) {
        Ok(app) => app,
        Err(err) => {
            eprintln!("failed to initialize app: {}", err);
            std::process::exit(3);
        }
    };

    if let Command::Panel(args) = &cli.command {
        if let Err(err) = panel::run_panel(app.ctx.clone(), args.port).await {
            eprintln!("panel failed: {}", err);
            std::process::exit(3);
        }
        return;
    }

    match app.execute(cli.clone()) {
        Ok((env, code)) => {
            print_envelope(&env, cli.json, cli.pretty);
            if code != 0 {
                std::process::exit(code);
            }
        }
        Err(err) => {
            eprintln!("command failed: {}", err);
            std::process::exit(3);
        }
    }
}

fn print_envelope(env: &Envelope, force_json: bool, pretty: bool) {
    if force_json {
        let rendered = if pretty {
            serde_json::to_string_pretty(env)
        } else {
            serde_json::to_string(env)
        };
        match rendered {
            Ok(s) => println!("{}", s),
            Err(e) => {
                eprintln!("failed to serialize output: {}", e);
                std::process::exit(5);
            }
        }
        return;
    }

    if env.ok {
        println!("{} ok", env.cmd);
        if !env.meta.warnings.is_empty() {
            for w in &env.meta.warnings {
                eprintln!("warning: {}", w);
            }
        }
        if !env.data.is_null() {
            println!("{}", pretty_json_or_empty_object(&env.data));
        }
    } else if let Some(err) = &env.error {
        eprintln!("{} failed: {} ({})", env.cmd, err.message, err.code);
        if !err.details.is_null() {
            eprintln!("{}", pretty_json_or_empty_object(&err.details));
        }
    } else {
        eprintln!("{} failed", env.cmd);
    }
}

fn has_flag(args: &[OsString], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn extract_request_id(args: &[OsString]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--request-id" {
            return iter.next().and_then(|value| {
                let value = value.to_string_lossy().into_owned();
                valid_request_id(value)
            });
        }
        if let Some(raw) = arg.to_string_lossy().strip_prefix("--request-id=") {
            return valid_request_id(raw.to_string());
        }
    }
    None
}

fn valid_request_id(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        None
    } else {
        Some(value)
    }
}

fn pretty_json_or_empty_object(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::pretty_json_or_empty_object;
    use serde_json::{Value, json};

    #[test]
    fn pretty_json_or_empty_object_formats_regular_values() {
        let rendered = pretty_json_or_empty_object(&json!({"ok": true}));
        assert!(rendered.contains("\"ok\": true"));
    }

    #[test]
    fn pretty_json_or_empty_object_preserves_empty_objects() {
        assert_eq!(
            pretty_json_or_empty_object(&Value::Object(Default::default())),
            "{}"
        );
    }
}
