# Legacy SQLite history fixture

`history-rusqlite-0.32.db` was created with rusqlite 0.32.1 and its bundled
SQLite 3.46.0. It contains synthetic data only: history ID 41 with a Unicode
title, completed audio/transcript deletion intents, a pending job deletion,
and `user_version=0`. There are no credentials or user files.

The history regression test copies it to a temporary directory before opening
it. It checks schema migration, metadata, autoincrement, transaction rollback,
and persistence after reopening with the patched bundled SQLite.

SHA-256: `d40ef63d8ef79e22f6284710f9cce03058ae636f800cc287741dc366461494bb`.
