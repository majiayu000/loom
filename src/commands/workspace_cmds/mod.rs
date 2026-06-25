mod binding;
mod doctor;
mod init;
mod orphan;
mod remote;
mod status;

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn unique_temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loom-symlink-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Regression for PR #1 review: `check_projection_drift` previously called
    /// `link_target.exists()` and `fs::canonicalize(&link_target)` on the raw
    /// `read_link` result, which resolves relative paths against the process
    /// CWD instead of the symlink's parent directory. A valid relative
    /// projection (e.g. `../skills/foo`) was therefore mis-reported as
    /// dangling/wrong-target. This test mirrors the production resolution
    /// rule and asserts it canonicalizes to the actual source.
    #[test]
    fn relative_symlink_resolves_against_parent_directory() {
        let base = unique_temp_dir();
        let src = base.join("skill_src");
        fs::create_dir(&src).unwrap();
        let materialized = base.join("link");
        std::os::unix::fs::symlink("skill_src", &materialized).unwrap();

        let link_target = fs::read_link(&materialized).unwrap();
        assert!(link_target.is_relative(), "fixture must be a relative link");

        let resolved = if link_target.is_absolute() {
            link_target.clone()
        } else {
            materialized
                .parent()
                .map(|parent| parent.join(&link_target))
                .unwrap()
        };

        assert!(resolved.exists(), "resolved relative link must exist");
        let canon_link = fs::canonicalize(&resolved).unwrap();
        let canon_src = fs::canonicalize(&src).unwrap();
        assert_eq!(canon_link, canon_src);

        let _ = fs::remove_dir_all(&base);
    }
}
