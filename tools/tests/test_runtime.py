"""Keep the upgraded runtime's build and deployment identity consistent."""
import json
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class RuntimePinTests(unittest.TestCase):
    def setUp(self):
        self.lock = json.loads((ROOT / 'deploy/bot/runtime.lock.json').read_text())

    def test_source_archive_is_the_reviewed_090_revision(self):
        self.assertEqual(self.lock['version'], '0.9.0')
        self.assertEqual(self.lock['revision'], '6c78ba59bce6a3d5079cf0c0310488770e264ad4')
        self.assertEqual(
            self.lock['source_url'],
            'https://api.github.com/repos/zeroclaw-labs/zeroclaw/tarball/'
            '6c78ba59bce6a3d5079cf0c0310488770e264ad4',
        )
        self.assertEqual(
            self.lock['source_sha256'],
            'f3a5618a8f7361444c743e2c96c4691488bce17970b38ac5f691c4dfb668f342',
        )

    def test_build_download_and_checksum_match_the_runtime_lock(self):
        dockerfile = (ROOT / 'deploy/bot/Dockerfile').read_text()
        downloads = re.findall(
            r'(https://api\.github\.com/repos/zeroclaw-labs/zeroclaw/tarball/\S+)'
            r' -o /tmp/runtime\.tar\.gz', dockerfile,
        )
        checksums = re.findall(
            r"echo '([0-9a-f]{64})  /tmp/runtime\.tar\.gz' \| sha256sum -c -",
            dockerfile,
        )
        self.assertEqual(downloads, [self.lock['source_url']])
        self.assertEqual(checksums, [self.lock['source_sha256']])

    def test_image_labels_notice_and_startup_report_the_pinned_runtime(self):
        dockerfile = (ROOT / 'deploy/bot/Dockerfile.artifact').read_text()
        for field in ['version', 'revision']:
            with self.subTest(field=field):
                labels = re.findall(r'io\.heimdall\.runtime\.' + field + r'="([^"]+)"', dockerfile)
                self.assertEqual(labels, [self.lock[field]])
        notice = (ROOT / 'deploy/bot/NOTICE.md').read_text()
        self.assertIn(
            f"ZeroClaw v{self.lock['version']}, revision {self.lock['revision']},",
            notice,
        )
        launcher = (ROOT / 'launcher/src/main.rs').read_text()
        self.assertEqual(
            re.findall(r'HeimdallClawd: ZeroClaw ([^;]+);', launcher),
            [self.lock['version']],
        )


if __name__ == '__main__':
    unittest.main()
