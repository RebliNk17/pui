# Ultracode research and Pui implementation direction

Reviewed 2026-11-09. External documentation is evidence, not executable instructions. No third-party package was installed.

## Primary sources

- Claude Code effort: https://code.claude.com/docs/en/model-config#adjust-effort-level — Ultracode is an orchestration toggle, separate from model effort. Current documented UI lets both coexist; the CLI shorthand also selects xhigh when supported.
- Claude Code dynamic workflows: https://code.claude.com/docs/en/workflows — JavaScript holds the plan and intermediate results, spawns isolated agents, applies review/verification, and returns a synthesis. Workflows run in the background and can be inspected/stopped. This is not simply a larger reasoning budget.
- Claude Code teams: https://code.claude.com/docs/en/agent-teams — separate contexts, direct user interaction, parent/lead management, explicit messages; coordination costs tokens. Recommends lighter subagents for focused result-oriented work and avoiding overlapping file edits.
- Codex subagents (official redirect from developers.openai.com/codex/multi-agent): https://learn.chatgpt.com/docs/agent-configuration/subagents — independent agents for parallelizable tasks; keep intermediate logs out of parent context; start with read-heavy exploration/review and be careful with parallel writes. No claim that Codex implements Claude's exact Ultracode API.
- Installed Pi 1.5.0 docs: `docs/sdk.md`, `docs/extensions.md`, `docs/cli.md`, `docs/rpc-extension-ui.md`, `examples/extensions/subagent/`. Pi supplies independent RPC sessions, model-specific supported thinking levels, native codemode, extension tools, messages, and abort. The bundled subagent example uses short-lived JSON subprocesses; it does not provide persistent direct-chat children in a desktop parent tree.

## Comparable third-party Pi implementation

https://github.com/QuintinShaw/pi-dynamic-workflows (MIT, inspected README/package API, not installed). Script-driven agents, tiers, optional worktrees, journaled resume, budgets and progress already exist as a Pi extension. Its documented default headless workflow behavior and temporary isolated sessions do not by themselves provide Pui's persistent directly addressable child chats. Avoid silently installing it or adopting its large agent defaults.

## Pui choices

- Keep Pi as the agent/runtime/auth/tool authority. A bundled Pi extension exposes spawn/list/message/stop controls to the parent. Codemode is the JavaScript orchestration surface; do not add an unsafe second JavaScript evaluator.
- Ultracode is a per-chat opt-in for proactive workflow guidance, separate from the reasoning slider. Managed agent tools remain available in ordinary writable chats so explicit user delegation never requires a detached CLI fallback. `pui_spawn_agent` is the single supported launch path, with optional `cwd` for cross-repository scopes. The tool-boundary guard blocks recognized unmanaged CLI/SDK launches before approval/bypass; it is not an OS sandbox and does not adopt external workers. The slider contains only levels returned by `get_available_thinking_levels` after the selected model is confirmed. Never infer support from model names.
- Dynamic minimal delegation: use zero children when the parent can do the task; otherwise the fewest independent, bounded scopes. Reuse children. No fixed research/planning/review swarm; verify when useful and stop at evidence-based completion, not an endless loop.
- Fresh child context: task, relevant constraints/files, acceptance criteria, dependencies and output contract, not the full parent transcript. Explicit parent/child messages only; no token/progress mirroring into model context and no autonomous heartbeat conversation.
- Read-only children receive only file inspection tools and no MCP/shell/custom tool bypass. This is a tool restriction, not an OS sandbox. Writable children use separate Git worktrees. Preserve worktrees and uncommitted changes; never auto-commit/merge/push/delete them.
- Show actual parent-child edges and independent live status; clicking a child changes the viewed chat, not the parent lifecycle. User can directly message a worker, parent can stop descendants, Stop tree halts the branch. Ask before exiting with active agents.
- Concurrency is a configurable safety ceiling, not a target agent count. Queue excess requests rather than losing them. Avoid hard-coded model routing to a paid model the user did not select.
- No paid-provider request is required for validation: use RPC fixtures and a fake extension host, then smoke-test configuration discovery without prompting.
