use std::ffi::OsString;
use std::fs::File;
use std::io;

use crate::observation::LocalFileIdentity;

/// Native private resource retained after a creation step partially succeeds.
#[derive(Debug)]
pub(crate) struct NativePrivateEntry {
    pub(crate) parent: File,
    pub(crate) name: OsString,
    pub(crate) file: Option<File>,
    pub(crate) identity: Option<LocalFileIdentity>,
}

/// Native private creation failure with ownership of any partial resource.
#[derive(Debug)]
pub(crate) struct NativePrivateCreateError {
    pub(crate) source: io::Error,
    pub(crate) retained: Option<NativePrivateEntry>,
}
