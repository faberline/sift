from pathlib import Path
import re
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
PAGE = "docs/operations/release-profiles.md"


class ReleaseProfilesDocTest(unittest.TestCase):
    def read(self, name):
        return (ROOT / name).read_text(encoding="utf-8")

    def test_sections_in_order(self):
        headings = [
            line
            for line in self.read(PAGE).splitlines()
            if line.startswith("## ")
        ]
        self.assertEqual(
            headings,
            [
                "## Profiles",
                "## Version and tag",
                "## Workflow design",
                "## Production profile is unchanged",
                "## Open decision",
            ],
        )

    def test_both_profiles_named(self):
        page = self.read(PAGE)
        self.assertIn("production", page)
        self.assertIn("single-node", page)

    def test_proposal_marker(self):
        self.assertIn(
            "Proposal: the owner confirms before any tag",
            self.read(PAGE),
        )

    def test_workflows_name_single_node_only_in_the_profile_parts(self):
        description = (
            "        description: Release profile. production keeps every current gate;"
            " single-node is the profile of docs/single-node-v1.md.\n"
        )
        allowed = {
            ".github/workflows/sift-release.yml": [
                description,
                "          - single-node\n",
                "      - name: Stop the single-node profile until its receipt step is wired\n",
                "        if: ${{ inputs.profile == 'single-node' }}\n",
                '          echo "single-node receipt step not yet wired" >&2\n',
            ],
            ".github/workflows/sift-release-candidate.yml": [
                description,
                "          - single-node\n",
                '          [[ "$REQUESTED_PROFILE" == "production" || "$REQUESTED_PROFILE" == "single-node" ]]'
                ' || { echo "invalid profile input" >&2; exit 1; }\n',
                "            ${{ inputs.profile == 'single-node' && 'profile=single-node' || '' }}\n",
            ],
        }
        for name, lines in allowed.items():
            text = self.read(name)
            found = [line for line in text.splitlines(keepends=True) if "single-node" in line]
            self.assertEqual(found, lines, name)
            self.assertIn(
                "        type: choice\n        default: production\n        options:\n"
                "          - production\n          - single-node\n",
                text,
                name,
            )

    def test_production_jobs_are_unchanged(self):
        jobs = {
            ".github/workflows/sift-release.yml": ["verify-inputs", "publish-release"],
            ".github/workflows/sift-release-candidate.yml": [
                "identity",
                "build",
                "sift-release-gates",
                "ghcr-image-and-attest",
                "manifest",
                "verify-candidate",
                "result",
            ],
        }
        for name, expected in jobs.items():
            text = self.read(name)
            found = re.findall(r"^  ([A-Za-z0-9_-]+):$", text.split("\njobs:\n", 1)[1], re.M)
            self.assertEqual(found, expected, name)

    def run_profile_notes(self, *args):
        return subprocess.run(
            ["bash", "scripts/release/profile-notes.sh", *args],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )

    def test_profile_notes_production_prints_nothing(self):
        result = self.run_profile_notes("production")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")

    def test_profile_notes_single_node_lines(self):
        result = self.run_profile_notes("single-node")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            result.stdout,
            "- Profile: profile=single-node\n"
            "- Claim: this release makes no claim for the three-voter topology.\n",
        )

    def test_profile_notes_rejects_other_input(self):
        for args in (["staging"], []):
            result = self.run_profile_notes(*args)
            self.assertEqual(result.returncode, 2, args)
            self.assertEqual(result.stdout, "", args)

    def test_candidate_label_only_for_single_node(self):
        lines = self.read(".github/workflows/sift-release-candidate.yml").splitlines()
        start = lines.index("          labels: |")
        block = []
        for line in lines[start + 1:]:
            if len(line) - len(line.lstrip()) < 12:
                break
            block.append(line)
        self.assertEqual(
            block,
            [
                "            org.opencontainers.image.source=https://github.com/${{ github.repository }}",
                "            org.opencontainers.image.revision=${{ needs.identity.outputs.commit }}",
                "            org.opencontainers.image.version=${{ needs.identity.outputs.version }}",
                "            org.opencontainers.image.url=https://github.com/${{ github.repository }}/actions/runs/${{ github.run_id }}/attempts/${{ github.run_attempt }}",
                "            ${{ inputs.profile == 'single-node' && 'profile=single-node' || '' }}",
            ],
        )

    def test_promotion_appends_profile_notes_after_the_heredoc(self):
        text = self.read(".github/workflows/sift-release.yml")
        self.assertEqual(text.count("RELEASE_PROFILE: ${{ inputs.profile }}"), 1)
        call = 'bash scripts/release/profile-notes.sh "$RELEASE_PROFILE" >> release-notes.md'
        self.assertEqual(text.count(call), 1)
        self.assertGreater(text.index(call), text.index("cat > release-notes.md <<EOF"))
        self.assertLess(text.index(call), text.index("gh release create"))
        self.assertEqual(
            text.count(
                "          - Compatibility: no HTTP, CLI, wire format, on-disk format, "
                "or Kubernetes manifest shape changed in this release.\n"
            ),
            1,
        )

    def test_readme_links_the_page(self):
        self.assertIn(
            "operations/release-profiles.md",
            self.read("docs/README.md"),
        )


if __name__ == "__main__":
    unittest.main()
