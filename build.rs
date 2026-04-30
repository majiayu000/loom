use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

const FRONTEND_INPUT_FILES: &[&str] = &[
    "package.json",
    "bun.lock",
    "package-lock.json",
    "index.html",
    "landing.html",
    "vite.config.ts",
    "tsconfig.json",
];

const FRONTEND_INPUT_DIRS: &[&str] = &["public", "src"];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let panel_dir = manifest_dir.join("panel");
    let source_dist = panel_dir.join("dist");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("out dir"));
    let embedded_dist = out_dir.join("panel-dist");

    println!("cargo:rerun-if-changed=panel/package.json");
    println!("cargo:rerun-if-changed=panel/bun.lock");
    println!("cargo:rerun-if-changed=panel/package-lock.json");
    println!("cargo:rerun-if-changed=panel/index.html");
    println!("cargo:rerun-if-changed=panel/landing.html");
    println!("cargo:rerun-if-changed=panel/public");
    println!("cargo:rerun-if-changed=panel/src");
    println!("cargo:rerun-if-changed=panel/vite.config.ts");
    println!("cargo:rerun-if-changed=panel/tsconfig.json");

    if embedded_dist.exists() {
        fs::remove_dir_all(&embedded_dist).expect("remove previous embedded panel dir");
    }
    fs::create_dir_all(&embedded_dist).expect("create embedded panel dir");

    if panel_dist_is_fresh(&panel_dir, &source_dist) {
        copy_dir_recursive(&source_dist, &embedded_dist);
        println!("cargo:rustc-env=LOOM_PANEL_EMBED_STATUS=ready");
    } else if let Some(built_dist) = maybe_build_panel_in_out_dir(&panel_dir, &out_dir) {
        copy_dir_recursive(&built_dist, &embedded_dist);
        println!("cargo:rustc-env=LOOM_PANEL_EMBED_STATUS=ready");
    } else {
        println!(
            "cargo:warning=panel frontend assets missing or stale; 'loom panel' will be unavailable unless 'bun run build' succeeds during build"
        );
        println!("cargo:rustc-env=LOOM_PANEL_EMBED_STATUS=missing");
    }
}

fn maybe_build_panel_in_out_dir(panel_dir: &Path, out_dir: &Path) -> Option<PathBuf> {
    let bun = if cfg!(windows) { "bun.exe" } else { "bun" };
    let build_dir = out_dir.join("panel-build");
    if build_dir.exists() {
        fs::remove_dir_all(&build_dir).ok()?;
    }
    fs::create_dir_all(&build_dir).ok()?;

    copy_panel_inputs(panel_dir, &build_dir).ok()?;

    let install_status = Command::new(bun)
        .arg("install")
        .arg("--frozen-lockfile")
        .current_dir(&build_dir)
        .status();

    let Ok(install_status) = install_status else {
        return None;
    };
    if !install_status.success() {
        return None;
    }

    let build_ok = Command::new(bun)
        .arg("run")
        .arg("build")
        .current_dir(&build_dir)
        .status()
        .is_ok_and(|status| status.success());
    if !build_ok {
        return None;
    }

    let built_dist = build_dir.join("dist");
    if panel_dist_is_fresh(&build_dir, &built_dist) {
        Some(built_dist)
    } else {
        None
    }
}

fn copy_panel_inputs(source: &Path, destination: &Path) -> std::io::Result<()> {
    for file in FRONTEND_INPUT_FILES {
        let source_file = source.join(file);
        if source_file.is_file() {
            fs::copy(&source_file, destination.join(file))?;
        }
    }
    for dir in FRONTEND_INPUT_DIRS {
        let source_dir = source.join(dir);
        if source_dir.is_dir() {
            let destination_dir = destination.join(dir);
            fs::create_dir_all(&destination_dir)?;
            copy_dir_recursive_result(&source_dir, &destination_dir)?;
        }
    }
    Ok(())
}

fn panel_dist_is_fresh(panel_dir: &Path, dist_dir: &Path) -> bool {
    let index_html = dist_dir.join("index.html");
    if !index_html.is_file() {
        return false;
    }
    let Some(newest_input) = newest_panel_input(panel_dir) else {
        return true;
    };
    let Some(index_mtime) = path_mtime(&index_html) else {
        return false;
    };
    index_mtime >= newest_input
}

fn newest_panel_input(panel_dir: &Path) -> Option<SystemTime> {
    let mut newest = None;
    for file in FRONTEND_INPUT_FILES {
        update_newest_mtime(panel_dir.join(file), &mut newest);
    }
    for dir in FRONTEND_INPUT_DIRS {
        update_newest_mtime(panel_dir.join(dir), &mut newest);
    }
    newest
}

fn update_newest_mtime(path: impl AsRef<Path>, newest: &mut Option<SystemTime>) {
    let path = path.as_ref();
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.file_type().is_symlink() {
        return;
    }
    if meta.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            update_newest_mtime(entry.path(), newest);
        }
        return;
    }
    let Ok(modified) = meta.modified() else {
        return;
    };
    if newest.is_none_or(|current| modified > current) {
        *newest = Some(modified);
    }
}

fn path_mtime(path: &Path) -> Option<SystemTime> {
    fs::symlink_metadata(path).ok()?.modified().ok()
}

fn copy_dir_recursive(source: &Path, destination: &Path) {
    copy_dir_recursive_result(source, destination).unwrap_or_else(|err| {
        panic!(
            "copy '{}' -> '{}' failed: {}",
            display_path(source),
            display_path(destination),
            err
        )
    });
}

fn copy_dir_recursive_result(source: &Path, destination: &Path) -> std::io::Result<()> {
    let entries = fs::read_dir(source)?;
    for entry in entries {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "refusing to embed symlinked panel asset '{}'",
                    display_path(&source_path)
                ),
            ));
        }
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_dir_recursive_result(&source_path, &destination_path)?;
            continue;
        }
        if file_type.is_file() {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn display_path(path: &Path) -> String {
    path.as_os_str()
        .to_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.as_os_str().to_string_lossy().into_owned())
}
