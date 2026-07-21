import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class WorkflowCacheTests(unittest.TestCase):
    def test_ci_caches_are_restore_only_for_pull_requests(self):
        reusable_ci = (ROOT / ".github/workflows/rust-ci.yml").read_text()
        toolkit_ci = (ROOT / ".github/workflows/ci.yml").read_text()

        self.assertEqual(reusable_ci.count("uses: Swatinem/rust-cache@v2.9.1"), 3)
        self.assertEqual(reusable_ci.count("shared-key: ci-${{ github.repository_id }}-"), 3)
        self.assertEqual(reusable_ci.count("github.event_name == 'push'"), 3)
        self.assertNotIn("shared-key: release-", reusable_ci)

        self.assertIn("shared-key: ci-${{ github.repository_id }}-toolkit", toolkit_ci)
        self.assertIn("github.event_name == 'push'", toolkit_ci)
        self.assertNotIn("shared-key: release-", toolkit_ci)

    def test_release_cache_requires_verified_trusted_dispatch(self):
        publish = (ROOT / ".github/workflows/publish-release.yml").read_text()
        prepare = (ROOT / ".github/workflows/prepare-release.yml").read_text()

        verify_position = publish.index("verify-release:")
        build_position = publish.index("  build:")
        cache_position = publish.index("uses: Swatinem/rust-cache@v2.9.1")
        self.assertLess(verify_position, build_position)
        self.assertLess(build_position, cache_position)
        self.assertIn("needs: verify-release", publish)
        self.assertIn("shared-key: release-${{ github.repository_id }}", publish)
        self.assertIn("save-if: ${{ needs.verify-release.outputs.cache-write == 'true' }}", publish)
        self.assertIn('[[ "$GITHUB_EVENT_NAME" == workflow_dispatch ]]', publish)
        self.assertIn('[[ "$GITHUB_REF" == "refs/heads/${default_branch}" ]]', publish)
        self.assertIn('[[ "$tagged_commit" == "$RELEASE_COMMIT" ]]', publish)
        self.assertIn('git merge-base --is-ancestor "$RELEASE_COMMIT"', publish)
        self.assertNotIn("shared-key: ci-", publish)
        self.assertIn('gh workflow run "$PUBLISH_WORKFLOW" --ref main', prepare)

    def test_cached_executables_and_failed_builds_are_never_saved(self):
        workflow_paths = [
            ROOT / ".github/workflows/ci.yml",
            ROOT / ".github/workflows/rust-ci.yml",
            ROOT / ".github/workflows/publish-release.yml",
        ]
        for path in workflow_paths:
            workflow = path.read_text()
            cache_steps = workflow.count("uses: Swatinem/rust-cache@v2.9.1")
            self.assertEqual(workflow.count("cache-bin: false"), cache_steps, path)
            self.assertEqual(workflow.count("cache-on-failure: false"), cache_steps, path)
            self.assertEqual(workflow.count("save-if:"), cache_steps, path)


if __name__ == "__main__":
    unittest.main()
