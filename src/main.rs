mod agent_adapters;
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
use serde_json::{Value, json};

use crate::cli::{Cli, Command};
use crate::commands::App;
use crate::envelope::Envelope;
use crate::types::ErrorCode;

#[tokio::main(flavor = "current_thread")]
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
        if env.cmd == "skill.inspect" {
            if !env.meta.warnings.is_empty() {
                for w in &env.meta.warnings {
                    eprintln!("warning: {}", w);
                }
            }
            if let Some(card) = render_skill_inspect_card(&env.data) {
                println!("{card}");
            } else if !env.data.is_null() {
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

fn render_skill_inspect_card(data: &Value) -> Option<String> {
    let skill = data.get("skill")?.as_str()?;
    let source = data.get("source")?;
    let spec = data.get("spec")?;
    let runtime = data.get("runtime")?.as_object()?;
    let quality = data.get("quality")?;
    let safety = data.get("safety")?;
    let next_actions = data.get("next_actions")?.as_array()?;

    let mut lines = vec![
        skill.to_string(),
        format!("Source:   {}", render_inspect_source(source)),
        format!("Spec:     {}", render_inspect_spec(spec)),
        format!("Runtime:  {}", render_inspect_runtime(runtime)),
        format!("Quality:  {}", render_inspect_quality(quality)),
        format!("Safety:   {}", render_inspect_safety(safety)),
    ];
    if let Some(action) = next_actions.iter().find_map(Value::as_str) {
        lines.push(format!("Next:     {action}"));
    } else {
        lines.push("Next:     none".to_string());
    }
    Some(lines.join("\n"))
}

fn render_inspect_source(source: &Value) -> String {
    let presence = if bool_field(source, "exists") {
        "present"
    } else {
        "missing"
    };
    let drift = if bool_field(source, "working_tree_drift") {
        "drift"
    } else {
        "clean"
    };
    let entrypoint = if bool_field(source, "entrypoint_exists") {
        None
    } else {
        Some("entrypoint missing")
    };
    join_status_parts([Some(presence), Some(drift), entrypoint])
}

fn render_inspect_spec(spec: &Value) -> String {
    let portable = str_field(spec, "portable").unwrap_or("unknown");
    let codex = str_field(spec, "codex").unwrap_or("unknown");
    let claude = str_field(spec, "claude").unwrap_or("unknown");
    format!("portable {portable}, codex {codex}, claude {claude}")
}

fn render_inspect_runtime(runtime: &serde_json::Map<String, Value>) -> String {
    if runtime.is_empty() {
        return "not checked".to_string();
    }
    runtime
        .iter()
        .map(|(agent, status)| {
            let visible = str_field(status, "visible_to_agent").unwrap_or("unknown");
            let base = if bool_field(status, "projected_to_target") {
                format!("{agent} projected, visibility {visible}")
            } else if bool_field(status, "active_rule_present") {
                format!("{agent} active rule, projection missing, visibility {visible}")
            } else if bool_field(status, "installed_in_registry") {
                format!("{agent} installed, not projected")
            } else {
                format!("{agent} not installed")
            };
            match status
                .get("materialized_path_exists")
                .and_then(Value::as_bool)
            {
                Some(false) => format!("{base}, materialized missing"),
                _ => base,
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn render_inspect_quality(quality: &Value) -> String {
    match str_field(quality, "last_eval") {
        Some(last_eval) => format!("last eval {last_eval}"),
        None => "no eval evidence".to_string(),
    }
}

fn render_inspect_safety(safety: &Value) -> String {
    let trust = str_field(safety, "trust").unwrap_or("unknown");
    let policy = str_field(safety, "policy").unwrap_or("unknown");
    if trust == policy {
        trust.to_string()
    } else {
        format!("{trust}, policy {policy}")
    }
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn join_status_parts<const N: usize>(parts: [Option<&str>; N]) -> String {
    parts.into_iter().flatten().collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::{pretty_json_or_empty_object, render_skill_inspect_card};
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
    fn render_skill_inspect_card_formats_status_model() {
        let rendered = render_skill_inspect_card(&json!({
            "skill": "demo",
            "source": {
                "exists": true,
                "entrypoint_exists": true,
                "working_tree_drift": false
            },
            "spec": {
                "portable": "pass",
                "codex": "pass",
                "claude": "warning"
            },
            "runtime": {
                "codex": {
                    "installed_in_registry": true,
                    "active_rule_present": true,
                    "projected_to_target": true,
                    "materialized_path_exists": true,
                    "visible_to_agent": "unknown"
                }
            },
            "quality": {
                "last_eval": null
            },
            "safety": {
                "trust": "unknown",
                "policy": "unknown"
            },
            "next_actions": ["loom skill diagnose demo"]
        }))
        .expect("render card");

        assert!(rendered.contains("demo\n"));
        assert!(rendered.contains("Source:   present, clean"));
        assert!(rendered.contains("Spec:     portable pass, codex pass, claude warning"));
        assert!(rendered.contains("Runtime:  codex projected, visibility unknown"));
        assert!(rendered.contains("Next:     loom skill diagnose demo"));
    }
}
