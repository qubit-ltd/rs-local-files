/// Progress of a directory-tree mutation operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalTreeMutationState {
    /// No namespace mutation was attempted.
    NotStarted,
    /// A namespace mutation was attempted.
    Started,
    /// The mutation result could not be determined.
    Indeterminate,
}
