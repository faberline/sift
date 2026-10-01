//! RestoreStorePort isolates archive I/O.

use crate::archive::domain::control_values::RestoreState;
use std::path::Path;

pub(crate) trait RestoreStorePort: Send + Sync {
    type Error;
    fn read_restore_state(&self, path: &Path) -> Result<Option<RestoreState>, Self::Error>;
    fn persist_restore_state(&self, path: &Path, state: &RestoreState) -> Result<(), Self::Error>;
    fn validate_building_restore_target(&self, target: &Path) -> Result<(), Self::Error>;
    fn publish_restore_stage(&self, target: &Path) -> Result<(), Self::Error>;
    fn remove_restore_stage(&self, stage: &Path) -> Result<(), Self::Error>;
    fn require_empty_restore_target(&self, target: &Path) -> Result<(), Self::Error>;
}
