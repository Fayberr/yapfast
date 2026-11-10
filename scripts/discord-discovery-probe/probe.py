"""Windows Discord storage experiment. Prints counts only, never credentials.

Explicit --live is required. Does not use the network or change Discord's files.
Raw-file discovery is deliberately a first-stage experiment: it can find historical
records and miss compressed/fragmented records. A result is not a verified login.
"""

import argparse
import base64
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import re
import sys

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

PREFIX = b'dQw4w9WgXcQ:'
ENCRYPTED = re.compile(re.escape(PREFIX) + rb'([A-Za-z0-9+/=]{1,2048})(?![A-Za-z0-9+/=])')
MAX_FILE_BYTES = 32 * 1024 * 1024
MAX_TOTAL_BYTES = 64 * 1024 * 1024
MAX_FILES = 64


class ProbeError(Exception):
    """Fixed error codes only; source data must never reach an exception message."""


def unprotect_windows(value):
    """Ask Windows DPAPI under the current logon, with interactive UI forbidden."""
    if os.name != 'nt':
        raise ProbeError('windows-required')

    class Blob(ctypes.Structure):
        _fields_ = [('size', wintypes.DWORD), ('data', ctypes.POINTER(ctypes.c_ubyte))]

    crypt = ctypes.WinDLL('crypt32', use_last_error=True)
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    crypt.CryptUnprotectData.argtypes = [ctypes.POINTER(Blob), ctypes.c_void_p,
                                       ctypes.c_void_p, ctypes.c_void_p,
                                       ctypes.c_void_p, wintypes.DWORD,
                                       ctypes.POINTER(Blob)]
    crypt.CryptUnprotectData.restype = wintypes.BOOL
    kernel.LocalFree.argtypes = [ctypes.c_void_p]
    kernel.LocalFree.restype = ctypes.c_void_p
    buffer = (ctypes.c_ubyte * len(value)).from_buffer_copy(value)
    source = Blob(len(value), buffer)
    result = Blob()
    if not crypt.CryptUnprotectData(ctypes.byref(source), None, None, None, None,
                                   1, ctypes.byref(result)):
        raise ProbeError('dpapi-decryption-failed')
    try:
        return ctypes.string_at(result.data, result.size)
    finally:
        ctypes.memset(result.data, 0, result.size)
        kernel.LocalFree(ctypes.cast(result.data, ctypes.c_void_p))


def read_key(profile, unprotect):
    try:
        path = profile / 'Local State'
        with path.open('rb') as handle:
            content = handle.read(2 * 1024 * 1024 + 1)
        if len(content) > 2 * 1024 * 1024:
            raise ProbeError('local-state-too-large')
        wrapped = base64.b64decode(json.loads(content)['os_crypt']['encrypted_key'], validate=True)
    except (OSError, ValueError, KeyError, TypeError):
        raise ProbeError('local-state-unreadable') from None
    if not wrapped.startswith(b'DPAPI'):
        raise ProbeError('unsupported-key-wrapper')
    key = unprotect(wrapped[5:])
    if len(key) != 32:
        raise ProbeError('unexpected-key-size')
    return key


def account_id(token):
    """Recognize a session's structure locally; this does not authenticate it."""
    if not re.fullmatch(rb'[A-Za-z0-9_-]{20,64}\.[A-Za-z0-9_-]{1,32}\.[A-Za-z0-9_-]{20,128}', token):
        return None
    first = token.split(b'.', 1)[0]
    try:
        decoded = base64.b64decode(first + b'=' * (-len(first) % 4), altchars=b'-_', validate=True)
    except ValueError:
        return None
    if re.fullmatch(rb'[0-9]{15,22}', decoded):
        return decoded
    return None


def analyze(profile, unprotect, max_file_bytes=MAX_FILE_BYTES):
    cipher = AESGCM(read_key(profile, unprotect))
    report = dict(filesRead=0, filesSkipped=0, filesChangedDuringRead=0,
                  bytesRead=0, markerOccurrences=0,
                  encodedRecords=0, decryptedRecords=0, decryptionFailures=0,
                  unsupportedSchemes=0, rejectedPlaintexts=0,
                  uniqueSessionCandidates=0, distinctAccountIds=0,
                  authenticationChecked=False, discovery='raw-file-scan')
    sessions, accounts = set(), set()
    try:
        files = sorted((profile / 'Local Storage' / 'leveldb').iterdir())
    except OSError:
        raise ProbeError('local-storage-unreadable') from None
    for path in files:
        if path.suffix not in ('.ldb', '.log'):
            continue
        if report['filesRead'] >= MAX_FILES:
            report['filesSkipped'] += 1
            continue
        remaining = min(max_file_bytes, MAX_TOTAL_BYTES - report['bytesRead'])
        try:
            before = path.stat()
            if not path.is_file() or before.st_size > remaining:
                report['filesSkipped'] += 1
                continue
            with path.open('rb') as handle:
                data = handle.read(remaining + 1)
                after = os.fstat(handle.fileno())
            if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
                report['filesChangedDuringRead'] += 1
                report['filesSkipped'] += 1
                continue
            if len(data) > remaining:
                report['filesSkipped'] += 1
                continue
        except OSError:
            report['filesSkipped'] += 1
            continue
        report['filesRead'] += 1
        report['bytesRead'] += len(data)
        report['markerOccurrences'] += data.count(PREFIX)
        for match in ENCRYPTED.finditer(data):
            try:
                blob = base64.b64decode(match.group(1), validate=True)
            except ValueError:
                continue
            report['encodedRecords'] += 1
            if blob[:3] != b'v10':
                report['unsupportedSchemes'] += 1
                continue
            if len(blob) < 3 + 12 + 16:
                report['decryptionFailures'] += 1
                continue
            try:
                token = cipher.decrypt(blob[3:15], blob[15:], None)
            except (InvalidTag, ValueError):
                report['decryptionFailures'] += 1
                continue
            report['decryptedRecords'] += 1
            identity = account_id(token)
            if identity is None:
                report['rejectedPlaintexts'] += 1
                continue
            sessions.add(token)
            accounts.add(identity)
    report['uniqueSessionCandidates'] = len(sessions)
    report['distinctAccountIds'] = len(accounts)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--live', action='store_true', help='Read this Windows user\'s vanilla Discord profile')
    args = parser.parse_args()
    if not args.live:
        parser.error('Use --live to explicitly read the local Discord profile.')
    if os.name != 'nt':
        print(json.dumps({'error': 'windows-required'}))
        return 1
    try:
        profile = Path(os.environ['APPDATA']) / 'discord'
        report = analyze(profile, unprotect_windows)
        print(json.dumps(report, sort_keys=True))
        return 0
    except ProbeError as error:
        print(json.dumps({'error': str(error)}))
        return 1
    except Exception:
        # No traceback or exception text: source data can contain credentials.
        print(json.dumps({'error': 'unexpected-probe-failure'}))
        return 1


if __name__ == '__main__':
    sys.exit(main())
