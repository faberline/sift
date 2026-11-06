//! Count content references through the spill port.

use crate::archive::domain::control_values::SpillBlobReference;
use crate::ContentBlobRef;
use anyhow::{bail, Context, Result};

pub(in crate::archive) fn add_spill_blob_reference(
    target: &mut crate::archive::application::spill_service::SpillService,
    reference: &ContentBlobRef,
) -> Result<()> {
    let key = format!("blob/{}", reference.hash);
    let mut counted = target
        .lookup(&key)?
        .map(|bytes| {
            serde_json::from_slice::<SpillBlobReference>(&bytes)
                .context("decode archive spill blob reference")
        })
        .transpose()?
        .unwrap_or_else(|| SpillBlobReference {
            reference: reference.clone(),
            count: 0,
        });
    if counted.reference != *reference {
        bail!("one blob hash has conflicting content references");
    }
    counted.count = counted.count.saturating_add(1);
    target.upsert(key, serde_json::to_vec(&counted)?)
}
