mod cli;
mod commands;
mod envelope;
mod fs_util;
mod gitops;
mod panel;
mod state;
mod state_model;
mod types;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::commands::App;
use crate::envelope::Envelope;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

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
            print_envelope(&env, cli.json);
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

fn print_envelope(env: &Envelope, force_json: bool) {
    if force_json {
        match serde_json::to_string_pretty(env) {
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
