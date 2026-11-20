"""Check that the single-node memory page still matches the code.

`docs/operations/single-node-memory.md` cites a file and a line for each
holder of memory. Every fact below is a text that must still be found in its
file. A changed constant, a moved eviction loop or a removed path makes a test
fail, so the page cannot keep a stale number. Stdlib only.
"""

import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PAGE = ROOT / 'docs' / 'operations' / 'single-node-memory.md'

# (file, text that must be found in it), one entry for each cited constant or
# code text of the page.
FACTS = [
    # Metric projection.
    ('src/projection/domain/metric.rs', 'DEFAULT_METRIC_MEMTABLE_BYTES: usize = 256 * 1024 * 1024'),
    ('src/projection/domain/metric.rs', 'DEFAULT_METRIC_CARDINALITY_LIMIT: usize = 10_000'),
    ('src/projection/domain/metric.rs', 'DEFAULT_RETAINED_POINTS_PER_SERIES: usize = 100_000'),
    ('src/projection/domain/metric.rs', 'MAX_METRIC_QUERY_SOURCE_BYTES: usize = 64 * 1024 * 1024'),
    ('src/projection/domain/metric_memory_size.rs', 'pub(super) fn metric_point_memory_bytes(point: &MetricPointV1)'),
    ('src/projection/infrastructure/metric_projection_maintenance.rs', 'fn enforce_memtable_limit(&self, state: &mut MetricState) -> Result<()> {'),
    ('src/projection/infrastructure/metric_projection_maintenance.rs', 'state.memtable_bytes > self.memtable_limit_bytes'),
    ('src/projection/infrastructure/metric_projection_maintenance.rs', 'while series.point_count > self.retained_points_per_series {'),
    ('src/projection/infrastructure/metric_projection_maintenance.rs', 'series.sealed_chunks.remove(0);'),
    ('src/projection/infrastructure/metric_projection_apply.rs', 'current_count >= self.cardinality_limit'),
    ('src/projection/infrastructure/metric_projection_apply.rs', 'self.evict_series_to_limit(series, stored.cursor)?'),
    ('src/projection/infrastructure/metric_projection_apply.rs', 'self.enforce_memtable_limit(&mut state)?;'),
    ('src/projection/domain/metric_series.rs', 'pub(in crate::projection) sealed_chunks: Vec<SealedMetricChunk>,'),
    ('src/projection/domain/metric_series.rs', 'pub(in crate::projection) rollups: Vec<MetricRollupV1>,'),
    ('src/app/projection_wiring.rs', 'MetricProjection::open(&metric_data_dir)'),
    # Log projection.
    ('src/projection/domain/log.rs', 'DEFAULT_RETAINED_LOG_RECORDS: usize = 100_000'),
    ('src/projection/infrastructure/logging_projection.rs', 'while state.records.len() > self.max_records {'),
    ('src/projection/infrastructure/logging_projection.rs', 'state.records.remove(&oldest)'),
    ('src/projection/infrastructure/logging_projection.rs', '.delete(&DocumentId::new(projection_row_key(oldest)), None)?;'),
    ('src/ingest/domain/governance_policy.rs', 'const DEFAULT_MAX_STRING_BYTES: usize = 4_096;'),
    # Trace projection.
    ('src/projection/domain/trace.rs', 'DEFAULT_RETAINED_TRACE_SPANS: usize = 100_000'),
    ('src/projection/domain/trace.rs', 'DEFAULT_RETAINED_TRACE_BYTES: usize = 256 * 1024 * 1024'),
    ('src/projection/infrastructure/trace_projection.rs', 'state.location_by_cursor.len() > self.max_spans'),
    ('src/projection/infrastructure/trace_projection.rs', 'state.retained_bytes > self.max_bytes'),
    # Journal.
    ('src/journal/domain/journal_limits.rs', 'DEFAULT_RESIDENT_JOURNAL_EVENTS: usize = 100_000'),
    ('src/journal/domain/journal_limits.rs', 'DEFAULT_RESIDENT_JOURNAL_BYTES: usize = 256 * 1024 * 1024'),
    ('src/journal/application/recover_journal.rs', 'while state.recent_events.len() > resident_limit {'),
    ('src/journal/application/recover_journal.rs', 'state.resident_bytes > resident_byte_limit'),
    ('src/archive/application/evict_cold_segments.rs', 'journal.evict_resident_before(cutoff)?;'),
    ('src/journal/infrastructure/storage/wal.rs', 'events: VecDeque<StoredEvent>,'),
    ('src/journal/infrastructure/storage/dedupe_index.rs', 'BACKGROUND_FLUSH_ENTRIES: usize = 100_000'),
    ('src/journal/infrastructure/storage/dedupe_index.rs', 'pub(super) const MAX_PENDING_ENTRIES: usize = BACKGROUND_FLUSH_ENTRIES * 2;'),
    ('src/journal/infrastructure/storage/dedupe_append.rs', 'if pending.saturating_add(incoming_entries) > MAX_PENDING_ENTRIES {'),
    ('src/journal/infrastructure/storage/dedupe_shard_file.rs', 'ENTRY_BYTES: usize = 48'),
    ('src/journal/infrastructure/storage/dedupe_generation_store.rs', 'pub(super) pending: BTreeMap<[u8; 32], (u64, i64)>,'),
    ('src/journal/domain/bloom_filter.rs', 'MAX_BLOOM_BYTES: usize = 64 * 1024 * 1024'),
    ('src/shared_kernel/raft_batch_limits.rs', 'RAFT_BATCH_MAX_BYTES: usize = 1_048_576'),
    ('src/journal/domain/journal_limits.rs', 'RECOVERY_PAGE_BYTES: usize = 16 * 1024 * 1024'),
    ('src/journal/infrastructure/storage/segment_read.rs', 'paths: VecDeque<(PathBuf, u64)>,'),
    ('src/journal/infrastructure/storage/segment_read.rs', 'SEGMENT_READER_MAX_PATHS: usize = 1_024'),
    # Ingest and admission.
    ('src/ingest/domain/ingest_limits.rs', 'max_compressed_body_bytes: 1_048_576,'),
    ('src/ingest/domain/ingest_limits.rs', 'max_decoded_body_bytes: 8_388_608,'),
    ('src/ingest/domain/ingest_limits.rs', 'max_event_bytes: 262_144,'),
    ('src/ingest/domain/ingest_limits.rs', 'max_events_per_batch: 1_000,'),
    ('src/ingest/domain/ingest_limits.rs', 'max_concurrent_requests_per_project: 32,'),
    ('src/ingest/domain/ingest_limits.rs', 'max_items_per_project_window: 720_000,'),
    ('src/ingest/application/admission_controller.rs', 'projects: service_http::WeightedAdmission<String>,'),
    ('src/ingest/application/admission_controller.rs', '            65_536,'),
    ('src/ingest/application/admission_controller.rs', 'service_http::WeightedAdmissionError::KeyLimitExceeded => AdmissionError::new('),
    # Query.
    ('src/query/domain/prom_models.rs', 'const PROM_MAX_SERIES: usize = 1_000;'),
    ('src/query/domain/promql_evaluator.rs', 'const PROM_MAX_RANGE_WORK_SAMPLES: usize = 1_000_000;'),
    # Archive cache (disk).
    ('src/archive/infrastructure/archive_segment_cache.rs', 'const ARCHIVE_CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;'),
    ('src/archive/infrastructure/archive_segment_cache.rs', 'if total <= ARCHIVE_CACHE_MAX_BYTES {'),
    # Question 2: the one timer that runs when ingest stops.
    ('src/projection/interfaces/projection_worker.rs', '_ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {'),
]

