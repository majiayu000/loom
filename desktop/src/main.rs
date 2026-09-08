#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod cloud;
mod local;
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(cloud::CloudState::default())
        .invoke_handler(tauri::generate_handler![
            local::bootstrap,
            local::choose_directory,
            local::choose_file,
            local::local_skills,
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
            cloud::logout
        ])
        .run(tauri::generate_context!())
        .expect("Loom desktop runtime failed");
}
