# Native AGY replay fixtures

These stdout captures come from [Paseo](https://github.com/getpaseo/paseo/tree/dc07efe937c3160e8fe83d02dc972c8a0b014957/plugins/antigravity-provider/test/fixtures), revision `dc07efe937c3160e8fe83d02dc972c8a0b014957` (latest main checked 2026-10-27).

The `.stdout.ndjson` files are copied unchanged with shorter filenames. They contain already-redacted AGY 1.2.13 frames wrapped in `{t_ms, stream, data}`. The upstream README documents capture conditions and limitations. These fixtures supplement Ait's synthetic AGY 1.3.0 cases, especially `ERROR` tool states, `ERROR` / `interrupted` results, cumulative denials, and subagent metadata.

Paseo is Apache-2.0 licensed, copyright (c) 2025-present Mohamed Boudra; its license and upstream attribution are retained in `paseo/LICENSE` at the repository root. No local conversations or credentials are included.

## AGY 1.5.16 compatibility captures

`schedule-invalid-1.5.16.ndjson` and `command-denied-1.5.16.ndjson` were captured
on 2026-11-24 with the official Linux x64 AGY 1.5.16 binary. Conversation IDs and the temporary
working directory are replaced with fixed placeholders; the remaining stdout fields are unchanged.
These are compatibility samples, not reproductions of the reported user failure.

The schedule capture uses plan mode and the invalid cron expression `INVALID`, so no task is created.
The command capture uses session-only `permissions.deny: ["command(*)"]`, then follows up with a
text-only turn. Both captures use `gemini-3.8-flash` with low effort and stream JSON input/output.
They cover native tool errors, successful responses after tool errors, and cache counters larger than
uncached input counters. They contain no original user conversation or native stderr/log payloads.
