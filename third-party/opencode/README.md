# OpenCode native command compatibility

Ait targets official OpenCode 2.1.3, pinned to
[`bb25a2f4`](https://github.com/anomalyco/opencode/tree/bb25a2f4a3f3f04d7c344bec0c0574ea3510b3b1).
Installation and builds do not modify OpenCode.

This version acknowledges registered commands before their native execution has
settled. Ait therefore guards registered commands unless ACP advertises
`agentCapabilities._meta["opencode/command-lifecycle"] = 1`. Absolute paths,
unknown slash-prefixed text and `/compact` retain their normal native paths.

The unshipped lifecycle proposal is isolated from this delivery. Its original
[patch and license](https://github.com/ait-app/ait/tree/1628ace3/third-party/opencode)
remain available as historical design material; an independent local proposal
commit is `82ba8c84` (`proposal/native-provider-interfaces-20261104`).
Capability negotiation and fallback behavior are delivered by Ait; the proposal
is not a released upstream capability. See the
[adaptation report](../../docs/reports/providers/dsh-opencode-capability-adaptation-2026-10-30.md).
