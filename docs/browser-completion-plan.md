# Browser completion implementation plan

Starting point: explicitly pulled latest master `7a8b47c8b` (1.0.1). Current source includes redesigned browser pane masking, inline CEF visuals, composer toolbar and fixes for earlier palette/SVG/workflow baseline failures; old validation failures are historical, not assumed current.

## Coordinated scope

1. Inspection: explicit same-origin frame inventory/scope, one selected open shadow root, bounded safe descendant text and generated unique-at-snapshot selector. No automatic cross-origin reads, form values or page actions; unsupported states refuse.
2. Element context: explicit Review for chat → Add to draft, frozen root/session/draft/attachment revision and page/epoch/facts. Whole-or-reject bounded metadata and no automatic send.
3. Screenshot: explicit capture disclosure, bounded current viewport pixels, frozen crop/arrow/rectangle annotations, Review → draft image with stale guards. Decode/render workers and shared preview memory budget. No automatic export/upload.
4. Navigation/privacy: bounded per-owner local visited history; explicit navigation/clear; named ephemeral in-memory context groups and confirmed switch/clear with no automatic reload/network. Existing no-Keychain/no-persistent-cookies policy retained. Persistent profiles are not introduced.
5. UI: preserve existing two toolbar tool slots (Inspect plus More), group screenshot/history/privacy/DevTools in More; scope details collapsed/compact, one active utility at a time, existing CEF view mask used for native overlap safety. Narrow/zoom/theme/cancel/disabled tests and real local runtime acceptance where authorized.

## Validation and delivery

Workers own separate screenshot/history/reader subsystems; parent owns shared UI, draft handoff, review, integration and final tests. Focused tests while implementing; full `scripts/test-rust.sh`, strict Clippy/fmt/diff, JS/Python at completion. Dedicated targets with correct compiled-source identity; never infer source from a stale shared binary. All failures retained, no assertion/deadline weakening or silent retries. Physical pointer/OS accessibility/live-site verification cannot be fabricated by deterministic fixtures.

Local commits only when validated per standing instructions. Prior push/runtime test authorizations do not authorize a fresh install/restart/push/release; ask for any new runtime bundle if necessary. Original checkout/user sessions untouched. No application profiling until development finished. No claim that all remaining work is complete merely from tests passing; record actual finished scope and outstanding acceptance gates.
