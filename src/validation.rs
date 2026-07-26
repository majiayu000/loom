//! Centralized input validation for user-supplied identifiers.
//!
//! These rules are shared by the CLI commands and the panel HTTP handlers so
//! that every entry point enforces the same (strictest) contract:
//!
//! - Skill names: ASCII `[A-Za-z0-9._-]`, not `.` or `..`, at most
//!   [`MAX_SKILL_NAME_LEN`] bytes. The charset excludes `/` and `\\`, so a
//!   valid name is always a single path component.
//! - Git revisions: ASCII `[A-Za-z0-9._/~^-]`, no leading `-` (prevents
//!   option injection into `git` argv), no leading/trailing `.`, no `..`
//!   (prevents range smuggling), at most [`MAX_GIT_REF_LEN`] bytes.
//! - Policy profiles: `[a-z0-9_-]{1,64}`.

use anyhow::{Result, anyhow};

/// Maximum accepted length (in bytes) for a skill name.
pub(crate) const MAX_SKILL_NAME_LEN: usize = 255;

/// Maximum accepted length (in bytes) for a git revision argument.
pub(crate) const MAX_GIT_REF_LEN: usize = 256;

/// Maximum accepted length (in bytes) for a policy profile.
pub(crate) const MAX_POLICY_PROFILE_LEN: usize = 64;

/// Validate a registry skill name, returning a descriptive error on failure.
pub(crate) fn validate_skill_name(skill: &str) -> Result<()> {
    if skill.is_empty() {
        return Err(anyhow!("skill name cannot be empty"));
    }
    if skill.len() > MAX_SKILL_NAME_LEN {
        return Err(anyhow!(
            "skill name must be at most {} bytes",
            MAX_SKILL_NAME_LEN
        ));
    }
    if skill == "." || skill == ".." {
        return Err(anyhow!("skill name cannot be '.' or '..'"));
    }
    if skill
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
    {
        return Err(anyhow!(
            "skill name '{}' contains unsupported characters; use [A-Za-z0-9._-]",
            skill
        ));
    }
    Ok(())
}

/// Boolean form of [`validate_skill_name`] for handlers that map failures to
/// their own error envelope.
pub(crate) fn is_valid_skill_name(skill: &str) -> bool {
    validate_skill_name(skill).is_ok()
}

/// Returns `true` when `rev` is safe to pass to `git` as a revision argument.
///
/// Rejects option-shaped values (leading `-`), revision ranges (`..`), and
/// leading/trailing `.` in addition to enforcing an ASCII whitelist.
pub(crate) fn is_safe_git_ref(rev: &str) -> bool {
    !rev.is_empty()
        && rev.len() <= MAX_GIT_REF_LEN
        && !rev.starts_with('-')
        && !rev.starts_with('.')
        && !rev.ends_with('.')
        && !rev.contains("..")
        && rev.bytes().all(|b| {
            matches!(
                b,
                b'a'..=b'z'
                    | b'A'..=b'Z'
                    | b'0'..=b'9'
                    | b'.'
                    | b'_'
                    | b'-'
                    | b'/'
                    | b'~'
                    | b'^'
            )
        })
}

/// Returns a static error message when `value` is not a valid policy
/// profile, or `None` when it is valid.
pub(crate) fn policy_profile_error(value: &str) -> Option<&'static str> {
    if !(1..=MAX_POLICY_PROFILE_LEN).contains(&value.len()) {
        return Some("--policy-profile must be 1-64 characters");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
    {
        return Some("--policy-profile must match [a-z0-9_-]{1,64}");
    }
    None
}

/// Boolean form of [`policy_profile_error`].
pub(crate) fn is_valid_policy_profile(value: &str) -> bool {
    policy_profile_error(value).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_name_accepts_common_forms() {
        for name in [
            "demo",
            "foo-bar_baz.v2",
            "A1",
            "skill.name",
            "a".repeat(255).as_str(),
        ] {
            assert!(is_valid_skill_name(name), "{name:?} should be accepted");
        }
    }

    #[test]
    fn skill_name_rejects_traversal_and_bad_charset() {
        for name in [
            "",
            ".",
            "..",
            "foo/bar",
            "foo\\bar",
            "../etc",
            "foo bar",
            "多词",
            "-flag\u{0}",
            "a".repeat(256).as_str(),
        ] {
            assert!(!is_valid_skill_name(name), "{name:?} should be rejected");
        }
    }

    #[test]
    fn git_ref_accepts_common_revisions() {
        for rev in [
            "HEAD",
            "HEAD~1",
            "HEAD^",
            "main",
            "release/demo/v1.0.0",
            "v1.0.0",
            "abc123def",
            "feature_branch-2",
        ] {
            assert!(is_safe_git_ref(rev), "{rev:?} should be accepted");
        }
    }

    #[test]
    fn git_ref_rejects_option_shaped_and_range_values() {
        for rev in [
            "",
            "-p",
            "--output=/tmp/injected.txt",
            "HEAD..main",
            "..",
            ".hidden",
            "trailing.",
            "a b",
            "rev;rm -rf",
            "$(cmd)",
            "rev\nother",
            "a".repeat(257).as_str(),
        ] {
            assert!(!is_safe_git_ref(rev), "{rev:?} should be rejected");
        }
    }

    #[test]
    fn policy_profile_rules() {
        assert!(is_valid_policy_profile("default"));
        assert!(is_valid_policy_profile("team-a_1"));
        assert!(!is_valid_policy_profile(""));
        assert!(!is_valid_policy_profile("UPPER"));
        assert!(!is_valid_policy_profile("has space"));
        assert!(!is_valid_policy_profile("a".repeat(65).as_str()));
        assert_eq!(
            policy_profile_error(""),
            Some("--policy-profile must be 1-64 characters")
        );
        assert_eq!(
            policy_profile_error("UPPER"),
            Some("--policy-profile must match [a-z0-9_-]{1,64}")
        );
    }
}
