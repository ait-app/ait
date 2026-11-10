# OMP native interface compatibility

Ait supports the public interfaces of official OMP 18.14.0, pinned to
[`v18.14.0` / `31147f9d`](https://github.com/can1357/oh-my-pi/tree/31147f9d1a76608549821c3759f8487b4d66c72d).
Installation and builds do not modify OMP.

Native editor completion is used only after `commandCompletion.version = 1` is
advertised. Otherwise Ait keeps the command catalog fallback. Arbitrary custom
TUI factories remain outside the official RPC surface.

The unshipped completion proposal is isolated from this delivery. Its original
[patch and license](https://github.com/ait-app/ait/tree/1628ace3/third-party/omp)
remain available as historical design material; they are not part of current
installation or builds. An independent local proposal commit is `82ba8c84`
(`proposal/native-provider-interfaces-20261104`).

See [ADR-234](../../docs/decisions/providers/adr-234-native-editor-completion.md)
for the negotiated Ait integration boundary.
