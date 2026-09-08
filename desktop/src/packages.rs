use crate::{cloud, local};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use flate2::{write::GzEncoder, Compression};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Component, Path},
};
use tauri::{AppHandle, State};
use tempfile::tempdir;

type Result<T> = std::result::Result<T, String>;
const MAX_ARCHIVE: usize = 10 * 1024 * 1024;
const MAX_CONTENT: u64 = 50 * 1024 * 1024;

fn utf8_name(name: &std::ffi::OsStr) -> Result<&str> {
    name.to_str().ok_or_else(|| "文件名必须为 UTF-8".into())
}

fn id(value: &str) -> Result<()> {
    uuid::Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| "云端资源标识无效".into())
}

fn require_existing_team(inspection: &Value, origin: &str, team: &str, skill: &str) -> Result<()> {
    let source = &inspection["data"]["provenance"]["team"];
    if inspection["ok"] != true
        || inspection["data"]["source"]["exists"] != true
        || source["service_origin"] != origin
        || source["team_id"] != team
        || source["skill_id"] != skill
    {
        return Err("已归档技能只允许恢复本机已安装的同来源技能，不能新增安装".into());
    }
    Ok(())
}

fn collect(
    root: &cap_std::fs::Dir,
    relative: &Path,
    files: &mut Vec<(std::path::PathBuf, u64)>,
    total: &mut u64,
    entries: &mut usize,
) -> Result<()> {
    if relative.components().count() > 32 {
        return Err("Skill 目录层级超过限制".into());
    }
    for entry in root.read_dir(".").map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        *entries += 1;
        if *entries > 2000 {
            return Err("Skill 目录条目超过限制".into());
        }
        let filename = entry.file_name();
        let name = utf8_name(&filename)?;
        let path = relative.join(&filename);
        let metadata = root
            .symlink_metadata(&filename)
            .map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() || name == ".git" || name.starts_with(".env") {
            return Err(format!("请移除不应上传的链接或私密文件：{name}"));
        }
        if metadata.is_dir() {
            let child = root
                .open_dir_nofollow(&filename)
                .map_err(|e| e.to_string())?;
            collect(&child, &path, files, total, entries)?;
        } else if metadata.is_file() {
            *total = total.checked_add(metadata.len()).ok_or("Skill 包过大")?;
            if files.len() >= 1000 || *total > MAX_CONTENT {
                return Err("Skill 包超过 1000 文件或 50 MiB 内容限制".into());
            }
            if path
                .components()
                .any(|v| !matches!(v, Component::Normal(_)))
                || path.to_str().ok_or("文件路径必须为 UTF-8")?.contains('\\')
            {
                return Err("文件路径不可移植".into());
            }
            files.push((path, metadata.len()));
        } else {
            return Err("Skill 包含特殊文件，不能上传".into());
        }
    }
    Ok(())
}

// Resolve each parent without following links, then inspect the opened handle.
// O_NONBLOCK prevents a file replaced by a FIFO from hanging the upload worker.
fn open_regular(root: &cap_std::fs::Dir, path: &Path, size: u64) -> Result<std::fs::File> {
    let mut directory = root.try_clone().map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        for part in parent.components() {
            let Component::Normal(part) = part else {
                return Err("文件路径不可移植".into());
            };
            directory = directory
                .open_dir_nofollow(part)
                .map_err(|e| e.to_string())?;
        }
    }
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = directory
        .open_with(path.file_name().ok_or("缺少文件名")?, &options)
        .map_err(|e| e.to_string())?
        .into_std();
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() != size {
        return Err("发布前文件发生变化，请重新预览".into());
    }
    Ok(file)
}