REQUIRED_SECTIONS = [
    '## Holders',
    '## Does memory go down after ingest stops',
    '## Unbounded holders',
]

FORBIDDEN = ['production-ready', 'passed', 'verified']


def page_text(root=ROOT):
    return (root / 'docs' / 'operations' / 'single-node-memory.md').read_text(encoding='utf-8')


def holders_rows(text):
    """Return the data rows of the `## Holders` table, without header and separator."""
    lines = text.splitlines()
    start = next((i for i, line in enumerate(lines) if line == '## Holders'), None)
    if start is None:
        raise AssertionError('the page has no `## Holders` section')
    rows = []
    for line in lines[start + 1:]:
        if line.startswith('## '):
            break
        if not line.startswith('|'):
            continue
        cells = [cell.strip() for cell in line.strip().strip('|').split('|')]
        if cells[0] == 'Holder' or set(''.join(cells)) <= set('-: '):
            continue
        rows.append(cells)
    return rows


def check_fact(root, rel, text):
    path = root / rel
    return path.is_file() and text in path.read_text(encoding='utf-8')


class SingleNodeMemoryDocTest(unittest.TestCase):
    def test_page_exists(self):
        self.assertTrue(PAGE.is_file(), f'missing {PAGE.relative_to(ROOT)}')

    def test_facts_cover_the_holders(self):
        self.assertGreaterEqual(len(FACTS), 20)

    def test_every_fact_still_in_its_file(self):
        for rel, text in FACTS:
            with self.subTest(file=rel, text=text):
                self.assertTrue(check_fact(ROOT, rel, text))

    def test_every_cited_path_is_in_the_page(self):
        text = page_text()
        for rel in sorted({rel for rel, _ in FACTS}):
            with self.subTest(path=rel):
                self.assertIn(rel, text)

    def test_page_has_the_required_sections(self):
        lines = page_text().splitlines()
        for heading in REQUIRED_SECTIONS:
            with self.subTest(heading=heading):
                self.assertIn(heading, lines)

    def test_every_table_row_has_a_line_number(self):
        rows = holders_rows(page_text())
        self.assertGreaterEqual(len(rows), 20)
        for cells in rows:
            with self.subTest(row=cells[0]):
                self.assertRegex(cells[-1], r'\.rs:\d+')

    def test_no_forbidden_words(self):
        text = page_text().lower()
        for word in FORBIDDEN:
            with self.subTest(word=word):
                self.assertNotIn(word, text)

    def test_check_fact_fails_on_a_changed_constant(self):
        rel = 'src/projection/domain/log.rs'
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / rel).parent.mkdir(parents=True)
            (root / rel).write_text(
                'pub const DEFAULT_RETAINED_LOG_RECORDS: usize = 200_000;\n',
                encoding='utf-8',
            )
            self.assertFalse(check_fact(root, rel, 'DEFAULT_RETAINED_LOG_RECORDS: usize = 100_000'))
            self.assertTrue(check_fact(root, rel, 'DEFAULT_RETAINED_LOG_RECORDS: usize = 200_000'))


if __name__ == '__main__':
    unittest.main()
