//! A non-durable file object store for rebuildable archive scratch pages.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::archive::infrastructure::archive_integrity::sha256;

/// Non-durable file store for rebuildable archive scratch pages.
///
/// Archive scratch state is never an acknowledgement or recovery source. The
/// containing operation can restart from the committed manifest, so these
/// writes intentionally avoid one fsync per copy-on-write page.
pub(super) struct EphemeralFileObjectStore {
    pub(super) root: PathBuf,
    gate: Mutex<()>,
}

impl EphemeralFileObjectStore {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            gate: Mutex::new(()),
        }
    }

    pub(super) fn path(&self, key: &str) -> storage_object::Result<PathBuf> {
        let key = key.trim_matches('/');
        if key.is_empty()
            || key.contains('\0')
            || key.contains('\\')
            || key
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
        {
            return Err(storage_object::ObjectStoreError::InvalidKey {
                key: key.to_string(),
            });
        }
        Ok(self.root.join(key))
    }

    fn meta(&self, key: &str, bytes: &[u8], content_type: &str) -> storage_object::ObjectMeta {
        let version = storage_object::ObjectVersion::new(sha256(bytes));
        storage_object::ObjectMeta {
            key: key.to_string(),
            size: bytes.len() as u64,
            content_type: content_type.to_string(),
            version: version.clone(),
            etag: Some(version.as_str().to_string()),
            updated: None,
        }
    }
}

impl storage_object::ObjectStore for EphemeralFileObjectStore {
    fn put(
        &self,
        key: &str,
        bytes: &[u8],
        content_type: &str,
        condition: storage_object::PutCondition,
    ) -> storage_object::Result<storage_object::ObjectMeta> {
        let _gate = self.gate.lock().expect("archive scratch lock poisoned");
        let path = self.path(key)?;
        if matches!(condition, storage_object::PutCondition::IfAbsent) && path.exists() {
            return Err(storage_object::ObjectStoreError::PreconditionFailed {
                key: key.to_string(),
            });
        }
        if let storage_object::PutCondition::IfVersion(expected) = condition {
            let current =
                std::fs::read(&path).map_err(|error| storage_object::ObjectStoreError::Io {
                    message: error.to_string(),
                })?;
            if sha256(&current) != expected.as_str() {
                return Err(storage_object::ObjectStoreError::PreconditionFailed {
                    key: key.to_string(),
                });
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                storage_object::ObjectStoreError::Io {
                    message: error.to_string(),
                }
            })?;
        }
        std::fs::write(&path, bytes).map_err(|error| storage_object::ObjectStoreError::Io {
            message: error.to_string(),
        })?;
        Ok(self.meta(key, bytes, content_type))
    }

    fn get(&self, key: &str) -> storage_object::Result<storage_object::Object> {
        let _gate = self.gate.lock().expect("archive scratch lock poisoned");
        let path = self.path(key)?;
        let bytes = std::fs::read(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                storage_object::ObjectStoreError::NotFound {
                    key: key.to_string(),
                }
            } else {
                storage_object::ObjectStoreError::Io {
                    message: error.to_string(),
                }
            }
        })?;
        Ok(storage_object::Object {
            meta: self.meta(key, &bytes, "application/json"),
            bytes,
        })
    }

    fn head(&self, key: &str) -> storage_object::Result<storage_object::ObjectMeta> {
        self.get(key).map(|object| object.meta)
    }

    fn list(&self, prefix: &str) -> storage_object::Result<Vec<storage_object::ObjectMeta>> {
        let _gate = self.gate.lock().expect("archive scratch lock poisoned");
        let mut pending = vec![self.root.clone()];
        let mut objects = Vec::new();
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(&directory).map_err(|error| {
                storage_object::ObjectStoreError::Io {
                    message: error.to_string(),
                }
            })? {
                let entry = entry.map_err(|error| storage_object::ObjectStoreError::Io {
                    message: error.to_string(),
                })?;
                let file_type =
                    entry
                        .file_type()
                        .map_err(|error| storage_object::ObjectStoreError::Io {
                            message: error.to_string(),
                        })?;
                if file_type.is_symlink() {
                    return Err(storage_object::ObjectStoreError::UnsafePath {
                        path: entry.path().display().to_string(),
                    });
                }
                if file_type.is_dir() {
                    pending.push(entry.path());
                    continue;
                }
                let key = entry
                    .path()
                    .strip_prefix(&self.root)
                    .expect("scratch entry remains below root")
                    .to_string_lossy()
                    .replace('\\', "/");
                if key.starts_with(prefix) {
                    let bytes = std::fs::read(entry.path()).map_err(|error| {
                        storage_object::ObjectStoreError::Io {
                            message: error.to_string(),
                        }
                    })?;
                    objects.push(self.meta(&key, &bytes, "application/json"));
                }
            }
        }
        objects.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(objects)
    }

    fn delete(&self, key: &str) -> storage_object::Result<()> {
        let _gate = self.gate.lock().expect("archive scratch lock poisoned");
        let path = self.path(key)?;
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(storage_object::ObjectStoreError::Io {
                message: error.to_string(),
            }),
        }
    }
}
