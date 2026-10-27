//! A set of blob hashes a restore or GC pass records and checks.

pub(crate) trait BlobHashSet {
    type Error;

    fn insert_hash(&mut self, hash: &str) -> Result<(), Self::Error>;
    fn contains_hash(&self, hash: &str) -> Result<bool, Self::Error>;
}
