use super::*;

pub(super) fn restore_registry_projections_if_owned(
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
