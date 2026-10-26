use std::error::Error as StdError;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io;

/// Records that recursive destination removal was invoked before failing.
#[derive(Debug)]
pub(super) struct DestinationRemovalFailure(io::Error);

impl Display for DestinationRemovalFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

impl StdError for DestinationRemovalFailure {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.0)
    }
}

pub(super) fn mark_attempted(error: io::Error) -> io::Error {
    let kind = error.kind();
    io::Error::new(kind, DestinationRemovalFailure(error))
}

pub(super) fn was_attempted(error: &io::Error) -> bool {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<DestinationRemovalFailure>())
        .is_some()
}
