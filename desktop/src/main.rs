#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[cfg(debug_assertions)]
use tauri::Manager;
mod cloud;
mod local;
mod packages;
fn main() {
    tauri::Builder::default()
        .setup(|app| {
            #[cfg(debug_assertions)]
            cloud::initialize_development_session(app.path().app_config_dir()?)
                .map_err(std::io::Error::other)?;
            Ok(())
        })
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(cloud::CloudState::default())
        .invoke_handler(tauri::generate_handler![
            local::bootstrap,
            local::choose_directory,
            local::choose_file,
            local::local_skills,
            local::local_operations,
            local::diagnose_skill,
            local::history_skill,
            local::diff_skill,
            local::inspect_skill,
            local::deps_skill,
            local::visibility_skill,
            local::preview_install,
            local::apply_install,
            local::preview_activate,
            local::apply_activate,
            cloud::get_cloud_config,
            cloud::save_cloud_config,
            cloud::request_otp,
            cloud::verify_otp,
            cloud::current_user,
            cloud::cloud_request,
            cloud::logout,
            packages::preview_publish,
            packages::publish_skill,
            packages::preview_team_install,
            local::apply_plan,
            local::initialize_registry
        ])
        .run(tauri::generate_context!())
        .expect("Loom desktop runtime failed");
}
