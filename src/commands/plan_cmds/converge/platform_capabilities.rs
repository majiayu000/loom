use std::path::Path;

use serde_json::json;

use crate::core::convergence::{
    ConvergenceInputConflict, ConvergenceInputDirection, ProjectionEffectPlan,
};

use super::projection_path_is_safe_symlink;

#[derive(Clone, Copy)]
struct PlatformCapabilities {
    exchange: bool,
    no_replace: bool,
    directory_handles: bool,
}

impl PlatformCapabilities {
    fn current() -> Self {
        Self {
            exchange: crate::fs_util::atomic_path_exchange_supported(),
            no_replace: crate::fs_util::atomic_no_replace_supported(),
            directory_handles: crate::fs_util::handle_relative_directory_operations_supported(),
        }
    }
}

pub(super) fn resolve_platform_capability_conflicts(
    direction: &ConvergenceInputDirection,
    effects: &[ProjectionEffectPlan],
    canonical_source: &Path,
) -> Vec<ConvergenceInputConflict> {
    resolve_with(
        direction,
        effects,
        canonical_source,
        PlatformCapabilities::current(),
    )
}

fn resolve_with(
    direction: &ConvergenceInputDirection,
    effects: &[ProjectionEffectPlan],
    canonical_source: &Path,
    capabilities: PlatformCapabilities,
) -> Vec<ConvergenceInputConflict> {
    let mut conflicts = Vec::new();
    if !capabilities.no_replace {
        conflicts.push(ConvergenceInputConflict {
            code: "PLATFORM_ATOMIC_TRANSACTION_OWNERSHIP_UNSUPPORTED".to_string(),
            message: "this platform cannot atomically claim convergence transaction ownership"
                .to_string(),
            evidence: json!({ "required_operation": "atomic_no_replace" }),
        });
    }
    if !effects.is_empty() && !capabilities.directory_handles {
        conflicts.push(ConvergenceInputConflict {
            code: "PLATFORM_DIRECTORY_HANDLE_UNSUPPORTED".to_string(),
            message: "this platform cannot keep convergence filesystem operations bound to opened directories"
                .to_string(),
            evidence: json!({ "required_operation": "handle_relative_directory_operations" }),
        });
    }
    if *direction == ConvergenceInputDirection::Projection && !capabilities.exchange {
        conflicts.push(ConvergenceInputConflict {
            code: "PLATFORM_ATOMIC_SOURCE_EXCHANGE_UNSUPPORTED".to_string(),
            message: "this platform cannot atomically replace the canonical source from a projection input"
                .to_string(),
            evidence: json!({ "required_operation": "atomic_path_exchange" }),
        });
    }

    let unsupported_effects = effects
        .iter()
        .filter(|effect| {
            let safe_symlink_noop = effect.effect == "refresh"
                && effect.method == "symlink"
                && projection_path_is_safe_symlink(
                    Path::new(&effect.materialized_path),
                    canonical_source,
                );
            requires_unsupported_activation(effect, safe_symlink_noop, capabilities)
        })
        .map(|effect| effect.instance_id.clone())
        .collect::<Vec<_>>();
    if !unsupported_effects.is_empty() {
        conflicts.push(ConvergenceInputConflict {
            code: "PLATFORM_ATOMIC_PROJECTION_ACTIVATION_UNSUPPORTED".to_string(),
            message: "this platform cannot atomically activate every planned projection"
                .to_string(),
            evidence: json!({ "projection_instances": unsupported_effects }),
        });
    }
    conflicts
}

fn requires_unsupported_activation(
    effect: &ProjectionEffectPlan,
    safe_symlink_noop: bool,
    capabilities: PlatformCapabilities,
) -> bool {
    if safe_symlink_noop {
        return false;
    }
    match effect.effect.as_str() {
        "create" => !capabilities.no_replace,
        "refresh" => !capabilities.exchange,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_ownership_is_required_without_projection_effects() {
        let conflicts = resolve_with(
            &ConvergenceInputDirection::Source,
            &[],
            Path::new("unused-source"),
            PlatformCapabilities {
                exchange: false,
                no_replace: false,
                directory_handles: true,
            },
        );
        assert!(conflicts.iter().any(|conflict| {
            conflict.code == "PLATFORM_ATOMIC_TRANSACTION_OWNERSHIP_UNSUPPORTED"
        }));
    }

    #[test]
    fn source_only_plans_do_not_require_projection_directory_handles() {
        let conflicts = resolve_with(
            &ConvergenceInputDirection::Source,
            &[],
            Path::new("unused-source"),
            PlatformCapabilities {
                exchange: true,
                no_replace: true,
                directory_handles: false,
            },
        );
        assert!(
            !conflicts
                .iter()
                .any(|conflict| { conflict.code == "PLATFORM_DIRECTORY_HANDLE_UNSUPPORTED" })
        );
    }

    #[test]
    fn projection_effects_require_directory_handles() {
        let effect = ProjectionEffectPlan {
            instance_id: "inst_demo".to_string(),
            binding_id: "bind_demo".to_string(),
            target_id: "target_demo".to_string(),
            agent: "codex".to_string(),
            profile: "default".to_string(),
            method: "copy".to_string(),
            ownership: "managed".to_string(),
            materialized_path: "unused-projection".to_string(),
            source_tree_digest: "sha256:unused".to_string(),
            materialized_tree_digest: None,
            effect: "create".to_string(),
        };
        let conflicts = resolve_with(
            &ConvergenceInputDirection::Source,
            &[effect],
            Path::new("unused-source"),
            PlatformCapabilities {
                exchange: true,
                no_replace: true,
                directory_handles: false,
            },
        );
        assert!(
            conflicts
                .iter()
                .any(|conflict| { conflict.code == "PLATFORM_DIRECTORY_HANDLE_UNSUPPORTED" })
        );
    }
}
