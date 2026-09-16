mod common;
#[path = "../src/sha256.rs"]
mod sha256;

use common::{TestDir, run_loom, write_skill};
use flate2::read::GzDecoder;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Cursor, Read},
    path::Path,
};
use tar::{Archive, Builder, Header};

fn entries(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let raw = fs::read(path).unwrap();
    let reader: Box<dyn Read> = if raw.starts_with(&[0x1f, 0x8b]) {
        Box::new(GzDecoder::new(Cursor::new(raw)))
    } else {
        Box::new(Cursor::new(raw))
    };
    Archive::new(reader)
        .entries()
        .unwrap()
        .map(|entry| {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_str().unwrap().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (path, bytes)
        })
        .collect()
}
fn fixture(format: &str) -> (TestDir, String, String) {
    let root = TestDir::new("package-native");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing native packages.\n---\n# Demo\nPortable instructions.\n",
    );
    let plan = root.path().join("plan.json").display().to_string();
    let artifact = root.path().join("artifact.tar").display().to_string();
    let (out, value) = run_loom(
        root.path(),
        &[
            "package",
            "plan",
            "skill:demo",
            "--format",
            format,
            "--output-plan",
            &plan,
        ],
    );
    assert!(out.status.success(), "{value}");
    let (out, value) = run_loom(
        root.path(),
        &[
            "package",
            "build",
            &plan,
            "--output",
            &artifact,
            "--idempotency-key",
            "first",
        ],
    );
    assert!(out.status.success(), "{value}");
    (root, plan, artifact)
}
#[test]
fn native_formats_round_trip_preserve_skills_and_rebuild_deterministically() {
    for (format, metadata) in [
        ("codex-plugin", ".codex-plugin/plugin.json"),
        ("claude-plugin", ".claude-plugin/plugin.json"),
        ("npm", "package.json"),
        ("github-release", "release.json"),
    ] {
        let (root, plan, artifact) = fixture(format);
        let files = entries(Path::new(&artifact));
        let (name, body) = files
            .iter()
            .find(|(path, _)| path.ends_with(metadata))
            .unwrap();
        let metadata: Value = serde_json::from_slice(body).unwrap();
        assert_eq!(metadata["name"], "demo");
        assert!(
            files
                .keys()
                .any(|path| path.ends_with("skills/demo/SKILL.md"))
        );
        if format == "npm" {
            assert_eq!(name, "package/package.json");
            assert!(metadata["scripts"].is_null());
            assert!(fs::read(&artifact).unwrap().starts_with(&[0x1f, 0x8b]));
        }
        if format == "codex-plugin" {
            assert_eq!(metadata["skills"], "./skills/");
        }
        let (out, value) = run_loom(
            root.path(),
            &["package", "verify", &artifact, "--format", format],
        );
        assert!(out.status.success(), "{value}");
        let (out, value) = run_loom(
            root.path(),
            &[
                "package",
                "build",
                &plan,
                "--output",
                &artifact,
                "--idempotency-key",
                "first",
            ],
        );
        assert!(out.status.success(), "{value}");
        assert_eq!(value["data"]["idempotent_replay"], true);
        let second = root.path().join("second.tar").display().to_string();
        let (out, value) = run_loom(
            root.path(),
            &[
                "package",
                "build",
                &plan,
                "--output",
                &second,
                "--idempotency-key",
                "second",
            ],
        );
        assert!(out.status.success(), "{value}");
        assert_eq!(fs::read(&artifact).unwrap(), fs::read(second).unwrap());
    }
}
#[test]
fn native_metadata_is_checked_even_when_checksums_are_recomputed() {
    let (root, _plan, artifact) = fixture("codex-plugin");
    for omit in [false, true] {
        let mut files = entries(Path::new(&artifact));
        let path = files
            .keys()
            .find(|path| path.ends_with(".codex-plugin/plugin.json"))
            .unwrap()
            .clone();
        let prefix = path.split('/').next().unwrap().to_string();
        if omit {
            files.remove(&path);
            let manifest = files.get_mut(&format!("{prefix}/manifest.json")).unwrap();
            let mut value: Value = serde_json::from_slice(manifest).unwrap();
            value["files"]
                .as_array_mut()
                .unwrap()
                .retain(|file| file["path"] != ".codex-plugin/plugin.json");
            *manifest = serde_json::to_vec(&value).unwrap();
        } else {
            files.insert(
                path,
                serde_json::to_vec(&json!({"name":"wrong","skills":"../../outside"})).unwrap(),
            );
        }
        let checksum_path = format!("{prefix}/checksums.txt");
        let sums = files
            .iter()
            .filter(|(name, _)| *name != &checksum_path)
            .map(|(name, bytes)| {
                let mut hash = sha256::Sha256::new();
                hash.update(bytes);
                format!(
                    "sha256:{}  {}\n",
                    sha256::to_hex(&hash.finalize()),
                    name.split_once('/').unwrap().1
                )
            })
            .collect::<String>();
        files.insert(checksum_path, sums.into_bytes());
        let bad = root
            .path()
            .join(if omit { "missing.tar" } else { "tampered.tar" });
        let mut tar = Builder::new(File::create(&bad).unwrap());
        for (path, bytes) in files {
            let mut header = Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, path, Cursor::new(bytes))
                .unwrap();
        }
        tar.finish().unwrap();
        drop(tar);
        let (out, value) = run_loom(root.path(), &["package", "verify", bad.to_str().unwrap()]);
        assert!(!out.status.success(), "{value}");
        assert_eq!(value["error"]["code"], "STATE_CORRUPT");
    }
}
#[test]
fn native_format_rejects_wrong_agent() {
    let root = TestDir::new("package-wrong-agent");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing package agent selection.\n---\n# Demo\n",
    );
    let (out, value) = run_loom(
        root.path(),
        &[
            "package",
            "plan",
            "demo",
            "--format",
            "codex-plugin",
            "--agent",
            "claude",
        ],
    );
    assert!(!out.status.success(), "{value}");
    assert_eq!(value["error"]["code"], "ARG_INVALID");
}
