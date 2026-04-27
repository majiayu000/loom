use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

const FRONTEND_INPUT_FILES: &[&str] = &[
    "package.json",
    "bun.lock",
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

    let dist_was_fresh = panel_dist_is_fresh(&panel_dir, &source_dist);
    if !dist_was_fresh {
        let _ = maybe_build_panel(&panel_dir);
    }

    if panel_dist_is_fresh(&panel_dir, &source_dist) {
        copy_dir_recursive(&source_dist, &embedded_dist);
        println!("cargo:rustc-env=LOOM_PANEL_EMBED_STATUS=ready");
    } else {
        println!(
            "cargo:warning=panel frontend assets missing or stale; 'loom panel' will be unavailable unless 'bun run build' succeeds during build"
        );
        println!("cargo:rustc-env=LOOM_PANEL_EMBED_STATUS=missing");
    }
}

fn maybe_build_panel(panel_dir: &Path) -> bool {
    let bun = if cfg!(windows) { "bun.exe" } else { "bun" };

    let install_status = Command::new(bun)
        .arg("install")
        .arg("--frozen-lockfile")
        .current_dir(panel_dir)
        .status();

    let Ok(install_status) = install_status else {
        return false;
    };
    if !install_status.success() {
        return false;
    }

    Command::new(bun)
        .arg("run")
        .arg("build")
        .current_dir(panel_dir)
        .status()
        .is_ok_and(|status| status.success())
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
    let entries = fs::read_dir(source).expect("read source dist dir");
    for entry in entries {
        let entry = entry.expect("read dist entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("read dist file type");
        if file_type.is_symlink() {
            panic!(
                "refusing to embed symlinked panel asset '{}'",
                display_path(&source_path)
            );
        }
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path).expect("create embedded subdir");
            copy_dir_recursive(&source_path, &destination_path);
            continue;
        }
        if file_type.is_file() {
            fs::copy(&source_path, &destination_path).unwrap_or_else(|err| {
                panic!(
                    "copy panel asset '{}' -> '{}' failed: {}",
                    display_path(&source_path),
                    display_path(&destination_path),
                    err
                )
            });
        }
    }
}

fn display_path(path: &Path) -> String {
    path.as_os_str()
        .to_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.as_os_str().to_string_lossy().into_owned())
}
