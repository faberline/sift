//! spill service over the original resource port.

use crate::archive::domain::blob_hash_set::BlobHashSet;
use crate::archive::domain::resource_ports::*;
use crate::shared_kernel::catalog_manifest::CatalogRoot;
use crate::shared_kernel::sha256_hex::sha256_hex;
use anyhow::Result;

type CatalogError = storage_segment::SegmentError;
type SpillPort = dyn ArchiveSpillPort<Error = anyhow::Error, CatalogError = CatalogError>;

pub(crate) struct SpillService {
    port: Box<SpillPort>,
    pub(crate) root: CatalogRoot,
}
impl SpillService {
    pub(crate) fn new(port: Box<SpillPort>) -> Self {
        let root = port.root();
        Self { port, root }
    }
    pub(crate) fn upsert(&mut self, key: String, value: Vec<u8>) -> Result<()> {
        self.port.upsert(key, value)?;
        self.root = self.port.root();
        Ok(())
    }
    pub(crate) fn lookup(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.port.lookup(key)
    }
    pub(crate) fn remove(&mut self, key: &str) -> Result<()> {
        self.port.remove(key)?;
        self.root = self.port.root();
        Ok(())
    }
    pub(crate) fn reader(&self) -> Result<CatalogReader<CatalogError>> {
        self.port.reader()
    }
    pub(crate) fn len(&self) -> u64 {
        self.root.entry_count
    }
    pub(crate) fn add_u64(&mut self, key: String, amount: u64) -> Result<()> {
        let current = self
            .lookup(&key)?
            .map(|bytes| {
                bytes
                    .as_slice()
                    .try_into()
                    .map(u64::from_le_bytes)
                    .map_err(|_| anyhow::anyhow!("archive spill count is corrupt"))
            })
            .transpose()?
            .unwrap_or_default();
        self.upsert(key, current.saturating_add(amount).to_le_bytes().to_vec())
    }
    pub(crate) fn get_u64(&self, key: &str) -> Result<u64> {
        self.lookup(key)?
            .map(|bytes| {
                bytes
                    .as_slice()
                    .try_into()
                    .map(u64::from_le_bytes)
                    .map_err(|_| anyhow::anyhow!("archive spill count is corrupt"))
            })
            .transpose()
            .map(|value| value.unwrap_or_default())
    }
    pub(crate) fn insert_uri(&mut self, uri: &str) -> Result<()> {
        self.upsert(
            format!("uri/{}", sha256_hex(uri.as_bytes())),
            uri.as_bytes().to_vec(),
        )
    }
    pub(crate) fn remove_uri(&mut self, uri: &str) -> Result<()> {
        self.remove(&format!("uri/{}", sha256_hex(uri.as_bytes())))
    }
    pub(crate) fn contains_uri(&self, uri: &str) -> Result<bool> {
        Ok(self
            .lookup(&format!("uri/{}", sha256_hex(uri.as_bytes())))?
            .is_some())
    }
}
impl BlobHashSet for SpillService {
    type Error = anyhow::Error;
    fn insert_hash(&mut self, hash: &str) -> Result<()> {
        self.upsert(format!("blob/{hash}"), Vec::new())
    }
    fn contains_hash(&self, hash: &str) -> Result<bool> {
        Ok(self.lookup(&format!("blob/{hash}"))?.is_some())
    }
}
