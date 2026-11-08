// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Read;
use std::io::Seek;
use std::io::Write;

use qubit_io::ByteInput;
use qubit_io::ByteOutput;
use qubit_io::Input;
use qubit_io::Output;
use qubit_io::StdRead;
use qubit_io::StdWrite;
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

assert_not_impl!(LocalFileReader, Read);
assert_not_impl!(LocalFileReader, Write);
assert_not_impl!(LocalFileReader, Output);
assert_not_impl!(LocalFileWriter, Write);
assert_not_impl!(LocalFileWriter, Read);
assert_not_impl!(LocalFileWriter, Input);
assert_not_impl!(LocalTempFile, Write);
assert_not_impl!(LocalTempFile, Read);
assert_not_impl!(LocalTempFile, Input);

#[test]
fn test_native_handles_implement_byte_traits() {
    fn input<T: ByteInput>() {}
    fn output<T: ByteOutput>() {}

    input::<LocalFileReader>();
    output::<LocalFileWriter>();
    output::<LocalTempFile>();
}

#[test]
fn test_standard_io_adapters_provide_read_write_and_seek() {
    fn reader<T: Read + Seek>() {}
    fn writer<T: Write>() {}
    fn seek_writer<T: Write + Seek>() {}

    reader::<StdRead<LocalFileReader>>();
    writer::<StdWrite<LocalFileWriter>>();
    seek_writer::<StdWrite<LocalTempFile>>();
}
