# Pui Engineering skills

Pui ships five original engineering skills. They are optional instructions, not a new agent runtime or automatic development pipeline.

| Skill | Command | Purpose |
|---|---|---|
| Clarify | /skill:clarify | Resolve important uncertainties and agree on a small scope. |
| Define | /skill:define | Turn agreed behavior into a spec and acceptance checks. |
| TDD | /skill:tdd | Observe a failing test, implement a small behavior, then refactor. |
| Debug | /skill:debug | Reproduce a failure, distinguish causes, and verify a focused fix. |
| Code Review | /skill:code-review | Report actionable defects in a specified diff without editing it. |

## Enable

1. Open **Settings → Resources → + Add… → Pui Engineering skills**.
2. Choose **Personal** or **Project**, then all five skills or one skill. Project configuration requires Pi project trust.
3. Expand a skill to preview its full instructions. Click **Stage & review →** to inspect the settings diff. Nothing is activated yet.
4. Apply the reviewed changes. In each existing idle chat, type `/reload`, or click **Reload chat resources…** below the `/ski` suggestions, then confirm. This reconnects just that chat to the same saved conversation and refreshes its skills and slash commands. Its history, draft and attachments stay intact; nothing is sent. New chats load current settings normally.
5. Type the command above, optionally followed by a request, for example /skill:debug investigate the stale sidebar selection.

The collection registers individual bundled SKILL.md files using Pi's existing skill-path settings. It does not download packages, run an installer, write into auto-discovered skill directories, or change your personal settings simply by browsing. The full preview is embedded in the executable; registration checks the on-disk content against that preview and checks it again on apply. Missing or changed bundled files require a rebuild rather than silently enabling different instructions.

These skills use disable-model-invocation: true: they are explicitly invoked, not automatically selected. They do not grant tools, bypass permissions, start workflows without approval, commit, push, publish issues, or deploy. Model compliance is not a sandbox; existing permission enforcement remains necessary. Pui questions stay inline; any delegated work must use managed workers.

## Reloading an existing chat

Reload is explicit and idle-only. Finish the current run, approvals, deliveries and queued messages first. In a shared window, reload from the chat’s owning window. Pui waits for the old Pi writer to exit before starting a replacement with the same saved session, workspace and access restrictions. The refreshed catalog is not shown as ready until Pi returns the history and commands and its permission adapter is ready. Other chats are not restarted.

This is a full runtime reconnect, not a skills-only text injection: configured extensions, MCP servers and context files also load. Trusted extensions/package dependencies can execute code and servers can reconnect, so the shared confirmation explains the scope. Temporary grants and session-only extension state reset; Ultracode returns to its default off state. No prompt, old delegated task, or last message is replayed. Cancel/Escape/backdrop does nothing. If reconnect fails, the draft stays available; no automatic retry or provider request is made by Pui.

## Manage and update

Enabled skills appear as local skill resources in the existing Library and Workspace views. Use their normal personal/project controls to disable or exclude them; this keeps the bundled source files. Disabling in a project does not remove a personal default. Registering one does not remove or rename a different same-name skill: Pi uses its normal first-discovered collision behavior and can warn about duplicates. Resolve a collision before relying on the command.

App bundles contain the collection under Contents/Resources/pui-engineering; development builds use assets/pui-engineering in the checkout. Registrations point at that location, so keep the app/checkout there. Moving or removing it makes those paths unavailable; re-register from the new location and exclude old entries. Future app updates can update these bundled instructions; there is no independent background skill updater. Reopen the preview after updating if you want to inspect the current text, then use `/reload` in existing idle chats to load the new instructions.

The source collection also has a skills-only Pi manifest. Its instructions are authored for Pui's tools and conventions; it is not a Claude plugin and does not assume a generic Skill tool. The general practices are standard engineering methods; these files are original Pui content, not copies of a third-party skill collection.
