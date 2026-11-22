# Workflows: from a verified-task pipeline to a durable development system

**Status:** architecture audit and proposed roadmap; no runtime changes implemented by this document.

**Inspected baseline:** `master` at `040d72be`, after a fast-forward pull. This is a repository-based assessment, not a claim of superiority over other products or a provider-backed quality evaluation.

## Recommendation in one minute

Pui has valuable foundations: scoped native consent, verified dirty-working-copy isolation, host-run checks, a fresh reviewer, bounded repair, retained evidence, and explicit apply-back. Keep them.

Its main limitation is structural. Today, a workflow is a fixed, window-owned task pipeline that occupies the parent tool call. Making prompts longer or launching more agents will not turn that into an excellent workflow system.

Build **one durable, app-owned execution core**, with the existing verified-change pipeline as its first recipe. Make chat responsive; record recoverable stage boundaries; give builders permissioned fast test feedback; make evidence and application status explicit; then add selective parallelism and reusable recipes.

The product promise should be:

> Describe the outcome. Review the plan when needed. Keep using your chat while Pui does the work. See what is happening, change direction safely, and receive a tested, reviewable result—with clear next actions when something blocks.

Success is useful, safely delivered work with fewer interventions—not more agents, stages, tokens, or green badges.

## 1. What already deserves to survive

- **Consent is real authority, not model output.** Host approval is objective-bound, supports concrete plans and revisions, and revalidates context. Plan → Manual is an explicit conversation-scoped choice. `src/app/tasks/consent.rs:25–328`.
- **Isolation includes actual working state.** Git preparation copies staged/unstaged/nonignored untracked state without committing or stashing, and checks before/after digests. Unsupported states fail closed. `src/tasks/workspace.rs:89–159`; `src/tasks/workspace/snapshot.rs`.
- **Checks are not builder prose.** The host runs exact commands through permissions, records exit status, and binds evidence to source fingerprints. `src/app/tasks/runtime.rs:583–660, 811–856`.
- **Independent review is a fresh context.** It receives the objective, contract, candidate edits and check results rather than the builder's explanation. Failure/blocked findings cannot become success through positive prose. `src/app/tasks/runtime.rs:531–548`; `src/tasks.rs:213–239`.
- **Uncertainty is not silently replayed.** Interrupted work and unknown shell effects do not automatically retry. Recovery, setup and application use separate scoped authority. `src/tasks.rs:358–392`; `src/app/tasks/recovery.rs`; `src/app/tasks/setup.rs`.
- **Apply-back already exists.** It has baseline receipts, conflict guards, journals and conditional rollback; it is not an atomic multi-file transaction or an automatic merge. Improve this machinery rather than inventing an unsafe replacement. `src/tasks/apply.rs`; `src/tasks/apply_transaction.rs`; `src/app/tasks/changes_preview.rs`.
- **Native UI conventions are sound.** Keep Quiet Mac framing, inline agent questions/permissions, independently inspectable activity, stable selection, and no navigation-as-approval shortcuts.

## 2. Findings, ranked by effect on real work

“Confirmed” below means observed in source or a bounded reproduction. It does not imply every issue has been exercised through the installed application.

