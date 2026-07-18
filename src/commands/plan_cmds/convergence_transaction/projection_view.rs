use super::*;

#[inline(never)]
pub(super) fn projection_view_digest(
    path: &Path,
    method: &str,
) -> std::result::Result<String, CommandFailure> {
    if method == "materialize" {
        materialized_tree_digest(path).map_err(map_io)
    } else {
        skill_tree_digest(path).map_err(map_io)
    }
}
