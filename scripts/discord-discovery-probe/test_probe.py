import base64
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from contextlib import redirect_stdout
from types import SimpleNamespace
from unittest.mock import patch

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

spec = importlib.util.spec_from_file_location('probe', Path(__file__).with_name('probe.py'))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class DiscoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.db = self.root / 'Local Storage' / 'leveldb'
        self.db.mkdir(parents=True)
        self.key = bytes(range(32))
        self.token = (base64.urlsafe_b64encode(b'100000000000000001').rstrip(b'=')
                      + b'.' + b'A' * 6 + b'.' + b'B' * 38)
        self.write_state()

    def write_state(self, prefix=b'DPAPI'):
        state = {'os_crypt': {'encrypted_key': base64.b64encode(prefix + b'fixture').decode()}}
        (self.root / 'Local State').write_text(json.dumps(state))

    def encrypted(self, token=None, scheme=b'v10'):
        nonce = bytes(range(12))
        blob = scheme + nonce + AESGCM(self.key).encrypt(nonce, token or self.token, None)
        return probe.PREFIX + base64.b64encode(blob)

    def analyze(self):
        return probe.analyze(self.root, lambda value: self.key)

    def test_recovers_and_deduplicates_without_disclosing_secrets(self):
        encrypted = self.encrypted()
        (self.db / '000001.ldb').write_bytes(b'prefix' + encrypted + b'"tail')
        (self.db / '000002.log').write_bytes(encrypted + b'\x00')
        output = io.StringIO()
        with redirect_stdout(output):
            report = self.analyze()
        self.assertEqual(report['uniqueSessionCandidates'], 1)
        self.assertEqual(report['distinctAccountIds'], 1)
        self.assertEqual(report['decryptedRecords'], 2)
        rendered = json.dumps(report) + output.getvalue()
        self.assertNotIn(self.token.decode(), rendered)
        self.assertNotIn(encrypted.decode(), rendered)
        self.assertNotIn('100000000000000001', rendered)

    def test_bad_tag_is_not_a_recovered_session(self):
        encrypted = bytearray(base64.b64decode(self.encrypted()[len(probe.PREFIX):]))
        encrypted[-1] ^= 1
        (self.db / '1.log').write_bytes(probe.PREFIX + base64.b64encode(encrypted))
        report = self.analyze()
        self.assertEqual(report['uniqueSessionCandidates'], 0)
        self.assertEqual(report['decryptionFailures'], 1)

    def test_decrypted_garbage_is_not_a_session(self):
        (self.db / '1.log').write_bytes(self.encrypted(b'not a session'))
        self.assertEqual(self.analyze()['uniqueSessionCandidates'], 0)

    def test_unknown_scheme_is_reported_without_guessing(self):
        (self.db / '1.log').write_bytes(self.encrypted(scheme=b'v20'))
        report = self.analyze()
        self.assertEqual(report['unsupportedSchemes'], 1)
        self.assertEqual(report['uniqueSessionCandidates'], 0)

    def test_wrong_key_format_is_a_sanitized_error(self):
        self.write_state(prefix=b'OTHER')
        with self.assertRaisesRegex(probe.ProbeError, '^unsupported-key-wrapper$'):
            self.analyze()

    def test_wrong_key_size_is_rejected(self):
        with self.assertRaisesRegex(probe.ProbeError, '^unexpected-key-size$'):
            probe.analyze(self.root, lambda value: b'bad')

    def test_empty_storage(self):
        self.assertEqual(self.analyze()['uniqueSessionCandidates'], 0)

    def test_unrelated_files_are_ignored(self):
        (self.db / 'LOCK').write_bytes(self.encrypted())
        self.assertEqual(self.analyze()['filesRead'], 0)

    def test_bounded_scan(self):
        (self.db / '1.log').write_bytes(self.encrypted())
        report = probe.analyze(self.root, lambda value: self.key, max_file_bytes=1)
        self.assertEqual(report['filesSkipped'], 1)
        self.assertEqual(report['uniqueSessionCandidates'], 0)

    def test_does_not_modify_profile(self):
        path = self.db / '1.log'
        path.write_bytes(self.encrypted())
        before = {p: (p.read_bytes(), p.stat().st_mtime_ns) for p in self.root.rglob('*') if p.is_file()}
        self.analyze()
        after = {p: (p.read_bytes(), p.stat().st_mtime_ns) for p in self.root.rglob('*') if p.is_file()}
        self.assertEqual(before, after)

    def test_changed_file_is_skipped(self):
        path = self.db / '1.log'
        path.write_bytes(self.encrypted())
        changed = SimpleNamespace(st_size=path.stat().st_size + 1, st_mtime_ns=0)
        with patch.object(probe.os, 'fstat', return_value=changed):
            report = self.analyze()
        self.assertEqual(report['filesChangedDuringRead'], 1)
        self.assertEqual(report['filesSkipped'], 1)
        self.assertEqual(report['uniqueSessionCandidates'], 0)


if __name__ == '__main__':
    unittest.main()