| Priority | Finding and evidence | Consequence | Recommended direction |
| --- | --- | --- | --- |
| P0 | **Missing-module text can incorrectly stop repair.** `failed_requirement` checks broad substrings in any failed output, independent of command or error provenance (`src/app/tasks/environment.rs:199–227`). The runtime immediately blocks on that classification (`src/app/tasks/runtime.rs:844–855`). An assertion containing “cannot find module”, or `Cannot find module './new-component'`, both reproduce the classification. | Builder-caused bugs can be labeled setup problems and never reach the reviewer/repair loop. | Structured failure categories grounded in command, phase, baseline and diagnostics. Relative project imports are not automatically missing dependencies. Unknown is not permission to install, retry, or declare an environment failure. |
| P0 | **A malformed task record aborts the whole history load.** Deserialization and checkpoint validation propagate out of the record loop (`src/tasks/store.rs:174–210`). The first 1,000 directory entries are selected before sorting. | One bad record can make otherwise valid workflows unavailable; a large history can omit recent work nondeterministically. | Per-record quarantine with visible diagnostics; versioned migrations; deterministic indexed/paginated listing. Preserve corrupt bytes for explicit inspection. Never resume a quarantined record. |
| P1 | **The parent tool call owns the entire wait.** `pui_workflow` is sequential and waits inside `ctx.ui.input` until completion; the host replies at terminal state (`scripts/workflows.ts:8–9, 44–56`; `src/app/tasks/consent.rs:361–380`). | The owning agent cannot carry on a normal conversation while coordinating the run. Event-driven waiting saves polling, but is not background execution. | Return a durable run handle after consent and persisted creation. Provide explicit await, status, steering, pause and cancel operations, with deduplicated result delivery. |
| P1 | **Execution is a window singleton and state progression is tied to the app controller.** `Tasks.live`, `job` and `processes` are window-wide (`src/app/tasks.rs:133–180`); consent rejects another active workflow (`src/app/tasks/consent.rs:92–98`). | Independent projects contend for a window slot; execution ownership, visible chat and presentation are harder to separate. | Extract an app-owned runtime independent of the selected chat/view. Keep single-workflow execution initially; add a global scheduler only after resource ownership is explicit. A daemon is a separate later decision. |
| P1 | **Preparation has no matching subprocess deadline.** Workspace inspection/preparation uses blocking `Command::output()` without a timeout (`src/tasks/workspace.rs:14–28, 111–130`); `task_job` has no deadline (`src/app/tasks/runtime.rs:36–47`). The 30-minute runtime check covers pending RPC work (`:683–693`), not these filesystem jobs. | A stalled Git/filesystem operation can outlive the advertised operation deadline. Stop drops the receiver but cannot cancel the running preparation thread. | Use a shared bounded process runner with output limits, process ownership, deadlines and cancellation receipts. Preserve any resulting worktree; cancellation must not imply that already-started preparation was undone. |
| P1 | **Retained history is not durable continuation.** Restart marks active phases Interrupted (`src/tasks.rs:358–392`). Recovery supports an unchanged captured candidate or initial implementation proven never to have started (`src/app/tasks/recovery.rs:169–287`). User “checkpoints” are explicitly notes (`src/tasks/checkpoints.rs:1–10`). | A restart can strand useful work; users must reconstruct intent even when some completed boundaries could safely be retained. | Durable step receipts and explicit continuation from safe boundaries. Reconcile in-flight effects; never replay unknown operations. Keep notes separate from executable checkpoints. |
| P1 | **Builders cannot get execution feedback while building.** They have file tools and questions only; bash, codemode and delegation are blocked (`src/app/tasks/runtime.rs:427–445, 551–580`; `scripts/ultracode.ts:110–120`). Checks start after the builder settles. | Simple compile/test issues cost a whole implementation/check/review/repair cycle. No red-before/green-after baseline is collected automatically. | A host-mediated `run_check(check_id)` capability for focused feedback, under existing permissions and resource locks. Final acceptance checks and independent review remain separate. Do not solve this with unrestricted shell access. |
| P1 | **Routing policy is hard-coded to a narrow catalog.** Family classification names specific Opus/Sol IDs; dynamic routing requires a same-provider Haiku. Unknown and zero-price entries are excluded, including pinned policy (`src/tasks/routing.rs:100–136, 199–243`). | Otherwise usable provider/model configurations cannot start workflows; catalog evolution becomes a source-code dependency. Per-stage routing also adds latency and another failure surface. | Explicit eligible model capabilities and user-approved profiles. Default to a confirmed selected model when eligible; offer opt-in adaptive routing. Price unknown is a disclosed policy decision, not an invented estimate. Never silently substitute or expand spending authority. |
| P1 | **Environment handling is safe but expensive in user attention.** Ignored dependencies are deliberately absent. Common missing dependencies block; setup and continuation need separate scoped actions (`src/app/tasks/environment.rs:26–125`; `src/app/tasks/setup.rs`; `src/app/tasks/recovery.rs:257–319`). | A routine task can become a setup → inspect → resume exercise before useful work starts. | Repository execution profiles with exact prerequisite/setup/check definitions, keyed to lockfiles, tools and relevant configuration. Review setup risk once per precise scope; reapproval on drift. Never infer approval for network, lifecycle scripts, credentials or shared dependency symlinks. |
| P1 | **Verified does not guarantee deliverable.** Apply-back rejects additions under new directories (`src/tasks/apply.rs:509–511`), non-ASCII apply paths (`:186–190`), changed binary content and mode changes (`:498–507`), and has smaller inventory limits than fingerprinting. | A valid-looking candidate can require manual export/integration at the finish line. | Surface delivery capability before implementation. First support journaled creation of ordinary parent directories; then handle other file kinds deliberately or provide an honest reviewed export. Distinguish “verified candidate” from “ready to apply”. |
| P1 | **Retained agent history exhausts execution capacity.** Spawn rejects when `agents.nodes.len() >= 128`, counting retained nodes (`src/app/agents.rs:877–879`). Stopping is not removal; deletion has a separate lifecycle (`:297–306`). This audit's managed reviewers were refused by that exact limit. | Ordinary history accumulation prevents new work. “Finish this workflow” does not explain how capacity is recovered. | Separate active-process/queue limits from retained history. Lazy-load dormant transcripts, bound memory, and expose non-destructive archival and capacity details. Never delete chats/worktrees automatically to make room. |
| P2 | **The plan is prose, not tracked execution.** `ExecutionPlan.steps` is a list of strings; runtime state is the fixed Role/Phase set (`src/tasks/plan.rs:6–8`; `src/tasks.rs:22–90`). | Plans cannot report per-step completion, dependencies, ownership, outputs or scope amendments accurately. | Stable typed step IDs, explicit dependencies and output contracts; the current linear pipeline remains a supported recipe. Avoid an arbitrary workflow canvas or a new JavaScript engine as the first move. |
| P2 | **Progress and result semantics can drift.** Overview shows “Not started” for a proposal intentionally skipped by a lead-authored plan (`src/app/tasks/work_item.rs:258–264`). `Phase::Failed` says repair limit reached, but a failed explicit recovery can set it without exhausting repairs (`src/tasks.rs:63`; `src/app/tasks/recovery.rs:977–991`; `src/app/tasks/work_item.rs:519–521`). | Even safe behavior becomes confusing when UI labels describe the wrong reason. | Store typed outcome/blocker reasons. Render “Approved plan supplied”, “Check failed”, “Review blocked”, “Repair budget exhausted” and “Unknown effect” distinctly from one shared projection. |

