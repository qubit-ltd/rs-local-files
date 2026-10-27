/// Kind of precondition used for conditional publication.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalConditionalKind {
    /// The destination must be absent.
    Absent,
    /// The destination content must match a digest.
    Content,
}
