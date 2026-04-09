mod cli;
mod commands;
mod envelope;
mod gitops;
mod panel;
mod state;
mod types;
mod v3;

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
            println!(
                "{}",
                serde_json::to_string_pretty(&env.data).unwrap_or_else(|_| "{}".to_string())
            );
        }
    } else {
        if let Some(err) = &env.error {
            eprintln!("{} failed: {} ({})", env.cmd, err.message, err.code);
            if !err.details.is_null() {
                eprintln!(
                    "{}",
                    serde_json::to_string_pretty(&err.details).unwrap_or_else(|_| "{}".to_string())
                );
            }
        } else {
            eprintln!("{} failed", env.cmd);
        }
    }
}
