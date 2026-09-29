//! A set of blob hashes a restore or GC pass records and checks.

pub(crate) trait BlobHashSet {
    fn insert_hash(&mut self, hash: &str) -> anyhow::Result<()>;
    fn contains_hash(&self, hash: &str) -> anyhow::Result<bool>;
}
