use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalResult;
use qubit_local_files::directory::{
    LocalResolutionOrigin, LocalResolutionPolicy, LocalResolutionScope, LocalTreeValidationOptions,
};
use qubit_local_files::error::{
    LocalFileError, LocalFileErrorKind, LocalFileErrorSource, LocalFileOperation, LocalResourceKind,
};
use qubit_local_files::observation::{
    LocalObservationRequirement, LocalSecurityObservationOptions, LocalSourceVerification,
};
use qubit_local_files::options::{
    LocalDirectoryOpenOptions, LocalListOptions, LocalObservedReadOptions, LocalPathBindingOptions,
    LocalResolutionLimits, LocalRootOpenOptions, LocalTempDirectoryOptions,
    LocalTreePublishOptions,
};
use qubit_local_files::policy::LocalOwnerAccessPolicy;
use qubit_local_files::policy::LocalPrivacyRequirement;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug)]
struct RejectBlockedTarget;

impl LocalResolutionPolicy for RejectBlockedTarget {
    fn validate_component(
        &self,
        _: &OsStr,
        _: LocalResolutionScope,
        _: LocalResolutionOrigin,
    ) -> LocalResult<()> {
        Ok(())
    }

    fn validate_symlink_target(&self, target: &Path, _: LocalResolutionScope) -> LocalResult<()> {
        if target == Path::new("blocked") {
            return Err(LocalFileError::new(
                LocalFileErrorKind::PermissionDenied,
                LocalFileOperation::OpenRoot,
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct RejectBlockedComponent;

impl LocalResolutionPolicy for RejectBlockedComponent {
    fn validate_component(
        &self,
        component: &OsStr,
        _: LocalResolutionScope,
        _: LocalResolutionOrigin,
    ) -> LocalResult<()> {
        if component == OsStr::new("blocked") {
            return Err(LocalFileError::new(
                LocalFileErrorKind::PermissionDenied,
                LocalFileOperation::OpenRoot,
            ));
        }
        Ok(())
    }

    fn validate_symlink_target(&self, _: &Path, _: LocalResolutionScope) -> LocalResult<()> {
        Ok(())
    }
}

#[cfg(unix)]
#[test]
fn root_policy_survives_child_symlink_reader_and_final_binding() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("fixture directory");
    std::fs::create_dir(tmp.path().join("safe")).expect("safe directory");
    std::fs::create_dir(tmp.path().join("blocked")).expect("blocked directory");
    std::fs::write(tmp.path().join("safe/source"), b"original").expect("source");
    std::fs::write(tmp.path().join("blocked/source"), b"replacement").expect("replacement");
    symlink("blocked", tmp.path().join("safe/alias")).expect("link to blocked target");

    let root_policy = Arc::new(RejectBlockedTarget);
    let fs = LocalFileSystem::rooted_with_options(
        tmp.path(),
        &LocalRootOpenOptions::new().with_resolution_policy(root_policy),
    )
    .expect("root with policy");
    let child = fs
        .open_directory_with_options(
            Path::new("safe"),
            &LocalDirectoryOpenOptions::new()
                .with_resolution_policy(Arc::new(RejectBlockedComponent)),
        )
        .expect("child may tighten the root policy");

    let child_error = child
        .open_observed_reader(
            Path::new("blocked/source"),
            &LocalObservedReadOptions::new(),
        )
        .expect_err("child component policy tightens the inherited root policy");
    assert_eq!(LocalFileErrorKind::PermissionDenied, child_error.kind());
    assert!(child_error.typed_source().is_none());

    let error = child
        .open_observed_reader(Path::new("alias/source"), &LocalObservedReadOptions::new())
        .expect_err("child reader cannot bypass root symlink target policy");
    assert_eq!(error.kind(), LocalFileErrorKind::PermissionDenied);
    assert_eq!(error.operation(), LocalFileOperation::BindPath);
    assert!(error.typed_source().is_none());
    assert_eq!(
        b"original".as_slice(),
        std::fs::read(tmp.path().join("safe/source"))
            .unwrap()
            .as_slice()
    );

    let reader = child
        .open_observed_reader(
            Path::new("source"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("bound reader opens");
    let before = reader
        .security_observation(&LocalSecurityObservationOptions::new())
        .expect("observe opened object");
    std::fs::rename(
        tmp.path().join("safe/source"),
        tmp.path().join("safe/saved"),
    )
    .expect("move retained object");
    std::fs::write(tmp.path().join("safe/source"), b"new name target").expect("replace name");
    let after = reader
        .security_observation(&LocalSecurityObservationOptions::new())
        .expect("security observation remains on opened object");
    assert_eq!(before.identity(), after.identity());
    assert_eq!(
        LocalSourceVerification::Changed,
        reader
            .verify_path_binding()
            .expect("binding checks the original name")
    );
}

#[cfg(unix)]
#[test]
fn custom_stage_observation_tightening_reopen_and_publish_preserve_owner_policy() {
    let tmp = tempfile::tempdir().expect("fixture directory");
    let fs = LocalFileSystem::host().expect("host filesystem");
    let parent = fs
        .open_directory_with_options(tmp.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let initial = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let tightened = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let mut stage = parent
        .create_staged_directory(
            &LocalTempDirectoryOptions::new().with_owner_access_policy(initial),
        )
        .expect("custom stage");
    stage
        .write_file_new(Path::new("middle/payload"), &mut &b"payload"[..], 64)
        .expect("create intermediate tree and file");

    let observed_root = stage
        .directory()
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required),
        )
        .expect("L03 observes stage root");
    assert_unix_owner_mode(&observed_root, 0o700);
    let observed_middle = stage
        .directory()
        .open_directory(Path::new("middle"), &LocalDirectoryOpenOptions::new())
        .expect("open intermediate directory")
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required),
        )
        .expect("L03 same-object observation");
    assert_unix_owner_mode(&observed_middle, 0o700);
    let observed_payload = stage
        .directory()
        .open_observed_reader(
            Path::new("middle/payload"),
            &LocalObservedReadOptions::new(),
        )
        .expect("open stage payload")
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required),
        )
        .expect("L03 observes stage payload");
    assert_unix_owner_mode(&observed_payload, 0o600);

    stage
        .apply_owner_access_policy_with_options(&tightened, &LocalListOptions::new())
        .expect("tighten policy across staged tree");
    let stage_name = stage.relative_name().to_os_string();
    let identity = stage.identity().expect("stage identity").clone();
    drop(stage.retain_in_place());
    let reopened = parent
        .open_staged_directory(
            Path::new(&stage_name),
            &identity,
            &LocalTreeValidationOptions::new()
                .with_owner_access_policy(tightened)
                .with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("reopen stage under recorded policy");
    assert_eq!(Some(&tightened), reopened.owner_access_policy());
    let reopened_root = reopened
        .directory()
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required),
        )
        .expect("L03 observes reopened stage root");
    assert_unix_owner_mode(&reopened_root, 0o700);
    let reopened_middle = reopened
        .directory()
        .open_directory(Path::new("middle"), &LocalDirectoryOpenOptions::new())
        .expect("open reopened middle")
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required),
        )
        .expect("L03 observes reopened intermediate directory");
    assert_unix_owner_mode(&reopened_middle, 0o700);
    let reopened_payload = reopened
        .directory()
        .open_observed_reader(
            Path::new("middle/payload"),
            &LocalObservedReadOptions::new(),
        )
        .expect("open reopened payload")
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required),
        )
        .expect("L03 observes reopened payload");
    assert_unix_owner_mode(&reopened_payload, 0o400);

    let outcome = reopened
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new().with_owner_access_policy(tightened),
        )
        .expect("publish tightened tree");
    assert_eq!(Some(&tightened), outcome.owner_access_policy());
    let published = parent
        .open_directory(Path::new("published"), &LocalDirectoryOpenOptions::new())
        .expect("open published tree for observation");
    let published_root = published
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required),
        )
        .expect("L03 observes published root");
    assert_unix_owner_mode(&published_root, 0o700);
    let published_middle = published
        .open_directory(Path::new("middle"), &LocalDirectoryOpenOptions::new())
        .expect("open published middle")
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required),
        )
        .expect("L03 observes published intermediate directory");
    assert_unix_owner_mode(&published_middle, 0o700);
    let published_payload = published
        .open_observed_reader(
            Path::new("middle/payload"),
            &LocalObservedReadOptions::new(),
        )
        .expect("open published payload")
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required),
        )
        .expect("L03 observes published payload");
    assert_unix_owner_mode(&published_payload, 0o400);
}

