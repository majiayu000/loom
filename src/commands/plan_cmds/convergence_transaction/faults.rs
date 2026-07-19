pub(super) fn interruption_fault_active() -> bool {
    matches!(
        std::env::var("LOOM_FAULT_INJECT").ok().as_deref(),
        Some(
            "convergence_interrupt_after_source_commit"
                | "convergence_interrupt_after_source_cas"
                | "convergence_interrupt_committing_source"
                | "convergence_interrupt_committing_registry"
                | "convergence_interrupt_after_owner_root_creation"
                | "convergence_interrupt_after_owner_marker_write"
                | "convergence_interrupt_before_index_snapshot"
                | "convergence_interrupt_after_index_snapshot"
                | "convergence_interrupt_after_index_snapshot_digest"
                | "convergence_interrupt_after_prepared"
                | "convergence_interrupt_after_projection_stage"
                | "convergence_interrupt_after_declared_backup"
                | "convergence_interrupt_after_source_replacement"
                | "convergence_interrupt_after_source_add"
                | "convergence_interrupt_after_staged_index_prepared"
                | "convergence_interrupt_after_staged_index_install"
                | "convergence_interrupt_after_projection_activation"
                | "convergence_interrupt_after_projection_swap"
                | "convergence_interrupt_after_projection_restore_wal"
                | "convergence_interrupt_after_registry_save_cas"
                | "convergence_interrupt_before_registry_cas"
                | "convergence_interrupt_after_reservation_pending_create"
        )
    )
}
