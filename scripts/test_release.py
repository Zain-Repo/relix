"""Release safety checks using temporary files and a mocked GitHub CLI."""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="relix-release-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.root_patch = patch.object(release, "ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)
        self.env_patch = patch.dict(os.environ, {
            "GITHUB_REPOSITORY": "example/relix", "GITHUB_SHA": "tested-commit",
        })
        self.env_patch.start()
        self.addCleanup(self.env_patch.stop)
        (self.root / "Cargo.toml").write_text('[package]\nversion = "0.1.0"\n')
        (self.root / "Cargo.lock").write_text('[[package]]\nname = "relix"\nversion = "0.1.0"\n')
        (self.root / "releases").mkdir()
        (self.root / "releases/v0.1.0.md").write_text("Implemented work and limitations\n")
        (self.root / "dist").mkdir()
        for suffix in release.TARGETS.values():
            name = f"Relix-0.1.0-{suffix}"
            data = b"synthetic archive"
            (self.root / "dist" / name).write_bytes(data)
            digest = hashlib.sha256(data).hexdigest()
            (self.root / "dist" / f"{name}.sha256").write_text(f"{digest}  {name}\n")

    def command(self, *args):
        if args == ("git", "rev-parse", "HEAD"):
            return "tested-commit"
        if args == ("git", "tag", "--list", "v*"):
            return ""
        if args == ("git", "tag", "--list", "v0.1.0"):
            return ""
        if args[:4] == ("gh", "api", "--method", "POST"):
            return ""
        if args[:2] == ("gh", "api") and "--paginate" in args:
            return "[[]]"
        if args[:3] == ("gh", "release", "view"):
            return '{"databaseId": 42}'
        if args[:3] == ("gh", "api", "repos/example/relix/releases/42"):
            assets = []
            for path in (self.root / "dist").iterdir():
                if path.suffix != ".sha256":
                    assets.append({"name": path.name, "size": path.stat().st_size,
                                   "digest": "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()})
            return json.dumps({"assets": assets})
        if args[:2] == ("gh", "release"):
            return ""
        raise AssertionError(f"Unexpected command: {args}")

    def test_manifest_and_lockfile_must_match(self):
        (self.root / "Cargo.lock").write_text('[[package]]\nname = "relix"\nversion = "0.0.9"\n')
        with self.assertRaisesRegex(ValueError, "must match"):
            release.version()

    def test_command_output_decodes_utf8_independently_of_windows_locale(self):
        output = release.run(sys.executable, "-c",
                             "import sys; sys.stdout.buffer.write(bytes([226, 128, 141]))")
        self.assertEqual(output, "\u200d")

    def test_missing_or_empty_notes_are_rejected(self):
        notes = self.root / "releases/v0.1.0.md"
        notes.write_text(" \n")
        with self.assertRaisesRegex(ValueError, "nonempty"):
            release.version()
        notes.unlink()
        with self.assertRaisesRegex(ValueError, "nonempty"):
            release.version()

    def test_prerelease_and_noncanonical_versions_are_rejected(self):
        for version in ["0.1.0-rc.1", "01.1.0", "0.1"]:
            (self.root / "Cargo.toml").write_text(f'[package]\nversion = "{version}"\n')
            with self.assertRaisesRegex(ValueError, "stable"):
                release.version()

    def test_existing_tag_on_another_commit_is_rejected(self):
        with patch.object(release, "run", side_effect=["tested-commit", "v0.1.0", "other-commit"]):
            with self.assertRaisesRegex(ValueError, "another commit"):
                release.validate_version()

    def test_older_version_is_rejected(self):
        with patch.object(release, "run", side_effect=["tested-commit", "v0.2.0"]):
            with self.assertRaisesRegex(ValueError, "newer"):
                release.validate_version()

    def test_checksum_failure_prevents_all_github_writes(self):
        (self.root / "dist/Relix-0.1.0-windows-x64.zip").write_bytes(b"corrupted")
        with patch.object(release, "run", side_effect=self.command) as command:
            with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                release.publish()
            self.assertFalse(any(call.args[0] == "gh" for call in command.call_args_list))

    def test_missing_platform_prevents_publication(self):
        (self.root / "dist/Relix-0.1.0-macos-x64.tar.gz").unlink()
        with patch.object(release, "run", side_effect=self.command) as command:
            with self.assertRaises(FileNotFoundError):
                release.publish()
            self.assertFalse(any(call.args[0] == "gh" for call in command.call_args_list))

    def test_all_assets_are_verified_before_publication(self):
        with patch.object(release, "run", side_effect=self.command) as command:
            release.publish()
            writes = [call.args for call in command.call_args_list
                      if call.args[:2] == ("gh", "release") and call.args[2] != "view"]
            self.assertEqual([call[2] for call in writes], ["create", "upload", "edit"])
            self.assertIn("--draft", writes[0])
            self.assertIn("--target", writes[0])
            self.assertIn("tested-commit", writes[0])
            self.assertIn("--draft=false", writes[-1])

    def test_remote_digest_failure_leaves_draft_unpublished(self):
        def command(*args):
            if args[:3] == ("gh", "api", "repos/example/relix/releases/42"):
                return '{"assets": []}'
            return self.command(*args)

        with patch.object(release, "run", side_effect=command) as calls:
            with self.assertRaisesRegex(ValueError, "draft retained"):
                release.publish()
            self.assertFalse(any(call.args[:3] == ("gh", "release", "edit") for call in calls.call_args_list))

    def test_github_api_errors_do_not_create_a_release(self):
        def command(*args):
            if args[:2] == ("gh", "api"):
                raise subprocess.CalledProcessError(1, args)
            return self.command(*args)

        with patch.object(release, "run", side_effect=command) as calls:
            with self.assertRaises(subprocess.CalledProcessError):
                release.publish()
            self.assertFalse(any(call.args[:2] == ("gh", "release") for call in calls.call_args_list))

    def test_published_release_is_immutable_on_retry(self):
        def command(*args):
            if args[:2] == ("gh", "api") and "--paginate" in args:
                return '[[{"tag_name": "v0.1.0", "draft": false}]]'
            if args[:3] == ("gh", "api", "repos/example/relix/commits/v0.1.0"):
                return '{"sha": "tested-commit"}'
            return self.command(*args)

        with patch.object(release, "run", side_effect=command) as calls:
            release.publish()
            self.assertFalse(any(call.args[:2] == ("gh", "release") for call in calls.call_args_list))

    def test_draft_retry_uploads_then_publishes(self):
        def command(*args):
            if args[:2] == ("gh", "api") and "--paginate" in args:
                return '[[{"tag_name": "v0.1.0", "draft": true}]]'
            if args[:3] == ("gh", "api", "repos/example/relix/commits/v0.1.0"):
                return '{"sha": "tested-commit"}'
            return self.command(*args)

        with patch.object(release, "run", side_effect=command) as calls:
            release.publish()
            writes = [call.args[2] for call in calls.call_args_list
                      if call.args[:2] == ("gh", "release") and call.args[2] != "view"]
            self.assertEqual(writes, ["upload", "edit"])

    def test_draft_tag_endpoint_404_does_not_block_publication(self):
        def command(*args):
            if args[:3] == ("gh", "api", "repos/example/relix/releases/tags/v0.1.0"):
                raise subprocess.CalledProcessError(1, args, output="Not Found (HTTP 404)")
            return self.command(*args)

        with patch.object(release, "run", side_effect=command) as calls:
            release.publish()
            self.assertTrue(any(call.args == ("gh", "api", "repos/example/relix/releases/42")
                                for call in calls.call_args_list))
            self.assertTrue(any(call.args[:3] == ("gh", "release", "edit")
                                for call in calls.call_args_list))


if __name__ == "__main__":
    unittest.main()
