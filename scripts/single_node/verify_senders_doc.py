"""Check that the `## Senders` section of the single-node page still matches the code.

Every fact below is a text that must still be found in its file. A changed
constant or a removed path makes a test fail, so the page cannot keep a stale
number. Stdlib only.
"""

import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PAGE = ROOT / 'docs' / 'single-node-v1.md'

AGENT = 'src/operations/infrastructure/k8s/agent_workloads.rs'
METRICS_AGENT = 'src/operations/infrastructure/k8s/metrics_agent.rs'
CONFIG = 'src/collector/domain/config.rs'
RUNTIME = 'src/collector/application/runtime.rs'
CLIENT = 'src/collector/infrastructure/client.rs'
COLLECT = 'src/bin/sift/collect.rs'
SCRAPE = 'src/bin/sift/scrape.rs'
DURABLE = 'src/collector/infrastructure/durable_delivery.rs'
CRI_STATE = 'src/collector/infrastructure/cri_source/state.rs'
CRI_CHECKPOINT = 'src/collector/infrastructure/cri_checkpoint.rs'
CRI_DISCOVERY = 'src/collector/infrastructure/cri_discovery.rs'
SCRAPER = 'src/collector/infrastructure/metric_scraper.rs'
METRIC_QUEUE = 'src/collector/infrastructure/metric_queue.rs'

# (file, text that must be found in it), one row for each fact of the section.
FACTS = [
    (AGENT, 'agent_daemon_set_plan'),
    (AGENT, '"/var/log/pods".into()'),
    (METRICS_AGENT, '-agent-metrics'),
    (AGENT, '"requests":{"cpu":"25m","memory":"64Mi"},"limits":{"cpu":"500m","memory":"256Mi"}'),
    (CONFIG, 'DEFAULT_BATCH_SIZE: usize = 100'),
    (CONFIG, 'MAX_BATCH_SIZE: usize = 1000'),
    (CONFIG, 'DEFAULT_MAX_LINE_BYTES: usize = 512 * 1024'),
    (CONFIG, 'MAX_LINE_BYTES: usize = 1024 * 1024'),
    (CONFIG, 'DEFAULT_MAX_RETRIES: usize = 3'),
    (CONFIG, 'collector max_retries must not exceed 20'),
    (RUNTIME, 'Duration::from_secs(5)'),
    (RUNTIME, 'DeliveryRetryMode::UntilCancelled'),
    (AGENT, '"--follow".into()'),
    (CLIENT, 'status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()'),
    (COLLECT, 'request_timeout_secs: u64'),
    (COLLECT, 'default_value_t = 10'),
    (DURABLE, 'pending.len() > 1000'),
    (DURABLE, '64 * 1024 * 1024'),
    (CRI_STATE, 'lost_bytes'),
    (CRI_CHECKPOINT, 'lost_bytes'),
    (CRI_CHECKPOINT, 'pub(super) struct CriFileCheckpoint'),
    (CRI_CHECKPOINT, 'pub(super) retired: bool'),
    (CRI_STATE, 'file.retired = true'),
    (CRI_DISCOVERY, 'if depth > 3'),
    (SCRAPE, 'default_value_t = 30'),
    (METRICS_AGENT, '"--interval-seconds"'),
    (SCRAPER, 'prometheus.io/scrape'),
    (SCRAPER, 'sift.axiom.dev/scrape'),
    (SCRAPER, 'unwrap_or("9090")'),
    (SCRAPER, '8 * 1024 * 1024'),
    (SCRAPER, '.chunks(1000)'),
    (SCRAPER, 'config.interval.min(Duration::from_secs(30))'),
    (METRIC_QUEUE, '64 * 1024 * 1024'),
    (AGENT, '"--namespace".into()'),
    (AGENT, 'log_namespaces(cx, spec)'),
    (METRICS_AGENT, '"--namespace",'),
    (COLLECT, "value_delimiter = ','"),
    (SCRAPE, "value_delimiter = ','"),
]

PATHS = [
    CONFIG,
    DURABLE,
    METRIC_QUEUE,
    SCRAPER,
    CRI_CHECKPOINT,
    CLIENT,
    AGENT,
]

NUMBERS_AND_UNITS = [
    '25m', '64Mi', '500m', '256Mi', '1000', '512 KiB', '64 MiB', '8 MiB', '30 s', '9090',
    '/var/lib/sift/agent',
]

FORBIDDEN = ['production-ready', 'passed', 'verified']


def section(root=ROOT):
    lines = (root / 'docs' / 'single-node-v1.md').read_text(encoding='utf-8').splitlines()
    start = next((i for i, line in enumerate(lines) if line == '## Senders'), None)
    if start is None:
        raise AssertionError('docs/single-node-v1.md has no `## Senders` section')
    body = []
    for line in lines[start + 1:]:
        if line.startswith('## '):
            break
        body.append(line)
    return '\n'.join(body)


def check_fact(root, rel, text):
    path = root / rel
    return path.is_file() and text in path.read_text(encoding='utf-8')


class SendersDocTest(unittest.TestCase):
    def test_section_exists(self):
        self.assertGreater(len(section()), 1500)

    def test_every_fact_still_in_its_file(self):
        for rel, text in FACTS:
            with self.subTest(file=rel, text=text):
                self.assertTrue(check_fact(ROOT, rel, text))

    def test_stated_values_are_in_the_section(self):
        body = section()
        for value in NUMBERS_AND_UNITS:
            with self.subTest(value=value):
                self.assertIn(value, body)
        self.assertIn('not a GKE claim', body)
        self.assertIn('No live GKE evidence exists yet', body)

    def test_every_constant_has_its_path(self):
        body = section()
        for path in PATHS:
            with self.subTest(path=path):
                self.assertIn(path, body)

    def test_answers_the_namespace_question(self):
        self.assertRegex(section(), r'\*\*(Yes|No)\*\*')

    def test_no_forbidden_words(self):
        body = section().lower()
        for word in FORBIDDEN:
            with self.subTest(word=word):
                self.assertNotIn(word, body)

    def test_check_fact_fails_on_a_changed_constant(self):
        rel = DURABLE
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / rel).parent.mkdir(parents=True)
            (root / rel).write_text('if pending.len() > 2000 {}\n', encoding='utf-8')
            self.assertFalse(check_fact(root, rel, 'pending.len() > 1000'))
            self.assertTrue(check_fact(root, rel, 'pending.len() > 2000'))


if __name__ == '__main__':
    unittest.main()
