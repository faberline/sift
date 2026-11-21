"""Tests that the sift auth-surface contract does not call guard (issue #340)."""

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CONTRACT = ROOT / "external-contracts/security-hardening/sift-auth.md"
E2E_COMMAND = "cargo test -p sift --test it -- runtime_security_e2e:: --nocapture"


class AuthContract(unittest.TestCase):
    def contract(self):
        return CONTRACT.read_text(encoding="utf-8")

    def test_contract_does_not_call_guard(self):
        text = self.contract()
        for fragment in ("tool: guard", "guard scan", "sift-guard-auth-surface", "guard.toml", "tool-contract"):
            self.assertNotIn(fragment, text, fragment)

    def test_guard_manifest_is_gone(self):
        self.assertFalse((ROOT / "guard.toml").exists())

    def test_runtime_security_case_stays(self):
        text = self.contract()
        self.assertIn("id: sift-security-hardening-bearer-auth", text)
        self.assertIn(f'command: "{E2E_COMMAND}"', text)
        self.assertIn("fill_sections: [e2e-test]", text)

    def test_vat_runner_runs_the_same_suite(self):
        vat = (ROOT / "vat.toml").read_text(encoding="utf-8")
        self.assertIn('id = "ec-security"', vat)
        self.assertIn("runtime_security_e2e:: --nocapture", vat)

    def test_security_suite_is_registered_and_not_empty(self):
        main = (ROOT / "tests/it/main.rs").read_text(encoding="utf-8")
        self.assertIn("mod runtime_security_e2e;", main)
        suite = (ROOT / "tests/it/runtime_security_e2e.rs").read_text(encoding="utf-8")
        count = suite.count("#[tokio::test]") + suite.count("#[test]")
        self.assertGreaterEqual(count, 7)

    def test_ci_runs_this_test(self):
        ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn("python3 -B -m unittest test_threat_model test_auth_contract -v", ci)


if __name__ == "__main__":
    unittest.main()
