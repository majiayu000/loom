mod agent_adapters;
mod cli;
mod cli_contract;
mod commands;
mod core;
mod envelope;
mod error_actions;
mod fs_util;
mod gitops;
mod main_runtime;
mod next_action_trace;
mod panel;
mod sha256;
mod state;
mod state_model;
mod types;
#[path = "core/vocab.rs"]
mod vocab;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    main_runtime::run().await;
}
