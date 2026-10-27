/// Publication fact for a staged directory tree.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalTreePublication {
    /// The stage was not published.
    NotPublished,
    /// The stage was published under its destination name.
    Published,
    /// The final namespace state could not be determined.
    Indeterminate,
}
