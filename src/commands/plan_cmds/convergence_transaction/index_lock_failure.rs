use super::*;

pub(super) fn map_install_error(error: anyhow::Error) -> CommandFailure {
    let retained = gitops::prepared_index_lock_was_retained(&error);
    let mut failure = super::map_git(error);
    if retained {
        failure.details = json!({ "index_lock_retained": true });
    }
    failure
}

pub(super) fn retained(failure: &CommandFailure) -> bool {
    failure
        .details
        .get("index_lock_retained")
        .and_then(Value::as_bool)
        == Some(true)
}
