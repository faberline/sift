//! Use the existing archive I/O operations and error chains.

use crate::archive::domain::file_modes_port::FileModesPort;
use anyhow::Result;
use std::path::Path;

pub(crate) struct NativeFileModesPort;

impl FileModesPort for NativeFileModesPort {
    type Error = anyhow::Error;
    fn set_private_file(&self, path: &Path) -> Result<()> {
        crate::archive::infrastructure::private_fs_mode::set_private_file(path)
    }
    fn set_private_dir(&self, path: &Path) -> Result<()> {
        crate::archive::infrastructure::private_fs_mode::set_private_dir(path)
    }
}
