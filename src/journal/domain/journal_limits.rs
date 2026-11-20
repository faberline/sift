//! How many events and bytes the journal keeps resident, and how many it reads per page.

pub(crate) const DEFAULT_RESIDENT_JOURNAL_EVENTS: usize = 100_000;

pub(crate) const DEFAULT_RESIDENT_JOURNAL_BYTES: usize = 256 * 1024 * 1024;

pub(in crate::journal) const RECOVERY_PAGE_EVENTS: usize = 10_000;

pub(in crate::journal) const RECOVERY_PAGE_BYTES: usize = 16 * 1024 * 1024;