#[cfg(unix)]
fn assert_unix_owner_mode(
    observation: &qubit_local_files::observation::LocalSecurityObservation,
    mode: u32,
) {
    let access = observation
        .access()
        .and_then(|access| access.unix())
        .expect("required Unix access facts");
    assert_eq!(mode, access.mode() & 0o777);
    assert!(access.owner_is_current_user());
}

#[cfg(unix)]
#[test]
fn resolution_quota_failure_keeps_structured_measurement_and_no_stage_owner_alias() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("fixture directory");
    std::fs::create_dir(tmp.path().join("target")).expect("target directory");
    symlink("target", tmp.path().join("alias")).expect("root alias");
    let error = LocalFileSystem::rooted_with_options(
        &tmp.path().join("alias"),
        &LocalRootOpenOptions::new()
            .with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(0)),
    )
    .expect_err("zero-hop resolution quota rejects a real symlink");
    assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
    let facts = error
        .resource_limit_error()
        .expect("resolution quota reports structured facts");
    assert_eq!(LocalResourceKind::SymlinkHops, facts.resource());
    assert_eq!(0, facts.limit());
    assert_eq!(Some(1), facts.observed());
    assert!(!facts.overflowed());
    assert!(error.temp_recovery().is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn acl_budget_failure_reports_acl_dimension_and_stage_remains_the_only_cleanup_owner() {
    use qubit_local_files::test_support::install_test_fault;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    let tmp = tempfile::tempdir().expect("fixture directory");
    let parent = LocalFileSystem::host()
        .expect("host filesystem")
        .open_directory_with_options(tmp.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage owner");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("stage payload");
    let payload = tmp.path().join(stage.relative_name()).join("payload");
    let native_path = CString::new(payload.as_os_str().as_bytes()).expect("path has no NUL");
    let acl_name = c"system.posix_acl_access";
    // Linux ACL v2 with a named user so the kernel retains an extended ACL.
    let acl: [u8; 44] = [
        2, 0, 0, 0, // version
        1, 0, 7, 0, 255, 255, 255, 255, // USER_OBJ
        2, 0, 4, 0, 0x40, 0xe2, 1, 0, // USER (uid 123456)
        4, 0, 5, 0, 255, 255, 255, 255, // GROUP_OBJ
        16, 0, 5, 0, 255, 255, 255, 255, // MASK
        32, 0, 0, 0, 255, 255, 255, 255, // OTHER
    ];
    // SAFETY: both pointers remain valid for the duration of setxattr and the
    // byte slice contains a complete Linux POSIX ACL v2 value.
    let set_result = unsafe {
        libc::setxattr(
            native_path.as_ptr(),
            acl_name.as_ptr(),
            acl.as_ptr().cast(),
            acl.len(),
            0,
        )
    };
    if set_result == -1 {
        let error = std::io::Error::last_os_error();
        panic!("install extended ACL fixture; runner must support POSIX ACLs: {error}");
    }
    let mode_after_acl_install = std::fs::metadata(&payload)
        .expect("ACL-bearing payload metadata")
        .mode()
        & 0o777;

    let reader = stage
        .directory()
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("open ACL-bearing object");
    let error = reader
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required)
                .with_max_acl_bytes(1),
        )
        .expect_err("one-byte ACL budget rejects the extended ACL before reading it");
    assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
    let facts = error
        .resource_limit_error()
        .expect("ACL quota facts are retained through the reader error");
    assert_eq!(LocalResourceKind::AclBytes, facts.resource());
    assert_eq!(1, facts.limit());
    assert_eq!(Some(acl.len()), facts.observed());
    assert!(!facts.overflowed());
    assert_eq!(
        mode_after_acl_install,
        std::fs::metadata(&payload)
            .expect("payload metadata")
            .mode()
            & 0o777
    );

    // Inject a later owner-policy ACL application failure. The stage remains
    // the sole cleanup owner; the error carries no duplicate retained owner.
    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
    let fault = install_test_fault("owner-policy-apply").expect("ACL apply fault installs");
    let access_error = stage
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new())
        .expect_err("injected ACL application failure preserves recovery state");
    drop(fault);
    assert_eq!(
        LocalFileErrorKind::PermissionDenied,
        access_error.cause().kind()
    );
    assert!(matches!(
        access_error.cause().typed_source(),
        Some(LocalFileErrorSource::Io(source))
            if source.kind() == std::io::ErrorKind::PermissionDenied
    ));
    assert!(access_error.retained_resources().is_empty());
    assert!(
        payload.exists(),
        "stage owner keeps the failed tree available"
    );
    assert_eq!(Some(&policy), stage.owner_access_policy());
    let stage_path = tmp.path().join(stage.relative_name());
    stage
        .cleanup()
        .expect("the sole stage owner cleans the retained tree");
    assert!(!stage_path.exists());
}

