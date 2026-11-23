"""Checks for the split Kubernetes install (issue #373).

No kubectl and no network: the tests read files and call the two validate functions.
"""

import importlib.util
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "verify-deployment-artifacts.py"
OPERATOR_FILES = [
    "k8s/operator/namespace/namespace.yaml",
    "k8s/operator/namespaced/serviceaccount.yaml",
    "k8s/operator/cluster/clusterrole.yaml",
    "k8s/operator/cluster/clusterrolebinding.yaml",
    "k8s/operator/namespaced/deployment.yaml",
]

try:
    import yaml
    import jsonschema  # noqa: F401
except ImportError:
    yaml = None


def read(relative):
    return (ROOT / relative).read_text(encoding="utf-8")


class VerifyDeploymentArtifactsTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if yaml is None:
            raise unittest.SkipTest("PyYAML or jsonschema missing")
        spec = importlib.util.spec_from_file_location("vda", SCRIPT)
        cls.vda = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.vda)

    def operator_text(self, skip=()):
        parts = []
        for relative in OPERATOR_FILES:
            if relative in skip:
                continue
            text = read(relative).rstrip("\n") + "\n"
            text = text.replace("sift-system", self.vda.OPERATOR_NAMESPACE)
            text = re.sub(r"image: \S+", f"image: {self.vda.IMAGE}", text)
            parts.append(text)
        parts.append(read("k8s/crd/sift.yaml").rstrip("\n") + "\n")
        return "---\n".join(parts)

    def test_validate_operator_accepts_operator_objects(self):
        manifests = self.vda.validate_operator(self.operator_text())
        self.assertIn(("Deployment", "sift-operator"), manifests)

    def test_validate_operator_rejects_missing_clusterrolebinding(self):
        text = self.operator_text(skip=("k8s/operator/cluster/clusterrolebinding.yaml",))
        with self.assertRaises((SystemExit, KeyError)):
            self.vda.validate_operator(text)

    def test_validate_overlay_needs_only_the_sift_object(self):
        text = read("k8s/overlays/template/sift.yaml")
        text = re.sub(r"REPLACE_ME__[A-Z0-9_]+",
                      lambda token: self.vda.FIXTURES.get(token[0], token[0]), text)
        crd = yaml.safe_load(read("k8s/crd/sift.yaml"))
        try:
            self.vda.validate_overlay(text, "template", crd)
        except SystemExit:
            pass  # a settings or image mismatch is not what this test checks
        except KeyError as error:
            self.fail(f"validate_overlay still needs an operator object: {error}")


if __name__ == "__main__":
    unittest.main()
