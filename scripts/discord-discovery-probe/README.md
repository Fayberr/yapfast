# Discord account discovery prototype

Independent Windows storage experiment for Yapfast. It reads vanilla Discord's
profile under the current user's `%APPDATA%`, unwraps its encryption key with
Windows DPAPI, and decrypts recognized session records in memory. The only
output is a JSON summary containing counts and fixed error codes.

It never exports a token, prints account identifiers, contacts Discord, changes
Discord's files, closes Discord, or adds an account to Yapfast. The source uses
Python's `cryptography` package, already present on the tested WinPC; no package
installation was needed. The Windows API wrapper uses only the standard library.

## Run

Use Windows Python 3 and PowerShell 7 (`pwsh`), under the Windows user whose
Discord profile should be checked:

```pwsh
python scripts/discord-discovery-probe/probe.py --live
```

Without `--live`, the program refuses to read the profile. It only considers
vanilla Stable's profile, does not recurse into other profiles or browsers, and
limits reads to 32 MiB per file and 64 MiB total. Read errors and changing files
are skipped and counted. Secrets exist temporarily in Python memory; this is
not a hardened secret-memory implementation and does not guarantee erasure of
every Python or cryptography allocation.

## Verification

```sh
python3 -m unittest discover -s scripts/discord-discovery-probe -v
```

The synthetic tests cover encryption round trips, duplicate records, corrupt
authentication tags, unrelated plaintext, unsupported formats, wrong keys,
read bounds, unchanged fixture files, and summary output without credentials.
No live credential is used in the tests.

Live run on Fabian's WinPC, 2026-11-10:

- Windows user `derfa`, PowerShell 7.6.9, Python 3.13.12.
- Vanilla Discord 1.0.9267 was already running and remained open.
- The DPAPI key was readable through the SSH session as the same Windows user.
- The first run read 5 storage files, 7,288,138 bytes.
- Found 2 encrypted records; both decrypted with AES-256-GCM, with no failures.
- Recovered 2 distinct structurally recognized session candidates for 1 account.
- All 11 synthetic tests passed on both Linux and Windows, including a later
  regression test for a file changing during its read.
- No credential, encryption key, or account identifier was printed or saved.
- No authenticated API request or Yapfast login was attempted.
- A second run reproduced the same two candidates for one account, with no
  skipped files or detected file changes during reading.

## What this proves and what remains

Reading and decrypting this actual installation's saved session records works
without changing Discord or requiring a phone login. It does **not** prove that
either recovered session is still accepted by Discord.

The first-stage discovery scans raw `.ldb` and `.log` files for known encrypted
records. It can find historical records and miss compressed or fragmented ones;
it does not reconstruct LevelDB's current logical state, sequence numbers,
deletions, or compaction manifest. A running Discord can compact files during a
scan, so a zero result is not conclusive. Retrying or reading a consistent copy
would be separate work.

Before porting this into a client:

1. Validate candidates through the client's normal account API flow and
   deduplicate by the authenticated account ID. A locally decoded ID is not
   authentication. Do not automatically import every historical record.
2. Test inactive account-switcher entries and revoked sessions; neither was
   established by the live run.
3. Decide whether to read LevelDB logically for complete coverage. A raw scan
   is sufficient for this proof, not a promise of finding every account.
4. Add selected accounts to Yapfast's credential store without changing the
   active account during the discovery step. Retain QR linking as a fallback.

This branch changes only this standalone prototype. No Yapfast UI, Rust source,
build configuration, or dependencies were changed.
