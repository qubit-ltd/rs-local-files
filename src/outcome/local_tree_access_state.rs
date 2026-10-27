/// Access policy known to hold for a staged directory tree.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalTreeAccessState {
    /// Existing entry access controls were preserved.
    Preserved,
    /// Access is restricted to the current user.
    CurrentUserOnly,
    /// Access is restricted to the current user and files are read-only.
    ReadOnlyCurrentUser,
    /// A confirmed mixture of access states remains across entries.
    Mixed,
    /// The access state could not be determined safely.
    Indeterminate,
}