fn pack(source: &str) -> Result<(Vec<u8>, Value)> {
    let root = Path::new(source);
    if !root.is_absolute() || !root.is_dir() {
        return Err("请选择 Skill 的绝对目录".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    let mut total = 0;
    let directory = cap_std::fs::Dir::open_ambient_dir(&root, cap_std::ambient_authority())
        .map_err(|e| e.to_string())?;
    collect(&directory, Path::new(""), &mut files, &mut total, &mut 0)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    if !files.iter().any(|(p, _)| p == Path::new("SKILL.md")) {
        return Err("包根目录缺少普通 SKILL.md 文件".into());
    }
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (path, size) in &files {
        let mut bytes = Vec::new();
        let file = open_regular(&directory, path, *size)?;
        let mut mode = 0o644;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if file
                .metadata()
                .map_err(|e| e.to_string())?
                .permissions()
                .mode()
                & 0o111
                != 0
            {
                mode = 0o755;
            }
        }
        file.take(size + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 != *size {
            return Err("读取期间文件发生变化，请重新预览".into());
        }
        let mut header = tar::Header::new_gnu();
        header.set_size(*size);
        header.set_mode(mode);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(&mut header, path, bytes.as_slice())
            .map_err(|e| e.to_string())?;
    }
    let bytes = tar
        .into_inner()
        .map_err(|e| e.to_string())?
        .finish()
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_ARCHIVE {
        return Err("压缩包超过 10 MiB 限制".into());
    }
    let manifest = json!({"files":files.iter().map(|(p,size)|json!({"path":p.to_string_lossy(),"size":size})).collect::<Vec<_>>(),"size_bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))});
    Ok((bytes, manifest))
}

#[tauri::command]
pub async fn preview_publish(source: String) -> Result<Value> {
    tauri::async_runtime::spawn_blocking(move || pack(&source).map(|(_, manifest)| manifest))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn publish_skill(
    state: State<'_, cloud::CloudState>,
    team: String,
    skill: Option<String>,
    source: String,
    metadata: Value,
    expected_sha256: String,
    idempotency_key: String,
) -> Result<Value> {
    id(&team)?;
    if let Some(value) = &skill {
        id(value)?;
    }
    let (bytes, manifest) = tauri::async_runtime::spawn_blocking(move || pack(&source))
        .await
        .map_err(|e| e.to_string())??;
    if manifest["sha256"].as_str() != Some(&expected_sha256) {
        return Err("Skill 内容在预览后发生变化，请重新预览".into());
    }
    let path = match skill {
        Some(skill) => format!("/v1/teams/{team}/skills/{skill}/versions"),
        None => format!("/v1/teams/{team}/skills"),
    };
    cloud::response_json(
        cloud::authorized_response(
            &state,
            "POST",
            &path,
            Some(metadata),
            Some(bytes),
            Some(idempotency_key),
            None,
        )
        .await?,
    )
    .await
}

#[tauri::command]
pub async fn preview_team_install(
    app: AppHandle,
    state: State<'_, cloud::CloudState>,
    team: String,
    skill: String,
    version: String,
    name: String,
    root: Option<String>,
    requested_ref: Option<String>,
) -> Result<Value> {
    id(&team)?;
    id(&skill)?;
    id(&version)?;
    let skill_response = cloud::authorized_response(
        &state,
        "GET",
        &format!("/v1/teams/{team}/skills/{skill}"),
        None,
        None,
        None,
        None,
    )
    .await?;
    let origin = skill_response.url().origin().ascii_serialization();
    let skill_metadata = cloud::response_json(skill_response).await?;
    let archived = !skill_metadata["data"]["skill"]["archived_at"].is_null();
    if archived {
        let inspection =
            local::inspect_skill(app.clone(), root.clone(), name.clone(), None, None).await?;
        require_existing_team(&inspection, &origin, &team, &skill)?;
    }
    let path = format!("/v1/teams/{team}/skills/{skill}/versions/{version}");
    let metadata_response =
        cloud::authorized_response(&state, "GET", &path, None, None, None, None).await?;
    if metadata_response.url().origin().ascii_serialization() != origin {
        return Err("服务配置在预览过程中变化，请重试".into());
    }
    let metadata = cloud::response_json(metadata_response).await?;
    let sha = metadata["data"]["version"]["sha256"]
        .as_str()
        .ok_or("版本响应缺少校验值")?;
    let mut response = cloud::authorized_response(
        &state,
        "GET",
        &format!("{path}/artifact"),
        None,
        None,
        None,
        None,
    )
    .await?;
    if response.url().origin().ascii_serialization() != origin {
        return Err("服务配置在下载过程中变化，请重新预览".into());
    }
    if !response.status().is_success() {
        return cloud::response_json(response).await;
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_ARCHIVE as u64)
    {
        return Err("下载包超过限制".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "包下载中断，请重试")? {
        if bytes.len() + chunk.len() > MAX_ARCHIVE {
            return Err("下载包超过限制".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if format!("{:x}", Sha256::digest(&bytes)) != sha {
        return Err("下载内容与云端版本校验值不匹配".into());
    }
    let staging = tempdir().map_err(|e| e.to_string())?;
    let archive = staging.path().join("skill.tar.gz");
    let manifest_path = staging.path().join("manifest.json");
    fs::write(&archive, bytes).map_err(|e| e.to_string())?;
    fs::write(&manifest_path,serde_json::to_vec(&json!({"service_origin":origin,"team_id":team,"skill_id":skill,"version_id":version,"sha256":sha,"requested_ref":requested_ref.unwrap_or_else(||version.clone())})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let plan = local::team_plan(&app, root, name, archive, manifest_path).await?;
    if archived
        && plan["ok"] == true
        && (plan["data"]["source"]["direction"] != "team"
            || !plan["data"]["source"]["tree_digest"]
                .as_str()
                .is_some_and(|digest| digest.starts_with("sha256:")))
    {
        return Err("本机安装在预览过程中变化，归档技能不能新增安装".into());
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archived_restore_requires_existing_matching_identity() {
        let original = json!({"ok":true,"data":{"source":{"exists":true},"provenance":{"team":{
            "service_origin":"https://team.example","team_id":"team-a","skill_id":"skill-a"
        }}}});
        assert!(
            require_existing_team(&original, "https://team.example", "team-a", "skill-a").is_ok()
        );
        assert!(
            require_existing_team(&original, "https://other.example", "team-a", "skill-a").is_err()
        );
        assert!(
            require_existing_team(&original, "https://team.example", "team-b", "skill-a").is_err()
        );
        let mut missing = original.clone();
        missing["data"]["source"]["exists"] = json!(false);
        assert!(
            require_existing_team(&missing, "https://team.example", "team-a", "skill-a").is_err()
        );
    }
    #[test]
    fn package_is_deterministic_and_rejects_private_files() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("SKILL.md"),
            "---\nname: demo\ndescription: demo\n---\nHello",
        )
        .unwrap();
        let a = pack(temp.path().to_str().unwrap()).unwrap();
        let b = pack(temp.path().to_str().unwrap()).unwrap();
        assert_eq!(a.0, b.0);
        fs::write(temp.path().join(".env"), "TEST=private").unwrap();
        assert!(pack(temp.path().to_str().unwrap()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn package_rejects_symlinks() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("SKILL.md"), "hello").unwrap();
        std::os::unix::fs::symlink("SKILL.md", temp.path().join("linked")).unwrap();
        assert!(pack(temp.path().to_str().unwrap()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn opening_rejects_replaced_links_and_fifo() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("SKILL.md"), "hello").unwrap();
        let root =
            cap_std::fs::Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        std::os::unix::fs::symlink("SKILL.md", temp.path().join("replaced")).unwrap();
        assert!(open_regular(&root, Path::new("replaced"), 5).is_err());
        fs::create_dir(temp.path().join("sub")).unwrap();
        fs::write(temp.path().join("sub/file"), "hello").unwrap();
        std::os::unix::fs::symlink("sub", temp.path().join("ancestor")).unwrap();
        assert!(open_regular(&root, Path::new("ancestor/file"), 5).is_err());
        let status = std::process::Command::new("mkfifo")
            .arg(temp.path().join("fifo"))
            .status()
            .unwrap();
        assert!(status.success());
        assert!(open_regular(&root, Path::new("fifo"), 0).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn package_rejects_non_utf8_names_before_preview() {
        use std::os::unix::ffi::OsStringExt;
        // APFS rejects these bytes at creation; exercise the same collection
        // boundary directly so the test also runs on macOS.
        let name = std::ffi::OsString::from_vec(vec![0xff]);
        assert!(utf8_name(&name).unwrap_err().contains("UTF-8"));
    }
}
