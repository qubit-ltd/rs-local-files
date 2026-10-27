/// Requested privacy when creating a temporary resource.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalPrivacyRequirement {
    /// Use the platform's existing temporary-resource defaults.
    #[default]
    PlatformDefault,
    /// Create the resource with access restricted to the current user.
    Required,
}
