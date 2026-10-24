// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use digest::Digest;
use digest::Output;

use super::ConditionalBackend;
use super::ConditionalEntry;
use super::digest_reader::digest_entry;
use crate::recovery::LocalRecoveryFileRole;

/// Creates and independently verifies a recovery copy before publication.
///
/// The entry is first marked `StagedCandidate`, because creation alone does
/// not prove it contains the complete candidate. A read error or digest
/// mismatch returns that entry with the error so its owner can retain it for
/// recovery. Only a matching digest changes its role to `CandidateCopy` and
/// permits the publication protocol to proceed with it.
pub(super) fn prepare_copy<D, B>(
    backend: &mut B,
    candidate_digest: &Output<D>,
) -> Result<ConditionalEntry, (std::io::Error, Option<ConditionalEntry>)>
where
    D: Digest,
    B: ConditionalBackend,
{
    let mut copy = backend.create_candidate_copy().map_err(|error| (error, None))?;
    copy.set_role(LocalRecoveryFileRole::StagedCandidate);
    if let Err(error) = backend.checkpoint(super::ConditionalPoint::CandidateCopyCreated) {
        return Err((error, Some(copy)));
    }
    let copied_digest = match digest_entry::<D>(&copy) {
        Ok(digest) => digest,
        Err(error) => return Err((error, Some(copy))),
    };
    if copied_digest != *candidate_digest {
        return Err((
            std::io::Error::new(std::io::ErrorKind::InvalidData, "candidate copy digest mismatch"),
            Some(copy),
        ));
    }
    copy.set_role(LocalRecoveryFileRole::CandidateCopy);
    Ok(copy)
}
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
