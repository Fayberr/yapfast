# feat(discord): import saved accounts from the desktop client

Windows users who already signed in to the Discord desktop app can import its
saved sessions from Yapfast's sign-in screen or Manage Accounts > Add Account.
Yapfast validates sessions before saving them, deduplicates authenticated user
IDs, preserves the active account and reports partial failures. Failed imports
can be retried, and cancelling refreshes accounts already saved.
Already linked accounts keep their existing tokens. Validation runs four
requests at a time with one build-number lookup per batch. Only HTTP 401 is
treated as signed out; network, service, timeout, profile and storage failures
have specific messages. The summary survives starting the first connection,
and the QR button explicitly says "Show the code" after an import failure.

This branch is rebased onto `upstream/main` and retains the multi-account
prerequisite commits from #209, which are not yet on main. The import commits
sit on top of that stack.

Discovery reads the current Windows user's Stable, PTB and Canary profiles.
It streams a bounded copy into memory, retries concurrent file changes and
leaves the original files read only. Recovery is best effort: rusty-leveldb
4.0.1 silently ends replay of a log at a bad record, so later updates or
deletions may be missed. QR login remains available.

## Dependencies

- `rusty-leveldb` 4.0.1 reads LevelDB tables and write-ahead logs, including
  fragmented records and Snappy compression. Its filesystem feature is
  disabled; recovery and any writes operate only on the in-memory copy.
- `crc32c` 0.6.9 supplies the CRC-32C operations used by the LevelDB format.
- `integer-encoding` 3.0.4 decodes LevelDB's integer encodings.
- `snap` 1.1.2 decodes Snappy-compressed table blocks.

The latter three are new transitive crates of `rusty-leveldb`. Its other
dependencies (`bytes`, `rand` and the build helper `rustc_version`) were already
locked in the workspace. The original import commit also adds the missing
`windows-sys` lockfile edge for its DPAPI dependency.

## Validation

On the Linux VPS, 45 focused source-module tests and strict Clippy pass. The
suite covers storage fixtures, retries, error-chain logging without candidate
tokens, validation and save failures, identity deduplication, summary wording
and preserving independent sessions, plus bounded concurrent validation and
cancellation. The import modules also pass an x86_64 Windows-target
typecheck locally, without running Windows programs.

Full application, UI and native Windows execution remain separate pending
gates. The focused harness does not compile the application state machine or
Slint UI. No complete inactive-account discovery is claimed from fixtures.
