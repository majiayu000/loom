use super::{
    artifact::inspect_shell_export_artifact_text, model::ProvisionPlan,
    tar_artifact::import_tar_files, utils::digest_str,
};
use crate::{
    commands::{CommandFailure, helpers::map_io},
    types::ErrorCode,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};
use uuid::Uuid;

pub(super) fn export_directory(
    plan: &ProvisionPlan,
    output: &Path,
) -> Result<Value, CommandFailure> {
    let mut files = BTreeMap::new();
    for file in &plan.files_to_write {
        if digest_str(&file.preview) != file.content_digest {
            return Err(invalid("reviewed file digest mismatch"));
        }
        if files
            .insert(file.path.clone(), file.preview.as_bytes().to_vec())
            .is_some()
        {
            return Err(invalid("duplicate target path"));
        }
    }
    write_directory(output, files)
}

pub(super) fn import_directory(artifact: &Path, output: &Path) -> Result<Value, CommandFailure> {
    let raw = fs::read(artifact).map_err(map_io)?;
    let files = if raw.starts_with(b"# loom-provision-artifact-v1") {
        let text = String::from_utf8(raw).map_err(map_io)?;
        let inspected = inspect_shell_export_artifact_text(&text)?;
        let (_, body) = text
            .split_once("\n\n")
            .ok_or_else(|| invalid("missing shell artifact body"))?;
        BTreeMap::from([(
            inspected
                .source_path
                .ok_or_else(|| invalid("missing shell path"))?,
            body.as_bytes().to_vec(),
        )])
    } else {
        import_tar_files(artifact)?
    };
    write_directory(output, files)
}

fn write_directory(
    output: &Path,
    files: BTreeMap<String, Vec<u8>>,
) -> Result<Value, CommandFailure> {
    match fs::symlink_metadata(output) {
        Ok(_) => return Err(invalid("output already exists; choose a new directory")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(map_io(err)),
    }
    for path in files.keys() {
        if path.is_empty()
            || path.contains('\\')
            || Path::new(path)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || Path::new(path)
                .components()
                .any(|c| matches!(c,Component::Normal(p) if p==".git" || p==".env"))
        {
            return Err(invalid("artifact contains an unsafe destination path"));
        }
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(map_io)?;
    let staging = parent.join(format!(".loom-provision-import-{}", Uuid::new_v4()));
    fs::create_dir(&staging).map_err(map_io)?;
    let result = (|| {
        for (path, bytes) in &files {
            let path = staging.join(path);
            fs::create_dir_all(
                path.parent()
                    .ok_or_else(|| invalid("missing destination parent"))?,
            )
            .map_err(map_io)?;
            fs::write(path, bytes).map_err(map_io)?;
        }
        if fs::symlink_metadata(output).is_ok() {
            return Err(invalid("output appeared during import"));
        }
        fs::rename(&staging, output).map_err(map_io)?;
        Ok(
            json!({"output":output,"files":files.keys().collect::<Vec<_>>(),"target_writes_performed":true,"scripts_executed":false}),
        )
    })();
    if result.is_err() {
        fs::remove_dir_all(&staging).map_err(map_io)?;
    }
    result
}
fn invalid(message: &str) -> CommandFailure {
    CommandFailure::new(ErrorCode::ArgInvalid, message)
}
