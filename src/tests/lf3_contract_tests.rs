use crate::options::{LocalSealOptions, LocalWriteMode, LocalWriteOptions};
use crate::policy::LocalPrivacyRequirement;
use crate::policy::{LocalDurabilityRequirement, LocalFileSyncRequirement};

#[test]
fn lf3_contract_defaults_preserve_old_guarantees() {
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew);
    assert_eq!(options.file_sync(), LocalFileSyncRequirement::NotRequired);
    assert_eq!(options.durability(), LocalDurabilityRequirement::NotRequired);
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
    assert_eq!(file_options.privacy(), LocalPrivacyRequirement::PlatformDefault);
    assert_eq!(directory_options.privacy(), LocalPrivacyRequirement::PlatformDefault);
    assert_eq!(
        file_options.clone().with_privacy(LocalPrivacyRequirement::Required).privacy(),
        LocalPrivacyRequirement::Required
    );
    assert_eq!(file_options.privacy(), LocalPrivacyRequirement::PlatformDefault);
}
