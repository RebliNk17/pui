# LiteLLM discovery, capabilities and Auto classifier investigation

## API contract research

Inspected this gateway's live [OpenAPI specification](https://llm.haip.eu-west-1-kaas-dev-1.platform-dev.e.chainalysis.com/openapi.json), reporting LiteLLM API 1.109.1, and fetched its public hub without credentials. Local research copies: `/tmp/pui-litellm-openapi.json`, `/tmp/pui-litellm-reasoning-schema.json`, `/tmp/pui-litellm-public-hub-live.json`.

| Endpoint | Appropriate use |
| --- | --- |
| `GET /v1/models` | OpenAI-compatible availability/model-ID list. Its description explicitly directs detailed metadata consumers to model info. The user's response has 74 models with input/output token limits. |
| `GET /model_group/info` | Preferred authenticated metadata enrichment. Its description explicitly recommends this endpoint for end users, because callers invoke public model groups rather than individual deployments. Response: `{ "data": [{ "model_group": ..., ... }] }`. |
| `GET /public/model_hub` | Unauthenticated fallback. Contains only published groups (57 in supplied attachment and live observation), so cannot replace the availability list. Response: array of `ModelGroupInfoProxy`. |
| `GET /v1/model/info` | Deployment-oriented configuration details, not the preferred primary list of user-callable aliases. |
| `GET /v2/model/info` | Paginated deployment metadata, default page size 50; not interchangeable with a complete model-group list. |
| `GET /public/v1/model_hub` | Newer paginated/searchable public hub; unnecessary when using the simpler full public array as fallback. Its prose says unauthenticated despite a security annotation, so the contract has inconsistencies. |

Unauthenticated `/model_group/info` returned HTTP 401; no credential was read or sent for research. The authenticated path is implemented and mock-tested, but not live-tested with the user's key.

### Important semantics

- `ModelGroupInfoProxy.max_input_tokens` / `max_output_tokens` are nullable **numbers**. Live values include `1048576.0`; integral floating-point JSON values must be accepted, not just integer JSON representations.
- `supports_reasoning` / `supports_vision` are optional booleans with schema defaults of **false**. A serialized false cannot establish whether metadata was explicitly negative or merely defaulted/unknown.
- Therefore group/hub enrichment imports positive capability evidence only; it never erases explicit `/models` flags (including nested/alternate fields) or existing family inference. Unknown/null metadata never erases limits or availability.
- `supported_reasoning_efforts`, `supported_openai_params`, and `supported_endpoints` should guide future compatibility work; a capability badge alone does not establish the correct wire parameter.
- `glm-5.2` advertises reasoning support but no effort list, and its supported parameter list omits `reasoning_effort` and `thinking`. Do not inject generic GLM/Z.ai thinking controls based only on the model name.
- Some aliases explicitly advertise `/v1/responses` rather than chat completions. This change does not migrate model API routing or claim all chat/responses backends are interchangeable.
- Prices are present per token in group metadata. This change does not import prices or infer free usage from the existing zero costs.

## Source changes

- Discovery parses LiteLLM `max_input_tokens` / `max_output_tokens`, nested `model_info`, `model_name`, and `model_group` identities. Positive, integral, finite values representable by u32 are preserved without the previous clamps. Invalid/missing values retain defaults.
- Explicit reasoning/vision metadata takes priority over updated family fallbacks (including modern Claude, GPT, GLM, Kimi, MiniMax and DeepSeek names).
- `scripts/discover-models.mjs` keeps `/models` authoritative for availability, enriches matching IDs from authenticated same-origin `/model_group/info`, then falls back to the unauthenticated public hub. Metadata requests reject redirects, use short deadlines, and never send the API key to the public endpoint. Missing metadata does not break discovery.
- Auto classifier ceiling increases from 256 to `min(2048, model.maxTokens)`. Existing strict complete-response/JSON validation, cancellation, 12-second per-candidate deadline, fallback scope and fail-closed behavior are preserved.

## Auto failure diagnosis

The reported `incomplete response` maps specifically to `stopReason: "length"`, which the installed SDK maps from upstream `finish_reason: "length"`. It was not a classifier denial. The prior 256-token budget can be consumed by server-default reasoning before decision JSON finishes. Installed SDK comments explicitly document the shared reasoning/answer ceiling. Omitted reasoning or `reasoning: "off"` do not reliably disable thinking on a generic gateway.

