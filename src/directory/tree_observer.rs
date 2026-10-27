use super::LocalDirectoryHandle;
use super::tree_validation::LocalTreeEntryObservation;
use crate::LocalDirectoryWalker;
use crate::LocalFileOperation;
use crate::LocalResult;

#[cfg(test)]
thread_local! {
    static TREE_ENTRY_OBSERVATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_test_observation_count() {
    TREE_ENTRY_OBSERVATIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn test_observation_count() -> usize {
    TREE_ENTRY_OBSERVATIONS.with(std::cell::Cell::get)
}

/// Lazy iterator over entries observed beneath a staged directory.
#[derive(Debug)]
pub struct LocalTreeObserver {
    directory: LocalDirectoryHandle,
    walker: LocalDirectoryWalker,
}

impl LocalTreeObserver {
    pub(crate) fn new(directory: LocalDirectoryHandle, walker: LocalDirectoryWalker) -> Self {
        Self { directory, walker }
    }
}

impl Iterator for LocalTreeObserver {
    type Item = LocalResult<LocalTreeEntryObservation>;

    fn next(&mut self) -> Option<Self::Item> {
        let entry = match self.walker.next()? {
            Ok(entry) => entry,
            Err(error) => return Some(Err(error)),
        };
        let relative = entry.relative_path().to_path_buf();
        #[cfg(test)]
        TREE_ENTRY_OBSERVATIONS.with(|count| count.set(count.get() + 1));
        Some(
            self.directory
                .observe_entry(&relative)
                .map(|observation| LocalTreeEntryObservation::new(relative, observation))
                .map_err(|error| error.with_operation(LocalFileOperation::Metadata)),
        )
    }
}