### Evidence boundaries that matter

Current fingerprints cover bounded source content, not the dependency tree, toolchain, environment, external services or a continuous filesystem lock. Current review findings are model-authored evidence, not formal proof. Current apply-back is journaled and guarded, not globally atomic. Improvements must keep those distinctions visible.

The standalone classifier reproduction uses the unchanged function extracted from `environment.rs`, compiled with small `Check` scaffolding. It confirms classification behavior, not a full native end-to-end run. Its three cases are: diagnostic text in an assertion → classified; missing relative source import → classified; ordinary assertion → not classified.

## 3. The desired experience

1. **Ask naturally.** “Fix this regression and add coverage.” Inspection stays in chat; no setup wizard for ordinary work.
2. **Get a compact plan when warranted.** Show scope, observable acceptance, exact checks, execution/delivery environment, model policy, and spending limits. Expand details on demand. Small changes do not require a ritualized team.
3. **Approve or revise in place.** Persist plan versions and feedback. Every execution run binds to one approved revision; later changes do not rewrite old approval.
4. **Keep chatting.** A compact attached card says what is running, what is waiting, who owns it, and what needs a decision. The parent may discuss or inspect, but conflicting writes are host-blocked or explicitly reconciled.
5. **Build with fast feedback.** The builder requests focused checks through the host. Results are attached to the exact source revision. Independent acceptance happens on the final candidate, not on the builder's assertion.
6. **Handle blockers as actions.** “Dependency setup required”, “Source changed”, “Permission needed”, and “Provider unavailable” each have one relevant next action. No generic “try again” for uncertain side effects.
7. **Review the actual deliverable.** Show baseline-relative net changes, behavioral evidence, outstanding risks, cost and delivery readiness. For UI work, attach screenshots/interaction evidence only when actually captured by approved tooling.
8. **Apply explicitly.** Review/apply remains separate from verification. After application, report applied paths and run integration checks when required: unrelated original-checkout edits can change behavior even when the isolated candidate passed. Commit/push/deploy remain separate policy decisions.
9. **Continue deliberately after interruption.** Show completed steps and unknown operations. “Continue” means start a new authorized attempt from a validated boundary—not rerun the last shell command because its result was lost.

No fake percentage or invented ETA. Show actual stage transitions, queue/resource waits, elapsed time and measured estimates only when justified.

## 4. Architecture: a small typed core, not another agent framework

### Separate the objects that currently overlap

