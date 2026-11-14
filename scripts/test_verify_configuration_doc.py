import importlib.util
import pathlib
import tempfile
import unittest

import yaml

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent
_spec = importlib.util.spec_from_file_location(
    "verify_configuration_doc", HERE / "verify-configuration-doc.py"
)
vcd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(vcd)


def small_crd():
    return {
        "spec": {
            "versions": [
                {
                    "schema": {
                        "openAPIV3Schema": {
                            "properties": {
                                "spec": {
                                    "properties": {
                                        "storage": {
                                            "properties": {
                                                "storeSize": {
                                                    "type": "string",
                                                    "default": "50Gi",
                                                }
                                            }
                                        }
                                    }
                                },
                                "status": {
                                    "properties": {"phase": {"type": "string"}}
                                },
                            }
                        }
                    }
                }
            ]
        }
    }


HELP = """Usage: sift serve [OPTIONS]

Commands:
  collect  Run the collector
  help     Print this message or the help of the given subcommand(s)

Options:
  -p, --port <PORT>  Listener port [default: 7380]
      --ephemeral    Dev only
  -h, --help         Print help
"""


class ConfigurationDoc(unittest.TestCase):
    def test_crd_paths_lists_nested_fields_and_defaults(self):
        paths = vcd.crd_paths(small_crd())
        self.assertIn("spec.storage", paths)
        self.assertEqual(paths["spec.storage.storeSize"], "50Gi")
        self.assertIn("status.phase", paths)
        self.assertIsNone(paths["status.phase"])

    def test_real_crd_has_the_known_fields(self):
        crd = yaml.safe_load((ROOT / "k8s/crd/sift.yaml").read_text())
        paths = vcd.crd_paths(crd)
        self.assertIn("spec.peerTlsSecret", paths)
        self.assertIn("spec.alerting.allowedHosts", paths)
        self.assertEqual(paths["spec.ingest.maxItemsPerMinute"], "720000")
        self.assertIn("status.lastDataError", paths)

    def test_check_fails_when_a_crd_field_is_missing(self):
        # Without the checker this file does not import, so the test fails.
        page = "| `spec.storage.storeSize` | string | 50Gi |\n"
        messages = vcd.check(page, small_crd(), set(), None)
        self.assertTrue(
            any("missing CRD field spec.storage" in m for m in messages), messages
        )

    def test_check_fails_on_a_wrong_crd_default(self):
        page = (
            "| `spec.storage` | object | - |\n"
            "| `spec.storage.storeSize` | string | 10Gi |\n"
            "| `status.phase` | string | none |\n"
        )
        messages = vcd.check(page, small_crd(), set(), None)
        self.assertTrue(
            any("wrong default for spec.storage.storeSize" in m for m in messages),
            messages,
        )

    def test_check_fails_when_an_env_var_is_missing(self):
        messages = vcd.check("no variables here\n", {}, {"SIFT_TOKEN"}, None)
        self.assertEqual(messages, ["missing environment variable SIFT_TOKEN"])
        page = "| `SIFT_TOKEN` | - | none | token |\n"
        self.assertEqual(vcd.check(page, {}, {"SIFT_TOKEN"}, None), [])

    def test_parse_help_reads_flags_defaults_and_subcommands(self):
        subs, flags = vcd.parse_help(HELP)
        self.assertEqual(subs, ["collect"])
        self.assertEqual(flags["--port"], "7380")
        self.assertIsNone(flags["--ephemeral"])
        self.assertNotIn("--help", flags)

    def test_check_flags_by_command_section(self):
        tree = {"sift serve": {"--port": "7380", "--host": None}}
        page = "### `sift serve`\n\n| `--port` | 7380 |\n"
        self.assertEqual(
            vcd.check(page, {}, set(), tree),
            ["missing flag --host in sift serve"],
        )
        self.assertEqual(
            vcd.check("no sections\n", {}, set(), tree),
            ["missing command section sift serve"],
        )
        wrong = "### `sift serve`\n\n| `--port` | 9999 |\n| `--host` | - |\n"
        messages = vcd.check(wrong, {}, set(), tree)
        self.assertEqual(
            messages, ["wrong default for --port in sift serve (help: 7380)"]
        )

    def test_main_exit_codes(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            (tmp / "src").mkdir()
            (tmp / "src" / "lib.rs").write_text('const X: &str = "SIFT_X";\n')
            crd = tmp / "crd.yaml"
            crd.write_text(yaml.safe_dump(small_crd()))
            page = tmp / "page.md"

            page.write_text("# nothing\n")
            args = ["--page", str(page), "--crd", str(crd), "--root", str(tmp)]
            self.assertEqual(vcd.main(args), 1)

            page.write_text(
                "| `spec.storage` | object | - |\n"
                "| `spec.storage.storeSize` | string | 50Gi |\n"
                "| `status.phase` | string | none |\n"
                "| `SIFT_X` | - | none | a test variable |\n"
                "| `SIFT_PEER_TLS_CERT` | - | none | peer certificate |\n"
                "| `SIFT_PEER_TLS_KEY` | - | none | peer key |\n"
                "| `SIFT_PEER_TLS_CA` | - | none | peer CA |\n"
                "| `SIFT_PEER_MTLS` | - | none | peer mTLS switch |\n"
            )
            self.assertEqual(vcd.main(args), 0)

            missing = tmp / "absent.md"
            self.assertEqual(
                vcd.main(
                    ["--page", str(missing), "--crd", str(crd), "--root", str(tmp)]
                ),
                2,
            )


if __name__ == "__main__":
    unittest.main()
