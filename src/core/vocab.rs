#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionMethod {
    Copy,
    Materialize,
    Symlink,
}

impl From<crate::cli::ProjectionMethod> for ProjectionMethod {
    fn from(value: crate::cli::ProjectionMethod) -> Self {
        match value {
            crate::cli::ProjectionMethod::Copy => Self::Copy,
            crate::cli::ProjectionMethod::Materialize => Self::Materialize,
            crate::cli::ProjectionMethod::Symlink => Self::Symlink,
        }
    }
}

impl From<ProjectionMethod> for crate::cli::ProjectionMethod {
    fn from(value: ProjectionMethod) -> Self {
        match value {
            ProjectionMethod::Copy => Self::Copy,
            ProjectionMethod::Materialize => Self::Materialize,
            ProjectionMethod::Symlink => Self::Symlink,
        }
    }
}
