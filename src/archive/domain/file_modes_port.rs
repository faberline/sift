//! FileModesPort isolates archive I/O.

use std::path::Path;

pub(crate) trait FileModesPort: Send + Sync {
    type Error;
    fn set_private_file(&self, path: &Path) -> Result<(), Self::Error>;
    fn set_private_dir(&self, path: &Path) -> Result<(), Self::Error>;
}