```text
WorkflowDefinition    Versioned recipe, typed steps, schemas; never execution authority
PlanRevision          Objective, scope, criteria, steps, checks, delivery/model policy
Approval              Exact revision + workspace + capabilities + budget scope
WorkflowRun           Immutable identities; lifecycle; owner; policy
StepAttempt           Specific inputs, selected worker/model, outputs, failure reason
OperationReceipt      Intent → dispatched → known result / unknown effect
Artifact              Content-addressed logs, candidate manifest, diff, review, images
DeliveryReceipt       What was applied where; conflicts/rollback; integration evidence
UserNote              Human annotation only, not execution state or proof
```

Use immutable IDs and generation/fencing tokens for operations and callbacks. Late events from a cancelled/replaced attempt cannot advance its successor.

### Extract orchestration from rendering

```text
Chat tool/API ─┐
Native UI ─────┼─> commands -> workflow reducer -> persisted transition -> effects
Recovery UI ──┘                        ↑                              |
                                      └──────── effect results ───────┘
                                                |
                          Pi RPC / managed workers / command runner / workspace service
```

- First extract a pure transition layer behind the existing UI. Pi remains the agent loop, authentication and tool-permission authority.
- State changes return explicit effects. Persist effect intent with acknowledgment before dispatching an irreversible or paid operation.
- Keep one authoritative writer per run. Use expected revision plus exclusive ownership/fencing; do not use a UI selection or stale in-memory object as authority.
- Choose a local transactional journal plus snapshots/artifact references. An embedded database is a reasonable option, but approve that dependency/storage migration separately; an unacknowledged JSON save queue is not a durable effect protocol.
- Persist workflow semantic transitions, not every token. Preserve Pi transcripts separately and bound/redact artifact retention.
- There is no exactly-once guarantee across a local journal, OS processes and provider requests. An intent whose dispatch/result is uncertain remains **UnknownEffect** until reconciled or explicitly replaced.
- Running while the app is open is the first contract. Closing the app safely and resuming later is distinct from running through app exit. Do not claim a background daemon until one is deliberately designed and implemented.

### Use a bounded execution graph

Start with this recipe:

```text
Inspect → Freeze plan → Prepare environment/workspace → Baseline evidence
    → Build with focused feedback → Capture candidate → Acceptance checks
    → Fresh review → Verified result → [explicit application → integration evidence]
                                  ↘ bounded, classified repair ↗
```

Steps declare input/output schemas, dependencies, capabilities, resource keys, deadlines, cancellation policy and whether new execution is safe after interruption. Invalid/cyclic graphs fail validation before consent.

Keep the current initial-plus-two-repairs limit initially. Focused check requests also need count/time/cost bounds so they do not become an unbounded hidden repair loop. Broader plan/contract changes require a new revision and appropriate approval; they cannot reset the budget invisibly.

Later support bounded optional fan-out: independent research, disjoint builders and specialist review. All branches use managed clickable workers. Parent-side integration validates child artifacts and dependencies. A downstream builder must consume the integrated candidate, not unknowingly start from an unrelated committed HEAD.

Do not add unrestricted saved JavaScript, another evaluator, a swarm default or distributed infrastructure to get the first recipe working. If script recipes are added later, use Pi's supported codemode and journal typed host operations, not closure/heap serialization or arbitrary tool replay.

### One worker substrate, different capabilities

Workflow workers and ordinary subagents should share lifecycle, queueing, transcript identity, cancellation and capacity management. A workflow role may intentionally remain non-interactive; sharing infrastructure must not automatically grant messaging, shell, delegation or mutation rights.

- **Builder:** bounded workspace edits, inspection, questions, and requests to a host-owned focused-check service.
- **Acceptance runner:** exact host-submitted commands; no model required.
- **Reviewer:** read-only independent context, criterion/evidence inspection, and a structured way to request missing checks. A requested check still needs applicable authority; the reviewer cannot invent a passing result.
- **Router:** optional, tool-free; constrained to an approved capability/price policy.

Resource keys cover repository integration, working directories, build targets/caches, package mutation and other shared outputs. A graph is not proof of filesystem independence. Serialize initially where scopes are unknown; split resource locks only with evidence.

## 5. Verification that means something

### Acceptance is a relationship, not a command string

Track criteria separately from checks: several criteria can cite one executed check without rerunning the same command unnecessarily. Each check needs explicit working directory, command/argv semantics, timeout, environment profile and expected evidence type. Preserve normal command permissions; an argv representation is not a sandbox.

A final verification decision should bind:

```text
approved plan revision + candidate manifest + relevant environment identity
+ exact check definitions/results/artifacts + fresh review + invalidation state
```

