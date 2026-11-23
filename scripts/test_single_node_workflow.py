from pathlib import Path
import json
import re
import shutil
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ".github/workflows/sift-single-node-acceptance.yml"
OVERLAY = "k8s/overlays/gke-single-node"
AW_PIN = "db865eec66388dd53ec68b0770a44443525daff2"


class SingleNodeWorkflowTest(unittest.TestCase):
    def read(self, name):
        return (ROOT / name).read_text(encoding="utf-8")

    def job(self, text, name):
        rest = text.split(f"\n  {name}:\n", 1)[1]
        match = re.search(r"\n  [A-Za-z_]+:\n", rest)
        return rest[: match.start()] if match else rest

    def test_manual_dispatch_only(self):
        text = self.read(WORKFLOW)
        self.assertIn("workflow_dispatch:", text)
        for trigger in ["push:", "pull_request:", "schedule:"]:
            self.assertNotIn(trigger, text)

    def test_dry_run_default_true(self):
        text = self.read(WORKFLOW)
        self.assertRegex(text, r"dry_run:\n(?:        .*\n)*?        default: true")

    def test_guard_uses_the_cost_guard(self):
        text = self.read(WORKFLOW)
        self.assertIn("cost_guard.py", text)
        self.assertIn("cost-inputs.single-node.json", text)

    def test_live_job_is_gone_and_resolve_is_gated(self):
        text = self.read(WORKFLOW)
        self.assertNotIn("  live:", text)
        self.assertIn("  deploy:", text)
        resolve = self.job(text, "resolve")
        self.assertIn("needs: guard", resolve)
        self.assertIn("!inputs.dry_run", resolve)

    def test_no_secret_or_host(self):
        text = self.read(WORKFLOW)
        self.assertNotIn("secrets.", text)
        self.assertNotIn("gke_", text)

    def test_top_level_permissions_empty(self):
        text = self.read(WORKFLOW)
        match = re.search(r"(?m)^permissions: \{\}$", text)
        self.assertIsNotNone(match)
        self.assertLess(match.start(), text.index("\njobs:\n"))

    def test_deploy_uses_the_pinned_aw_workflow(self):
        deploy = self.job(self.read(WORKFLOW), "deploy")
        self.assertIn(
            f"uses: faberline/aw/.github/workflows/gke-test.yml@{AW_PIN}", deploy
        )
        self.assertIn(f"aw-ref: {AW_PIN}", deploy)
        self.assertIn("namespace: sift-single-node", deploy)
        self.assertIn(f"overlay: {OVERLAY}", deploy)
        self.assertIn("ttl-minutes: 310", deploy)
        self.assertIn("queue-minutes: 30", deploy)
        for perm in ["id-token: write", "contents: read", "packages: read"]:
            self.assertIn(perm, deploy)
        self.assertNotIn("runs-on:", deploy)
        self.assertNotIn("steps:", deploy)

    def test_no_environment_and_no_secret(self):
        text = self.read(WORKFLOW)
        self.assertNotIn("environment:", text)
        self.assertNotIn("secrets.", text)

    def test_overlay_has_toleration_and_dynamic_rwo(self):
        sift = self.read(f"{OVERLAY}/sift.yaml")
        for text in [
            "topology: singleNode",
            'auth: "off"',
            "faberline.dev/pool: sift-server",
            "effect: NoSchedule",
            "storageClass: dynamic-rwo",
        ]:
            self.assertIn(text, sift)
        kustomization = self.read(f"{OVERLAY}/kustomization.yaml")
        for text in [
            "namespace: sift-single-node",
            "../../operator",
            "$patch: delete",
            "imagePullSecrets",
            "value: sift",
        ]:
            self.assertIn(text, kustomization)
        for name in ["kustomization.yaml", "sift.yaml", "images-config.yaml"]:
            self.assertNotIn("REPLACE_ME", self.read(f"{OVERLAY}/{name}"))

    def test_store_size_matches_cost_model(self):
        result = subprocess.run(
            [
                sys.executable,
                "-B",
                "scripts/production/cost_model.py",
                "--inputs",
                "scripts/production/cost-inputs.single-node.json",
                "--format",
                "json",
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        gib = json.loads(result.stdout)["store_pvc_gib"]
        self.assertIn(f"storeSize: {gib}Gi", self.read(f"{OVERLAY}/sift.yaml"))

    def test_overlay_renders_when_kubectl_exists(self):
        if shutil.which("kubectl") is None:
            self.skipTest("kubectl is not installed")
        result = subprocess.run(
            ["kubectl", "kustomize", OVERLAY],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertIn("kind: Sift", result.stdout)
        self.assertNotIn("kind: Namespace", result.stdout)
        self.assertIn("namespace: sift-single-node", result.stdout)

    def test_run_script_syntax_and_content(self):
        path = "scripts/single_node/acceptance/run.sh"
        text = self.read(path)
        self.assertTrue(text.startswith("#!/usr/bin/env bash\n"))
        for needle in [
            "set -euo pipefail",
            "setup.json",
            "data-sift-all-0",
            "ProvisioningFailed",
            "sift-all-0",
            "phases.py",
            "timeout",
            "EVIDENCE_DIR",
        ]:
            self.assertIn(needle, text)
        self.assertNotIn("gcloud", text)
        if shutil.which("bash") is None:
            self.skipTest("bash is not installed")
        result = subprocess.run(["bash", "-n", str(ROOT / path)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_phases_script_exists_and_is_stdlib(self):
        text = self.read("scripts/single_node/acceptance/phases.py")
        self.assertIn("PHASES = ('install_one_namespace', 'load_30m', 'load_24h', 'faults', 'comparison', 'retention', 'deleted_pod_logs')", text)
        self.assertNotIn("import requests", text)
        self.assertNotIn("import yaml", text)

    def test_run_script_guards_each_phase(self):
        text = self.read("scripts/single_node/acceptance/run.sh")
        for needle in [
            'phases.py" guard',
            "COST_CAP_USD",
            '--elapsed-seconds "$SECONDS"',
            "cost guard refused",
        ]:
            self.assertIn(needle, text)
        self.assertLess(text.index('phases.py" guard'), text.index('timeout "$limit"'))

    def test_deploy_passes_the_checked_cap(self):
        text = self.read(WORKFLOW)
        deploy = self.job(text, "deploy")
        guard = self.job(text, "guard")
        self.assertIn("needs: [guard, publish]", deploy)
        self.assertIn(
            "test-command: COST_CAP_USD=${{ needs.guard.outputs.cap_usd }} "
            "bash scripts/single_node/acceptance/run.sh",
            deploy,
        )
        self.assertIn("id: cap", guard)
        self.assertIn("outputs:", guard)
        self.assertNotIn("inputs.cost_cap_usd", deploy)

    def test_ci_runs_this_test(self):
        self.assertIn(
            "python3 -B scripts/test_single_node_workflow.py -v",
            self.read(".github/workflows/ci.yml"),
        )


if __name__ == "__main__":
    unittest.main()