The larger budget is a bounded mitigation, not a live-verified backend cure. Worst-case output/cost ceiling rises up to eightfold, and thinking can still exceed 2048 tokens or the deadline. No live classifier/model requests were made for investigation. Independent read-only review found no remaining concrete blocker after an explicit capability-precedence regression was fixed.

## Installed configuration repair

Updated only the managed `~/Library/Application Support/pui/pi-agent/models.json`; no credentials were printed or modified.

- Corrected 69 existing models from the user's first attachment; verified 135 reported limits, including corresponding existing context overrides.
- Updated 23 models' reasoning/vision from unambiguous installed SDK catalog matches; mapped known mantle/runtime aliases to base catalog IDs.
- Public hub established positive reasoning/vision for `standard` and `fast`.
- Reversed the temporary downgrades caused by treating default-false hub metadata as authoritative; retained previously known DeepSeek reasoning and alias vision.
- Specialized/unverified aliases remain otherwise unchanged. No models were added or removed.
- Backups beside the config include `models.json.before-litellm-limits-20261124-165515.bak`, `models.json.before-capabilities-1795532204054.bak`, `models.json.before-model-hub-20261124-165842.bak`, and `models.json.before-default-false-repair-20261124-171302.bak`.

## Validation history

Environment: stable aarch64 Rust toolchain, explicit Command Line Tools SDKROOT, pinned cached CEF distribution, `CARGO_TARGET_DIR=/tmp/pui-btw-clean-target`, two build jobs. Full JS invocation also exports the installed 1.5.2 SDK.

- Focused provider tests: **11 passed**; `/tmp/pui-litellm-final-focused-rust.log`.
- Pre-integration full JS suite: **184 passed, 1 existing skipped, zero failures**; `/tmp/pui-litellm-final-js.log`. Covers discovery merge/fallback/default-false and nested capability precedence, classifier budgeting and fail-closed behavior.
- Formatting and whitespace checks: passed.
- Earlier complete Rust runs passed before the last API-contract-driven refinements. These are not claimed as final-tree acceptance.
- Earlier complete `./scripts/test-rust.sh` attempt: **2113 bulk passed, 11 failed, 2 ignored**. Failures concerned existing Git conflict/identity/branch/fetch/fast-forward inspection deadlines and associated assertions. The runner stopped before serial groups; that run was **not green**. Log `/tmp/pui-litellm-final-rust.log`. Concurrent external builds used the same target during validation; this is observed contention, not proven failure attribution. No production deadlines/assertions or runner groups were weakened. The separate integrated validation below used serial scheduling rather than retrying individual failures.
- An initial full JS attempt lacked CEF configuration for the streaming test and failed on a certificate error; the final correctly configured full JS run above passed.
- Initial shell lacked cargo in PATH; resolved the existing stable toolchain without installation.

## Master integration and publishing validation

After the user requested commit and push, fetched origin and fast-forwarded local master from `d661105b` to `5f30253f` (four upstream side-chat commits), preserving the six feature files without conflicts. Inspected the final diff; no unrelated edits are included.

Final validation on the integrated tree:

- `cargo fmt --all -- --check`: **passed**.
- `cargo clippy --offline --locked --all-targets -- -D warnings`: **passed**; `/tmp/pui-litellm-push-clippy.log`.
- `PUI_TEST_THREADS=1 ./scripts/test-rust.sh`: **2350 passed, zero failures, 2 existing ignored across 24 result groups**, including every serial group; `/tmp/pui-litellm-push-rust.log`. Used the resolved Command Line Tools Git/compiler and existing pinned CEF, stable Rust and target directory. No assertions, deadlines, skips or runner logic changed.
- `node --test tests/*.test.mjs` with the same Rust/CEF environment and installed SDK: **184 passed, zero failures, 1 existing skipped**; `/tmp/pui-litellm-push-js.log`.
- `git diff --check`: **passed**.

The installed metadata repair was applied and verified independently; new discovery/classifier code still requires a future build/install. No application package or deployment is part of this commit/push.
