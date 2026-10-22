# ADR-184: Pi native reasoning controls and defaults

- Status: Proposed (implemented, under review).
- Date: 2026-10-23.
- Scope: `crates/provider` Pi adapter and shared forms, controls, and agent facts in `apps/mobile`.
- Revises: [ADR-156](adr-156-pi-native-rpc-provider.md) and the unsupported-thinking
  statement in [ADR-174](adr-174-pi-session-import.md).
- Follows: [ADR-034](adr-034-agent-config-session-events.md) for requested configuration
  and runtime facts, [ADR-179](adr-179-provider-owned-tests.md) for adapter tests, and
  [ADR-183](../clients/adr-183-shared-reasoning-control-selection.md) for shared controls.

## Context

Pi exposes model reasoning capabilities, a native thinking level, and a command to change
that level. Rejecting every host thinking selection prevents Ait from using these controls.
Guessing a level from catalog order would override Pi settings. Native clamping and
extension-selected levels also mean a saved host request can differ from effective state.

Pi's `set_model` resets thinking to the destination model's saved/default level. A model-only
update must therefore preserve the effective level explicitly; replaying an unchanged host
request would overwrite a later extension change or a previously clamped native value.

## Decision

### Catalog and form defaults

- Discover `get_available_models` and `get_state` in the same ephemeral `--no-session`
  process. Each uses the existing control deadline. State-read errors fail discovery;
  a slower successful read must not silently change defaults persisted by forms.
- Models with `reasoning: true` expose `off`, `minimal`, `low`, `medium`, and `high`
  unless a null `thinkingLevelMap` entry disables a level. `xhigh` and `max` require
  explicit non-null mappings. Additional non-null map keys are opaque selectable IDs
  with their ID as the fallback label. Non-reasoning models expose no thinking choices.
- Mark the current native model as the catalog default when its identity matches.
  Advertise its native thinking level as the default only when that level is offered.
  Other models, missing state, and unsupported defaults leave the thinking default unset.
- Pi profile and agent forms preserve an unspecified thinking selection when no concrete
  default is advertised. Other providers retain their first-option fallback. Normalize
  option-level default markers before resolving a concrete form default.
- Selection validation requests only the model catalog. An explicit model must exist;
  reasoning IDs are validated as nonempty, control-free strings of at most 512 UTF-8 bytes.
  Pi owns level validation and clamping, including future IDs and stale/imported selections.

### Native configuration and resume

- On creation and interactive resume, apply an explicit model before an explicit thinking
  selection. Use `set_thinking_level` and read back `get_state` to obtain effective state.
  Unspecified startup/resume thinking preserves the selected model's native defaults.
- Later configuration updates apply only changed model/thinking fields. An unchanged
  host thinking request does not overwrite an extension change on the next prompt;
  mode-only updates send no thinking control.
- Before a model-only update, synchronize current native state, capture its string thinking
  level, switch models, restore the captured level, and read back Pi's clamped result.
  Missing/non-string native thinking leaves the destination default intact.
- When model and thinking change together, the new thinking request takes precedence.
  Clearing a previous explicit selection requests `medium` because Pi has no unset RPC.
  Pi may clamp that baseline. Clearing does not modify native global defaults.

### Runtime facts and ownership

- Publish effective state as `runtimeInfo.extra.thinkingLevel` and keep
  `runtimeInfo.thinkingOptionId` unset. Directory filtering continues to use the latest
  persisted host request, including pending changes and unspecified/default selections.
- Observe `thinking_level_changed` while idle, during admission, and during turns.
  Coalesce changes through the existing runtime-info event. No additional timeline event
  or protocol schema is introduced.
- Unknown string metadata is preserved. Missing/non-string state leaves the runtime level
  unset; change events without a string level are ignored. Optional metadata variations
  do not prevent sessions or turns from running.
- Pi continues to own credentials, extensions, native settings, and session files.
  The adapter and its behavior tests remain in `crates/provider`; no daemon test owner
  or new public port is introduced. Client controls display requested selections using
  ADR-183; native metadata remains a separate runtime fact.

### Client display

- The composer trigger and dropdown checkmark display the requested selection/catalog
  default, including when it is pending or Pi has clamped it.
- Ait-managed child-agent rows and their compact-layout facts sheets display Pi's effective
  `extra.thinkingLevel`. Missing metadata falls back to the requested selection/catalog
  default; present malformed metadata displays "Unknown". Unknown string IDs use their
  fallback label. Regular agent tabs use the composer controls without this facts line.
- The runtime projection does not enumerate Pi-native extension subagents. The facts
  line applies to child agents whose parent relationship is managed by Ait.

## Consequences

Pi reasoning can be selected through existing controls, and stale thinking choices remain
admissible for native resolution on restore. Discovery now depends on a successful state
read. An unknown thinking default can vary with the host's native settings, so forms
preserve that absence rather than promising a portable concrete level.

Model-only updates add a state synchronization and thinking restoration when native state
reports a level. Runtime metadata exposes clamping and extension changes without changing
the identity of the saved request. Unsupported explicit controls still fail through the
existing bounded transport and error contract.

## Validation

Fixture regressions cover catalog defaults and failure paths, future IDs, clamping,
creation/restore/import, clearing overrides, extension events, and model-only updates.
Fixtures reproduce `set_model` resetting thinking with distinct model defaults.
Opt-in installed-Pi tests use isolated settings and a loopback model service to check
native controls, model switches, and outgoing `reasoning_effort` values.
UI tests cover Pi defaults and other providers' existing fallback behavior.
Facts-line and helper tests distinguish effective native levels from requested selections,
including missing, malformed, future, and other-provider metadata.

Operational behavior: [Pi](../../operations/pi.md).
Results and scope: [reasoning validation](../../reports/providers/pi-reasoning-controls-2026-10-23.md).
