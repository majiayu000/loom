use super::*;

pub(super) fn restore_registry_state_if_owned(
    paths: &RegistryStatePaths,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    restore_registry_projections_if_owned(paths, journal)?;
    restore_registry_audit_if_owned(paths, journal)
}

fn restore_registry_projections_if_owned(
    paths: &RegistryStatePaths,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let live = paths.load_projections().map_err(map_registry_state)?;
    let live_value = serde_json::to_value(&live).map_err(map_io)?;
    let original_value = serde_json::to_value(&journal.original_projections).map_err(map_io)?;
    if live_value == original_value {
        return Ok(());
    }
    let expected = journal.expected_projections.as_ref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "transaction registry replacement evidence is missing",
        )
    })?;
    if live_value != serde_json::to_value(expected).map_err(map_io)? {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "registry projections changed before rollback compare-and-exchange",
        ));
    }
    if !paths
        .compare_exchange_projections(expected, &journal.original_projections)
        .map_err(map_registry_state)?
    {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "registry projections changed during rollback compare-and-exchange",
        ));
    }
    let restored = paths.load_projections().map_err(map_registry_state)?;
    if serde_json::to_value(restored).map_err(map_io)? != original_value {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "registry projections changed after rollback compare-and-exchange",
        ));
    }
    Ok(())
}

fn restore_registry_audit_if_owned(
    paths: &RegistryStatePaths,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let original_operations = journal.original_operations.as_ref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "transaction registry operations backup is missing",
        )
    })?;
    let original_checkpoint = journal.original_checkpoint.as_ref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "transaction registry checkpoint backup is missing",
        )
    })?;
    let live_operations = paths.load_operations().map_err(map_registry_state)?;
    let live_checkpoint = paths.load_checkpoint().map_err(map_registry_state)?;
    let operations_are_old = same_value(&live_operations, original_operations)?;
    let checkpoint_is_old = same_value(&live_checkpoint, original_checkpoint)?;
    if operations_are_old && checkpoint_is_old {
        return Ok(());
    }

    let aggregate_operation = journal.aggregate_operation.as_ref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "transaction aggregate operation evidence is missing",
        )
    })?;
    let aggregate_checkpoint = journal.aggregate_checkpoint.as_ref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "transaction aggregate checkpoint evidence is missing",
        )
    })?;
    let operations_are_new = live_operations.len() == original_operations.len() + 1
        && same_value(
            &live_operations[..original_operations.len()],
            original_operations,
        )?
        && same_value(
            &live_operations[original_operations.len()],
            aggregate_operation,
        )?;
    let checkpoint_is_new = same_value(&live_checkpoint, aggregate_checkpoint)?;
    if !checkpoint_is_new || (!operations_are_new && !operations_are_old) {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "registry audit changed before rollback compare-and-restore",
        ));
    }
    if operations_are_new {
        paths
            .save_operations(original_operations)
            .map_err(map_registry_state)?;
    }
    paths
        .save_checkpoint(original_checkpoint)
        .map_err(map_registry_state)?;
    if !same_value(
        &paths.load_operations().map_err(map_registry_state)?,
        original_operations,
    )? || !same_value(
        &paths.load_checkpoint().map_err(map_registry_state)?,
        original_checkpoint,
    )? {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "registry audit changed after rollback compare-and-restore",
        ));
    }
    Ok(())
}

fn same_value<T: Serialize + ?Sized>(
    left: &T,
    right: &T,
) -> std::result::Result<bool, CommandFailure> {
    Ok(serde_json::to_value(left).map_err(map_io)?
        == serde_json::to_value(right).map_err(map_io)?)
}
