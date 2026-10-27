use crate::options::{LocalSealOptions, LocalWriteMode, LocalWriteOptions};
use crate::policy::LocalPrivacyRequirement;
use crate::policy::{LocalDurabilityRequirement, LocalFileSyncRequirement};

#[test]
fn lf3_contract_defaults_preserve_old_guarantees() {
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew);
    assert_eq!(options.file_sync(), LocalFileSyncRequirement::NotRequired);
    assert_eq!(
        options.durability(),
        LocalDurabilityRequirement::NotRequired
    );
    assert_eq!(
        LocalSealOptions::new().file_sync(),
        LocalFileSyncRequirement::Required
    );
}

#[test]
fn lf3_contract_byte_traits_accept_unsized_values() {
    fn accepts_input<R: qubit_io::ByteInput + ?Sized>(_reader: &mut R) {}
    fn accepts_output<W: qubit_io::ByteOutput + ?Sized>(_writer: &mut W) {}

    let input: &mut dyn qubit_io::ByteInput = &mut std::io::Cursor::new(Vec::<u8>::new());
    let output: &mut dyn qubit_io::ByteOutput = &mut Vec::<u8>::new();
    accepts_input(input);
    accepts_output(output);
}

#[test]
fn lf3_contract_privacy_defaults_and_copy_configuration() {
    let file_options = crate::options::LocalTempFileOptions::new();
    let directory_options = crate::options::LocalTempDirectoryOptions::new();
    assert_eq!(
        file_options.privacy(),
        LocalPrivacyRequirement::PlatformDefault
    );
    assert_eq!(
        directory_options.privacy(),
        LocalPrivacyRequirement::PlatformDefault
    );
    assert_eq!(
        file_options
            .clone()
            .with_privacy(LocalPrivacyRequirement::Required)
            .privacy(),
        LocalPrivacyRequirement::Required
    );
    assert_eq!(
        file_options.privacy(),
        LocalPrivacyRequirement::PlatformDefault
    );
}

#[test]
fn lf3_contract_read_options_keep_explicit_budgets_and_snapshot_limit() {
    use crate::options::{
        LocalObservedReadOptions, LocalPathBindingOptions, LocalReadAllOptions, LocalReadOptions,
        LocalSnapshotOptions,
    };
    use crate::policy::LocalSymlinkPolicy;

    assert_eq!(LocalReadAllOptions::new().max_bytes(), None);
    assert_eq!(LocalReadAllOptions::new().read_options(), None);
    assert_eq!(
        LocalReadOptions::new()
            .with_symlink_policy(LocalSymlinkPolicy::Reject)
            .symlink_policy(),
        Some(LocalSymlinkPolicy::Reject)
    );
    let all = LocalReadAllOptions::new()
        .with_max_bytes(8)
        .with_read_options(LocalReadOptions::new())
        .with_symlink_policy(LocalSymlinkPolicy::Reject);
    assert_eq!(all.max_bytes(), Some(8));
    assert_eq!(all.symlink_policy(), Some(LocalSymlinkPolicy::Reject));
    assert_eq!(
        LocalObservedReadOptions::new().native_observation(),
        crate::observation::LocalObservationRequirement::Optional
    );
    assert_eq!(LocalPathBindingOptions::new().max_ancestors(), None);
    let snapshot = LocalSnapshotOptions::builder(0)
        .read_options(LocalReadOptions::new())
        .build();
    assert_eq!(snapshot.max_bytes(), 0);
}

#[test]
fn lf3_contract_invalid_encoding_retains_utf8_source() {
    use crate::error::{LocalFileError, LocalFileErrorKind, LocalFileOperation};

    let invalid = String::from_utf8(vec![0xff]).expect_err("test byte is invalid UTF-8");
    let error = LocalFileError::from_utf8(LocalFileOperation::Read, None, invalid);
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidEncoding);
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::InvalidData);
    assert!(matches!(
        error.typed_source(),
        Some(crate::error::LocalFileErrorSource::InvalidEncoding(_))
    ));
}