Use baseline evidence to distinguish an inherited failure, a regression and an unknown. A failing baseline never grants permission to ignore tests. Describe which requested behaviors were established; never turn “same as baseline” into “all tests passed”.

Changing a test/script is legitimate development but changes what an approved command does. Keep baseline-to-candidate test/config diffs available to review. Require meaningful regression coverage; do not make the builder the only judge of adequacy.

### Classify failure before choosing an action

| Category | Default action |
| --- | --- |
| Implementation/test assertion failure | Eligible bounded repair with exact diagnostic evidence |
| Missing dependency/tool/environment | Offer scoped environment action; do not consume repair budget merely for setup |
| Provider transport/rate-limit | Bounded retry only where dispatch semantics are known safe; otherwise reconcile |
| Permission denied/cancelled | Stop or wait for a new explicit decision; never reinterpret as approval |
| Source/input drift | Invalidate affected evidence; inspect/replan before further effects |
| Uncertain external effect | Quarantine that operation; inspect/reconcile; never auto-replay |
| Insufficient behavioral evidence | Request missing evidence or report blocked; do not convert prose confidence into a pass |
| Unsupported integration | Preserve candidate and expose a reviewed delivery/export route |

### Deliver usable artifacts

Store bounded full logs separately from summaries, with source/check identity and truncation metadata. Current retained summaries are valuable but should not be the only route to diagnostics. Treat logs, screenshots and source as potentially sensitive and untrusted. Local by default; no automatic upload.

For final application, use the existing baseline/transaction machinery. Add supported directories with journaled ownership and rollback rules; never remove a directory containing concurrent external files. Recheck candidate and destination immediately before effect dispatch. Explicitly report partial/unknown outcomes.

## 6. Models, spending and latency

- Replace model-name policy with catalog-backed capabilities and explicit user configuration. Include actual supported effort levels, context requirements, tools, availability and disclosed price confidence.
- Let pinned selected-model execution be a first-class path when its capabilities and cost policy are known. Adaptive routing should be opt-in and bound to the approved pool; unavailable or unsuitable choices block honestly.
- Do not require a model call to decide every deterministic transition. Reuse an approved routing decision when its relevant inputs have not changed.
- Separate ceilings: reported dollars, reserved estimated request cost, model requests, output tokens, active workers, repair rounds and elapsed time. Clearly label which are enforceable locally.
- Reserve bounded estimated cost before dispatch where rates and output bounds are known; reconcile actual usage afterward. Provider-reported/delayed billing still prevents an absolute dollar-cap guarantee.
- Show stage costs including routing, permission classification and compaction. Evaluate cost **per accepted useful change**, not cost per token or cheapest model selected.

## 7. Incremental delivery plan

Each slice should be independently reviewable and committed only after its focused checks and applicable completion validation. No big-bang storage/runtime/UI rewrite.

### Release 1 — Reliability and truthful state

**First implementation slice I recommend:** correct missing-module classification, introduce typed terminal/blocker reasons, and add regression coverage for initial execution and recovery labels.

Then deliver separate slices for per-record history quarantine/deterministic loading, accurate skipped-plan status, and actionable agent-capacity reporting. Do not raise retention limits or delete history as a shortcut.

**Exit criteria:** a missing relative source import reaches eligible diagnosis/repair; assertion text alone does not claim missing environment; a real dependency failure still blocks safely; cancellation/unknown effects never retry; recovery failures never claim exhausted repairs without proof; malformed history does not hide valid runs.

### Release 2 — Durable runtime underneath the existing recipe

Extract reducer/commands/effects, define versioned receipts and migration, and make dispatch acknowledgment mandatory at semantic effect boundaries. Keep the existing linear workflow and UI behavior initially.

**Exit criteria:** restart/late-event/crash tests cover each persistence/dispatch/result boundary; no unknown operation is replayed; completed step evidence remains inspectable; legacy tasks remain read-only unless a supported explicit migration proves the required state.

### Release 3 — Background execution and real control

Return a run handle after persisted start; add event-driven await/status, narrow typed controls, deduplicated completion delivery and a responsive chat. Add pause-at-boundary separately from cancellation. Provide a compact native run card with one relevant next action.

**Exit criteria:** chat can answer while a run proceeds; duplicate events do not duplicate effects/reports; stop semantics distinguish parent turn, workflow and tree; source conflicts are enforced by the host. This release may still serialize workflows globally.

### Release 4 — Fast checks, prepared environments and delivery

