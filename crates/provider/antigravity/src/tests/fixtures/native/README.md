# Native AGY replay fixtures

These stdout captures come from [Paseo](https://github.com/getpaseo/paseo/tree/dc07efe937c3160e8fe83d02dc972c8a0b014957/plugins/antigravity-provider/test/fixtures), revision `dc07efe937c3160e8fe83d02dc972c8a0b014957` (latest main checked 2026-10-27).

The `.stdout.ndjson` files are copied unchanged with shorter filenames. They contain already-redacted AGY 1.2.13 frames wrapped in `{t_ms, stream, data}`. The upstream README documents capture conditions and limitations. These fixtures supplement Ait's synthetic AGY 1.3.0 cases, especially `ERROR` tool states, `ERROR` / `interrupted` results, cumulative denials, and subagent metadata.

Paseo is Apache-2.0 licensed, copyright (c) 2025-present Mohamed Boudra; its license and upstream attribution are retained in `paseo/LICENSE` at the repository root. No local conversations or credentials are included.