#[cfg(windows)]
#[test]
fn rooted_content_remains_unsupported_without_mutation_while_l01_is_blocked() {
    use qubit_local_files::options::{LocalWriteMode, LocalWriteOptions, LocalWritePrecondition};
    use sha2::{Digest, Sha256};

    let tmp = tempfile::tempdir().expect("fixture directory");
    std::fs::write(tmp.path().join("target"), b"old").expect("old target");
    let fs = LocalFileSystem::rooted(tmp.path()).expect("rooted authority");
    let mut entries_before = std::fs::read_dir(tmp.path())
        .expect("list namespace before conditional open")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<Vec<_>>();
    entries_before.sort_unstable();
    let before = std::fs::read(tmp.path().join("target")).expect("read old target");
    let result = fs.open_conditional_writer::<Sha256>(
        Path::new("target"),
        LocalWritePrecondition::Content(Sha256::digest(b"old")),
        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
    );
    let error = result.expect_err("Rooted Content remains unsupported while L01 is blocked");
    assert_eq!(LocalFileErrorKind::Unsupported, error.kind());
    assert!(error.typed_source().is_none());
    assert_eq!(
        before,
        std::fs::read(tmp.path().join("target")).expect("target unchanged")
    );
    let mut entries_after = std::fs::read_dir(tmp.path())
        .expect("list namespace after unsupported conditional open")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<Vec<_>>();
    entries_after.sort_unstable();
    assert_eq!(
        entries_before, entries_after,
        "Unsupported creates no namespace entries"
    );
}
