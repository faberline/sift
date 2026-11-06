//! How much local storage an append of a given size and event count reserves.

pub(crate) fn storage_reservation(encoded_bytes: u64, event_count: usize) -> u64 {
    encoded_bytes
        .saturating_mul(3)
        .saturating_add((event_count as u64).saturating_mul(128))
}
