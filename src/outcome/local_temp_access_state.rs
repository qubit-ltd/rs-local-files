/// Write access state after a possibly partial temporary-file sealing attempt.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalTempAccessState {
    /// The retained owner still permits writes.
    Writable,
    /// The retained owner is restricted to read access.
    ReadOnly,
    /// The native access state could not be established.
    Indeterminate,
}