Introduce host-mediated focused checks, baseline evidence, repository execution profiles, evidence artifacts and directory-capable journaled apply-back. Verify integration against current original-checkout state when needed.

**Exit criteria:** ordinary dependency-backed projects have a deliberate setup path; builders receive bounded feedback without unrestricted shell; a normal new-directory feature has an explicit supported delivery path; isolated passing evidence is never falsely reused as integration evidence.

### Release 5 — Adaptive plans and reusable workflows

Add versioned templates, selective fan-out, scope-aware scheduling and explicit plan amendments using the durable core. Start with Verified change, Investigate, and Review; differing recipes need truthful differing outcomes. A read-only investigation need not pretend to produce an applied, tested code change.

**Exit criteria:** templates are inspectable and version-bound; edits never change an active run's authority; minimal worker count remains the default; branch integration validates actual input lineage; measured quality exceeds the current linear baseline on the evaluation set.

## 8. How to determine whether it is actually better

### Deterministic, provider-free gates

- State transition invariants and invalid graph/plan rejection.
- Fault injection before/after durable intent, dispatch, response and acknowledgment.
- Duplicate/out-of-order events, cancellation races, source drift, stale approvals and ownership takeover.
- Existing worktree/index/symlink/apply safety tests; retain real OS/process/lock assertions.
- Corrupt/legacy/oversized history and artifact records; migration rollback/recovery behavior.
- Full `scripts/test-rust.sh` at feature completion, with every scheduled group and no hidden failure retries, plus JS and prompt guards.
- Quiet Mac light/dark/narrow/zoom, keyboard navigation, disabled actions, cancellation, waiting and failure states.

### Separately approved provider-backed pilot

Use a fixed benchmark of representative small bug fixes, cross-file features, UI changes, inherited failing tests, missing dependencies, provider failures and interrupted runs. Compare the existing pipeline and new recipe on equivalent starting states and declared model/cost policies. Record unsuccessful attempts too.

Measure:

1. Independently accepted, delivered outcomes per task.
2. False-success and permission/scope violations: zero tolerated in the pilot, without claiming proof from a small sample.
3. User interventions and time spent resolving blockers.
4. End-to-end p50/p95 duration and cost per accepted outcome.
5. Useful versus discarded model requests and repairs.
6. Recovery rate from supported interruptions without duplicate effects.
7. Integration conflicts/partial application and evidence lost to truncation.
8. Concurrent work's CPU, memory, process and build-cache contention.

Set quantitative speed/cost targets after measuring the current baseline. An attractive graph or many passing fixtures does not establish model quality or practical superiority.

## 9. What was validated during this audit

- Fast-forwarded `master` to `040d72be`. Two colliding untracked files were moved, not discarded, to `/tmp/pui-pre-pull-Qyidjihc/{outcome,plan}.rs`. They differ from upstream and were not reintegrated.
- Focused workflow/Ultracode/permission JavaScript tests: **47 passed, 0 failed**.
- Standalone exact-function classifier reproduction: **3 passed**. The initial audit harness had a syntax typo, corrected before the passing run; production code was not changed. Reproduction: `/tmp/pui-workflow-audit-environment.rs` and `/tmp/pui-workflow-audit-environment`.
- Broader `node --test tests/*.test.mjs`: **118 passed, 1 failed, 1 skipped**. `tests/streaming.test.mjs` invokes Cargo and failed in CEF build setup: its attempted distribution fetch rejected a certificate with `UnknownIssuer`. No streaming assertion ran successfully for that case. Log: `/tmp/pui-workflow-audit-js.log`. No failure was retried or reclassified as a pass.
- Complete native Rust suite, installed-app interaction and paid-provider pilot were **not run**. This documentation-only audit is not a green release validation.
- Independent managed review was attempted but blocked by the host's **128 retained agent sessions** limit; no workers launched and no unmanaged fallback was used.
- Concurrent working edits appeared in permissions/orchestration/settings/sync/protocol files during the audit. They were preserved, not staged with this document, and not reviewed as part of the baseline. Test results describe the executed commands, not an immutable clean release snapshot.
- This audit made no runtime edits, application install, push, deployment, live task-record edits, permission-policy changes or destructive history cleanup. The broader test's build script attempted a CEF fetch; it failed as described above.

## Bottom line

The strongest next version is not “more agents.” It is **a responsive, durable workflow runtime that produces trustworthy, usable results and makes recovery understandable**. Preserve the current safety properties, fix the concrete failure modes first, and grow the existing verified-change recipe into that system one validated slice at a time.
