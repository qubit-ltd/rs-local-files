#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
/// Classification of the filesystem containing an observed object.
pub enum LocalFileSystemLocality {
    /// The filesystem is classified as local.
    Local,
    /// The filesystem is classified as remote.
    Remote,
    /// The filesystem could not be classified; this is still a successful observation.
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
/// Unix platform family used to interpret filesystem observations.
pub enum LocalUnixPlatform {
    /// Linux.
    Linux,
    /// Darwin (macOS).
    Darwin,
    /// Another Unix platform.
    Other,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
/// Presence or support status of an ACL mechanism.
pub enum LocalAclPresence {
    /// The ACL is known to be absent.
    Absent,
    /// An ACL is present.
    Present,
    /// The platform or filesystem explicitly reports no support for the queried ACL kind.
    NotSupported,
}
#[derive(Clone, Debug, Eq, PartialEq)]
/// Security facts observed from one retained filesystem object.
///
/// The fields may represent observations taken at different instants; this value
/// is not an atomic snapshot and does not guarantee future permissions.
pub struct LocalSecurityObservation {
    identity: Option<super::LocalFileIdentity>,
    filesystem: Option<LocalFileSystemObservation>,
    access: Option<LocalAccessObservation>,
    link_count: Option<u64>,
}
impl LocalSecurityObservation {
    pub(crate) const fn new(
        identity: Option<super::LocalFileIdentity>,
        filesystem: Option<LocalFileSystemObservation>,
        access: Option<LocalAccessObservation>,
        link_count: Option<u64>,
    ) -> Self {
        Self {
            identity,
            filesystem,
            access,
            link_count,
        }
    }
    /// Returns the object's identity when the platform supplied it.
    pub const fn identity(&self) -> Option<&super::LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Returns filesystem facts when requested and available.
    pub const fn filesystem(&self) -> Option<&LocalFileSystemObservation> {
        self.filesystem.as_ref()
    }
    /// Returns access-control facts when requested and available.
    pub const fn access(&self) -> Option<&LocalAccessObservation> {
        self.access.as_ref()
    }
    /// Returns the native link count when requested and available.
    pub const fn link_count(&self) -> Option<u64> {
        self.link_count
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
/// Filesystem classification and platform-specific filesystem facts.
pub struct LocalFileSystemObservation {
    locality: LocalFileSystemLocality,
    unix: Option<LocalUnixFileSystemObservation>,
    windows: Option<LocalWindowsFileSystemObservation>,
}
impl LocalFileSystemObservation {
    pub(crate) const fn new(
        locality: LocalFileSystemLocality,
        unix: Option<LocalUnixFileSystemObservation>,
        windows: Option<LocalWindowsFileSystemObservation>,
    ) -> Self {
        Self {
            locality,
            unix,
            windows,
        }
    }
    /// Returns the classification; `Unknown` is a successful but inconclusive result.
    pub const fn locality(&self) -> LocalFileSystemLocality {
        self.locality
    }
    /// Returns Unix filesystem details when observed on Unix.
    pub const fn unix(&self) -> Option<&LocalUnixFileSystemObservation> {
        self.unix.as_ref()
    }
    /// Returns Windows filesystem details when observed on Windows.
    pub const fn windows(&self) -> Option<&LocalWindowsFileSystemObservation> {
        self.windows.as_ref()
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
/// Unix filesystem type identifier and optional platform name.
pub struct LocalUnixFileSystemObservation {
    platform: LocalUnixPlatform,
    type_id: u64,
    type_name: Option<String>,
}
impl LocalUnixFileSystemObservation {
    pub(crate) fn new(
        platform: LocalUnixPlatform,
        type_id: u64,
        type_name: Option<String>,
    ) -> Self {
        Self {
            platform,
            type_id,
            type_name,
        }
    }
    /// Returns the Unix platform family.
    pub const fn platform(&self) -> LocalUnixPlatform {
        self.platform
    }
    /// Returns the native filesystem type identifier zero-extended to `u64`.
    pub const fn type_id(&self) -> u64 {
        self.type_id
    }
    /// Returns a platform-provided filesystem type name, if available.
    pub fn type_name(&self) -> Option<&str> {
        self.type_name.as_deref()
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
/// Windows filesystem name and volume attribute flags.
pub struct LocalWindowsFileSystemObservation {
    filesystem_name: String,
    volume_attributes: u32,
}
impl LocalWindowsFileSystemObservation {
    pub(crate) fn new(filesystem_name: String, volume_attributes: u32) -> Self {
        Self {
            filesystem_name,
            volume_attributes,
        }
    }
    /// Returns the filesystem name reported by Windows.
    pub fn filesystem_name(&self) -> &str {
        &self.filesystem_name
    }
    /// Returns the native volume attribute bitmask.
    pub const fn volume_attributes(&self) -> u32 {
        self.volume_attributes
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
/// Platform-specific access-control facts.
pub struct LocalAccessObservation {
    unix: Option<LocalUnixAccessObservation>,
    windows: Option<LocalWindowsAccessObservation>,
}
impl LocalAccessObservation {
    pub(crate) const fn new(
        unix: Option<LocalUnixAccessObservation>,
        windows: Option<LocalWindowsAccessObservation>,
    ) -> Self {
        Self { unix, windows }
    }
    /// Returns Unix access facts when observed on Unix.
    pub const fn unix(&self) -> Option<&LocalUnixAccessObservation> {
        self.unix.as_ref()
    }
    /// Returns Windows access facts when observed on Windows.
    pub const fn windows(&self) -> Option<&LocalWindowsAccessObservation> {
        self.windows.as_ref()
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
/// Unix mode, owner, and ACL presence facts.
pub struct LocalUnixAccessObservation {
    mode: u32,
    owner_is_current_user: bool,
    access_acl: LocalAclPresence,
    default_acl: LocalAclPresence,
}
impl LocalUnixAccessObservation {
    pub(crate) const fn new(
        mode: u32,
        owner_is_current_user: bool,
        access_acl: LocalAclPresence,
        default_acl: LocalAclPresence,
    ) -> Self {
        Self {
            mode,
            owner_is_current_user,
            access_acl,
            default_acl,
        }
    }
    /// Returns permission and special mode bits (`st_mode & 0o7777`).
    pub const fn mode(&self) -> u32 {
        self.mode
    }
    /// Returns whether the owner matches the current effective user.
    pub const fn owner_is_current_user(&self) -> bool {
        self.owner_is_current_user
    }
    /// Returns the access ACL status.
    pub const fn access_acl(&self) -> LocalAclPresence {
        self.access_acl
    }
    /// Returns the default ACL status.
    pub const fn default_acl(&self) -> LocalAclPresence {
        self.default_acl
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
/// Windows owner, DACL state, and ordered ACE observations.
pub struct LocalWindowsAccessObservation {
    owner_sid: String,
    owner_is_current_user: bool,
    dacl_present: bool,
    dacl_is_null: bool,
    dacl_protected: bool,
    acl_valid: bool,
    aces: Vec<LocalWindowsAceObservation>,
}
impl LocalWindowsAccessObservation {
    pub(crate) fn new(
        owner_sid: String,
        owner_is_current_user: bool,
        dacl_present: bool,
        dacl_is_null: bool,
        dacl_protected: bool,
        acl_valid: bool,
        aces: Vec<LocalWindowsAceObservation>,
    ) -> Self {
        Self {
            owner_sid,
            owner_is_current_user,
            dacl_present,
            dacl_is_null,
            dacl_protected,
            acl_valid,
            aces,
        }
    }
    /// Returns the owner SID in string form.
    pub fn owner_sid(&self) -> &str {
        &self.owner_sid
    }
    /// Returns whether the owner SID matches the current security principal.
    pub const fn owner_is_current_user(&self) -> bool {
        self.owner_is_current_user
    }
    /// Returns whether the security descriptor has a DACL.
    pub const fn dacl_present(&self) -> bool {
        self.dacl_present
    }
    /// Returns whether the descriptor contains a NULL DACL.
    pub const fn dacl_is_null(&self) -> bool {
        self.dacl_is_null
    }
    /// Returns whether DACL inheritance is protected.
    pub const fn dacl_protected(&self) -> bool {
        self.dacl_protected
    }
    /// Returns whether the DACL structure passed validation.
    pub const fn acl_valid(&self) -> bool {
        self.acl_valid
    }
    /// Returns ACEs in native descriptor order.
    pub fn aces(&self) -> &[LocalWindowsAceObservation] {
        &self.aces
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
/// One Windows access-control entry, retaining undecoded native facts.
pub struct LocalWindowsAceObservation {
    ace_type: u8,
    flags: u8,
    native_size: u16,
    access_mask: Option<u32>,
    trustee_sid: Option<String>,
    trustee_is_current_user: Option<bool>,
    is_decoded: bool,
}
impl LocalWindowsAceObservation {
    pub(crate) const fn new(
        ace_type: u8,
        flags: u8,
        native_size: u16,
        access_mask: Option<u32>,
        trustee_sid: Option<String>,
        trustee_is_current_user: Option<bool>,
        is_decoded: bool,
    ) -> Self {
        Self {
            ace_type,
            flags,
            native_size,
            access_mask,
            trustee_sid,
            trustee_is_current_user,
            is_decoded,
        }
    }
    /// Returns the native ACE type byte.
    pub const fn ace_type(&self) -> u8 {
        self.ace_type
    }
    /// Returns the native ACE flags byte.
    pub const fn flags(&self) -> u8 {
        self.flags
    }
    /// Returns the native ACE size in bytes.
    pub const fn native_size(&self) -> u16 {
        self.native_size
    }
    /// Returns a decoded access mask when the ACE type exposes one.
    pub const fn access_mask(&self) -> Option<u32> {
        self.access_mask
    }
    /// Returns the trustee SID when it was decoded.
    pub fn trustee_sid(&self) -> Option<&str> {
        self.trustee_sid.as_deref()
    }
    /// Returns whether the trustee is the current principal when known.
    pub const fn trustee_is_current_user(&self) -> Option<bool> {
        self.trustee_is_current_user
    }
    /// Returns whether the ACE type and trustee were fully decoded.
    pub const fn is_decoded(&self) -> bool {
        self.is_decoded
    }
}
