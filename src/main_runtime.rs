use std::ffi::OsString;

use clap::{Parser, error::ErrorKind};
use serde_json::{Value, json};

use crate::cli::{Cli, Command};
use crate::commands::{App, command_name};
use crate::envelope::Envelope;
use crate::panel;
use crate::types::ErrorCode;

pub async fn run() {
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
            let code = ErrorCode::InitError;
            if cli.json {
                let env = failure_envelope(
                    &cli,
                    "app.init",
                    code,
                    format!("failed to initialize app: {err}"),
                    json!({ "stage": "app.init" }),
                );
                print_envelope(&env, true, cli.pretty);
            } else {
                eprintln!("failed to initialize app: {}", err);
            }
            std::process::exit(code.exit_code());
        }
    };

    if let Command::Panel(args) = &cli.command {
        if let Err(err) = panel::run_panel(app.ctx.clone(), args.port).await {
            let code = ErrorCode::IoError;
            if cli.json {
                let env = failure_envelope(
                    &cli,
                    "panel",
                    code,
                    format!("panel failed: {err}"),
                    json!({ "stage": "panel.serve", "port": args.port }),
                );
                print_envelope(&env, true, cli.pretty);
            } else {
                eprintln!("panel failed: {}", err);
            }
            std::process::exit(code.exit_code());
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
            let code = ErrorCode::InternalError;
            if cli.json {
                let env = top_level_failure_envelope(&cli, err.to_string());
                print_envelope(&env, true, cli.pretty);
            } else {
                eprintln!("command failed: {}", err);
            }
            std::process::exit(code.exit_code());
        }
    }
}

fn failure_envelope(
    cli: &Cli,
    cmd: &str,
    code: ErrorCode,
    message: String,
    details: Value,
) -> Envelope {
    let request_id = cli
        .request_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    Envelope::err(cmd, request_id, code, message, details)
}

fn top_level_failure_envelope(cli: &Cli, message: String) -> Envelope {
    failure_envelope(
        cli,
        command_name(&cli.command),
        ErrorCode::InternalError,
        format!("command failed: {message}"),
        json!({ "stage": "app.execute" }),
    )
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
        if env.cmd == "skill.inspect" {
            if !env.meta.warnings.is_empty() {
                for w in &env.meta.warnings {
                    eprintln!("warning: {}", w);
                }
            }
            if !print_skill_inspect_card(&env.data) && !env.data.is_null() {
                println!("{}", pretty_json_or_empty_object(&env.data));
            }
            return;
        }
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
        for action in &err.next_actions {
            eprintln!("hint: try {} - {}", action.cmd, action.reason);
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

fn print_skill_inspect_card(data: &Value) -> bool {
    let Some(skill) = data.get("skill").and_then(Value::as_str) else {
        return false;
    };
    let source = &data["source"];
    let spec = &data["spec"];
    let runtime = &data["runtime"];
    let safety = &data["safety"];
    let source_state = if source["exists"].as_bool() == Some(true) {
        "present"
    } else {
        "missing"
    };
    let source_drift = if source["working_tree_drift"].as_bool() == Some(true) {
        "drift"
    } else {
        "clean"
    };
    println!("{skill}");
    if source["entrypoint_exists"].as_bool() == Some(false) {
        println!("Source:   {source_state}, {source_drift}, entrypoint missing");
    } else {
        println!("Source:   {source_state}, {source_drift}");
    }
    println!(
        "Spec:     portable {}, codex {}, claude {}",
        spec["portable"].as_str().unwrap_or("unknown"),
        spec["codex"].as_str().unwrap_or("unknown"),
        spec["claude"].as_str().unwrap_or("unknown")
    );
    print!("Runtime:  ");
    if let Some(map) = runtime.as_object() {
        for (index, (agent, status)) in map.iter().enumerate() {
            if index > 0 {
                print!("; ");
            }
            let visible = status["visible_to_agent"].as_str().unwrap_or("unknown");
            if status["projected_to_target"].as_bool() == Some(true) {
                print!("{agent} projected, visibility {visible}");
            } else if status["active_rule_present"].as_bool() == Some(true) {
                print!("{agent} active rule, projection missing, visibility {visible}");
            } else if status["installed_in_registry"].as_bool() == Some(true) {
                print!("{agent} installed, not projected");
            } else {
                print!("{agent} not installed");
            }
            if status["materialized_path_exists"].as_bool() == Some(false) {
                print!(", materialized missing");
            }
        }
    } else {
        print!("not checked");
    }
    println!();
    print_skill_quality(&data["quality"]);
    let trust = safety["trust"].as_str().unwrap_or("unknown");
    let policy = safety["policy"].as_str().unwrap_or("unknown");
    let decision = safety["decision"].as_str().unwrap_or("unknown");
    if decision == "unknown" {
        println!("Safety:   trust {trust}, policy {policy}");
    } else {
        println!("Safety:   trust {trust}, policy {policy}, decision {decision}");
    }
    let next = data["next_actions"]
        .as_array()
        .and_then(|actions| actions.iter().find_map(Value::as_str))
        .unwrap_or("none");
    println!("Next:     {next}");
    true
}

fn print_skill_quality(quality: &Value) {
    let status = quality["status"].as_str().unwrap_or("unavailable");
    let Some(last_eval) = quality["last_eval"].as_str() else {
        println!("Quality:  {status}");
        return;
    };
    let mut metrics = Vec::new();
    if let Some(value) = quality["trigger_precision"].as_f64() {
        metrics.push(format!("precision {value:.2}"));
    }
    if let Some(value) = quality["trigger_recall"].as_f64() {
        metrics.push(format!("recall {value:.2}"));
    }
    if metrics.is_empty() {
        println!("Quality:  {status} at {last_eval}");
    } else {
        println!("Quality:  {status} at {last_eval}, {}", metrics.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{pretty_json_or_empty_object, top_level_failure_envelope};
    use crate::cli::Cli;
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

    #[test]
    fn top_level_error_seam_builds_structured_json_envelope() {
        let cli = Cli::try_parse_from([
            "loom",
            "--json",
            "--request-id",
            "req-top-level",
            "workspace",
            "status",
        ])
        .expect("parse fixture CLI");
        let env = top_level_failure_envelope(&cli, "injected top-level failure".to_string());
        let value = serde_json::to_value(env).expect("serialize fixture envelope");

        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["cmd"], json!("workspace.status"));
        assert_eq!(value["request_id"], json!("req-top-level"));
        assert_eq!(value["error"]["code"], json!("INTERNAL_ERROR"));
        assert_eq!(value["error"]["details"]["stage"], json!("app.execute"));
    }
}
