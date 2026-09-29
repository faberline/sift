//! The scalable Bloom filter layers that screen dedupe lookups.

pub(in crate::journal) const MIN_BLOOM_BYTES: usize = 1024 * 1024;

const MAX_BLOOM_BYTES: usize = 64 * 1024 * 1024;

const BLOOM_BITS_PER_ENTRY: u64 = 12;

pub(in crate::journal) struct BloomLayer {
    pub(in crate::journal) bits: Vec<u8>,
    entries: u64,
}

impl BloomLayer {
    pub(in crate::journal) fn new(bytes: usize) -> Self {
        Self {
            bits: vec![0; bytes],
            entries: 0,
        }
    }

    fn is_full(&self) -> bool {
        self.entries >= bloom_entries_for_bytes(self.bits.len())
    }
}

fn bloom_entries_for_bytes(bytes: usize) -> u64 {
    (bytes as u64)
        .saturating_mul(8)
        .checked_div(BLOOM_BITS_PER_ENTRY)
        .unwrap_or_default()
        .max(1)
}

pub(in crate::journal) fn bloom_insert_scalable(layers: &mut Vec<BloomLayer>, digest: &[u8; 32]) {
    if layers.last().is_none_or(BloomLayer::is_full) {
        let bytes = layers
            .last()
            .map(|layer| layer.bits.len().saturating_mul(2))
            .unwrap_or(MIN_BLOOM_BYTES)
            .min(MAX_BLOOM_BYTES);
        layers.push(BloomLayer::new(bytes));
    }
    let layer = layers.last_mut().expect("Bloom layer exists");
    bloom_insert(&mut layer.bits, digest);
    layer.entries = layer.entries.saturating_add(1);
}

fn bloom_insert(bloom: &mut [u8], digest: &[u8; 32]) {
    for position in bloom_positions(digest, bloom.len() * 8) {
        bloom[position / 8] |= 1 << (position % 8);
    }
}

pub(in crate::journal) fn bloom_contains(bloom: &[u8], digest: &[u8; 32]) -> bool {
    bloom_positions(digest, bloom.len() * 8)
        .into_iter()
        .all(|position| bloom[position / 8] & (1 << (position % 8)) != 0)
}

fn bloom_positions(digest: &[u8; 32], bits: usize) -> [usize; 4] {
    let word = |offset| {
        u64::from_le_bytes(
            digest[offset..offset + 8]
                .try_into()
                .expect("fixed digest word"),
        ) as usize
            % bits
    };
    [word(0), word(8), word(16), word(24)]
}
