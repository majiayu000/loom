use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_shell::ShellExt;

type Result<T> = std::result::Result<T, String>;
fn atom(value: &str) -> Result<()> {
    if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err("参数为空或包含无效字符".into());
    }
    Ok(())
}
fn directory(value: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    if !path.is_absolute() || !path.is_dir() {
        return Err("请选择已存在的绝对目录".into());
    }
    path.canonicalize().map_err(|e| e.to_string())
}
async fn run(app: &AppHandle, root: Option<String>, args: Vec<String>) -> Result<Value> {
    let mut argv = vec!["--json".to_string()];
    if let Some(root) = root {
        argv.extend([
            "--root".into(),
            directory(&root)?.to_string_lossy().into_owned(),
        ]);
    }
    argv.extend(args);
    let output = app
        .shell()
        .sidecar("loom")
        .map_err(|e| e.to_string())?
        .args(argv)
        .output()
        .await
        .map_err(|e| format!("内置引擎启动失败：{e}"))?;
    // Existing CLI errors are JSON too. Preserve their envelope rather than replacing them.
    serde_json::from_slice(&output.stdout).map_err(|_| {
        format!(
            "引擎未返回有效 JSON，退出状态 {:?}；写入结果未知，请检查操作记录后重试",
            output.status.code()
        )
    })
}
pub(crate) async fn team_plan(
    app: &AppHandle,
    root: Option<String>,
    name: String,
    archive: PathBuf,
    manifest: PathBuf,
) -> Result<Value> {
    atom(&name)?;
    run(
        app,
        root,
        vec![
            "plan".into(),
            "team-install".into(),
            name,
            "--archive".into(),
            archive.to_string_lossy().into_owned(),
            "--manifest".into(),
            manifest.to_string_lossy().into_owned(),
        ],
    )
    .await
}
#[tauri::command]
pub async fn apply_plan(
    app: AppHandle,
    root: Option<String>,
    plan_id: String,
    plan_digest: String,
    idempotency_key: String,
) -> Result<Value> {
    atom(&plan_id)?;
    atom(&plan_digest)?;
    atom(&idempotency_key)?;
    run(
        &app,
        root,
        vec![
            "apply".into(),
            plan_id,
            "--plan-digest".into(),
            plan_digest,
            "--idempotency-key".into(),
            idempotency_key,
        ],
    )
    .await
}
#[tauri::command]
pub async fn initialize_registry(app: AppHandle, root: Option<String>) -> Result<Value> {
    run(&app, root, vec!["init".into()]).await
}
async fn git(app: &AppHandle) -> Result<String> {
    let output = app
        .shell()
        .command("git")
        .args(["--version"])
        .output()
        .await
        .map_err(|_| "未找到 Git，请先安装 Git".to_string())?;
    if !output.status.success() {
        return Err("Git 检查失败".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}
#[tauri::command]
pub async fn bootstrap(app: AppHandle) -> Value {
    match git(&app).await {
        Ok(version) => json!({"git":{"available":true,"version":version}}),
        Err(message) => json!({"git":{"available":false,"message":message}}),
    }
}
#[tauri::command]
pub async fn choose_directory(app: AppHandle) -> Result<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .map(|p| p.into_path().map(|p| p.to_string_lossy().into_owned()))
            .transpose()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn choose_file(app: AppHandle) -> Result<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_file()
            .map(|p| p.into_path().map(|p| p.to_string_lossy().into_owned()))
            .transpose()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn local_skills(app: AppHandle, root: Option<String>) -> Result<Value> {
    run(&app, root, vec!["skill".into(), "list".into()]).await
}
fn read_args(
    op: &str,
    skill: String,
    agent: Option<String>,
    workspace: Option<String>,
) -> Result<Vec<String>> {
    atom(&skill)?;
    let mut args = vec!["skill".into(), op.into(), skill];
    if let Some(agent) = agent {
        atom(&agent)?;
        args.extend(["--agent".into(), agent]);
    }
    if let Some(workspace) = workspace {
        args.extend([
            "--workspace".into(),
            directory(&workspace)?.to_string_lossy().into_owned(),
        ]);
    }
    Ok(args)
}
#[tauri::command]
pub async fn inspect_skill(
    app: AppHandle,
    root: Option<String>,
    skill: String,
    agent: Option<String>,
    workspace: Option<String>,
) -> Result<Value> {
    run(&app, root, read_args("inspect", skill, agent, workspace)?).await
}
#[tauri::command]
pub async fn deps_skill(
    app: AppHandle,
    root: Option<String>,
    skill: String,
    agent: Option<String>,
    workspace: Option<String>,
) -> Result<Value> {
    run(&app, root, read_args("deps", skill, agent, workspace)?).await
}
#[tauri::command]
pub async fn visibility_skill(
    app: AppHandle,
    root: Option<String>,
    skill: String,
    agent: String,
    workspace: Option<String>,
) -> Result<Value> {
    run(
        &app,
        root,
        read_args("visibility", skill, Some(agent), workspace)?,
    )
    .await
}
fn install_args(source: String, name: String, preview: bool) -> Result<Vec<String>> {
    atom(&name)?;
    let source = directory(&source)?;
    if source.to_string_lossy().contains('@') {
        return Err("目录名含 @，现有 CLI 会将其解析为版本分隔符，请选择不含 @ 的目录".into());
    }
    if !source.join("SKILL.md").is_file() {
        return Err("所选目录缺少 SKILL.md".into());
    }
    let mut args = vec![
        "skill".into(),
        "install".into(),
        format!("local:{}", source.display()),
        "--name".into(),
        name,
    ];
    if preview {
        args.push("--dry-run".into());
    }
    Ok(args)
}
#[tauri::command]
pub async fn preview_install(
    app: AppHandle,
    root: Option<String>,
    source: String,
    name: String,
) -> Result<Value> {
    run(&app, root, install_args(source, name, true)?).await
}
#[tauri::command]
pub async fn apply_install(
    app: AppHandle,
    root: Option<String>,
    source: String,
    name: String,
) -> Result<Value> {
    git(&app).await?;
    run(&app, root, install_args(source, name, false)?).await
}
fn activate_args(
    skill: String,
    agent: String,
    workspace: Option<String>,
    preview: bool,
) -> Result<Vec<String>> {
    let scope = if workspace.is_some() {
        "project"
    } else {
        "user"
    };
    let mut args = read_args("activate", skill, Some(agent), workspace)?;
    args.extend(["--scope".into(), scope.into()]);
    if preview {
        args.push("--dry-run".into());
    }
    Ok(args)
}
#[tauri::command]
pub async fn preview_activate(
    app: AppHandle,
    root: Option<String>,
    skill: String,
    agent: String,
    workspace: Option<String>,
) -> Result<Value> {
    run(&app, root, activate_args(skill, agent, workspace, true)?).await
}
#[tauri::command]
pub async fn apply_activate(
    app: AppHandle,
    root: Option<String>,
    skill: String,
    agent: String,
    workspace: Option<String>,
) -> Result<Value> {
    git(&app).await?;
    run(&app, root, activate_args(skill, agent, workspace, false)?).await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn option_injection_is_rejected() {
        for value in ["", "--root", "x\ny"] {
            assert!(atom(value).is_err());
        }
    }
    #[test]
    fn shell_text_stays_one_argument() {
        let args = read_args("inspect", "hello; touch /tmp/no".into(), None, None).unwrap();
        assert_eq!(args.len(), 3);
    }
    #[test]
    fn relative_directory_rejected() {
        assert!(directory(".").is_err());
    }
}
