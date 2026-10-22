// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalFileReader;
use qubit_local_files::LocalFileWriter;
use qubit_local_files::LocalTempFile;

macro_rules! assert_not_impl {
    ($ty:ty, $bound:path) => {
        const _: fn() = || {
            trait AmbiguousIfImpl<A> {
                fn check() {}
            }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            struct Invalid;
            impl<T: ?Sized + $bound> AmbiguousIfImpl<Invalid> for T {}
            let _ = <$ty as AmbiguousIfImpl<_>>::check;
        };
    };
}

assert_not_impl!(LocalFileReader, std::io::Read);
assert_not_impl!(LocalFileReader, std::io::Write);
assert_not_impl!(LocalFileReader, qubit_io::Output);
assert_not_impl!(LocalFileWriter, std::io::Write);
assert_not_impl!(LocalFileWriter, std::io::Read);
assert_not_impl!(LocalFileWriter, qubit_io::Input);
assert_not_impl!(LocalTempFile, std::io::Write);
assert_not_impl!(LocalTempFile, std::io::Read);
assert_not_impl!(LocalTempFile, qubit_io::Input);

#[test]
fn adapters_supply_standard_traits_and_seek() {
    fn reader<T: std::io::Read + std::io::Seek>() {}
    fn writer<T: std::io::Write>() {}
    fn seek_writer<T: std::io::Write + std::io::Seek>() {}

    reader::<qubit_io::StdRead<LocalFileReader>>();
    writer::<qubit_io::StdWrite<LocalFileWriter>>();
    seek_writer::<qubit_io::StdWrite<LocalTempFile>>();
}
