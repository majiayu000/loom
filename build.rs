use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

    if !source_dist.join("index.html").is_file() {
        maybe_build_panel(&panel_dir);
    }

    if source_dist.join("index.html").is_file() {
        copy_dir_recursive(&source_dist, &embedded_dist);
        println!("cargo:rustc-env=LOOM_PANEL_EMBED_STATUS=ready");
    } else {
        println!(
            "cargo:warning=panel frontend assets missing; 'loom panel' will be unavailable unless 'bun run build' succeeds during build"
        );
        println!("cargo:rustc-env=LOOM_PANEL_EMBED_STATUS=missing");
    }
}

fn maybe_build_panel(panel_dir: &Path) {
    let bun = if cfg!(windows) { "bun.exe" } else { "bun" };

    let install_status = Command::new(bun)
        .arg("install")
        .arg("--frozen-lockfile")
        .current_dir(panel_dir)
        .status();

    let Ok(install_status) = install_status else {
        return;
    };
    if !install_status.success() {
        return;
    }

    let _ = Command::new(bun)
        .arg("run")
        .arg("build")
        .current_dir(panel_dir)
        .status();
}

fn copy_dir_recursive(source: &Path, destination: &Path) {
    let entries = fs::read_dir(source).expect("read source dist dir");
    for entry in entries {
        let entry = entry.expect("read dist entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("read dist file type");
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path).expect("create embedded subdir");
            copy_dir_recursive(&source_path, &destination_path);
            continue;
        }
        if file_type.is_file() || file_type.is_symlink() {
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
