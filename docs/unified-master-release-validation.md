# Unified master release validation

## Validated source

Release resumes user-authorized stabilization/build/install, not feature development. Source `00194b25` includes scheduled-job delete/cross-window workflow cleanup plus previous Git workflows, compact controls, project picker, chronological transcript, theme colors, background previews and terminal-open updates. Initial source hashes (1007 files) remained unchanged throughout validation except the intentional runner scheduling change below. No concurrent edits were overwritten.

## Complete local evidence

First full run `/tmp/pui-final-release-rust.log`:1787 bulk passed/1failed/2ignored, Git preview3sdeadline in index-action UI Escape fixture. Runner now excludes entire `app::changes::index_actions::tests::` group from bulk and runs its6tests once serially. No test assertion/deadline/resource bound changed, no omitted coverage/automatic failure retry. Resolved CommandLineToolsGit and installed Python are explicitly on PATH. This is test scheduling mitigation, not proof of a production deadline repair or zero flake rate.

Final `/tmp/pui-final-release-serial-rust.log`: **1927 binary +10integration passed**,2existingignored/zero failures. Bulk1782;watchers14/sync32/projection19/export43/commitUI8/modes3/SVGloader2/indexUI6/commitbackend14/SVGlifecycle1/ownership2/automationlock1. All runner groups completed exactly once. Native tests include light/dark narrow/zoom cancellation and permission states; disposable Git backend tests cover initialize→identity→stage→initialcommit, branch creation/switch, and local bare Push. These are automated/injected tests, not physical manual acceptance or live hosted authentication.

`/tmp/pui-final-release-js.log`:98passed/1existingoptional skipped. `/tmp/pui-final-release-python.log`:48passed; publish output mocked. StrictClippy alltargets-Dwarnings, cargo fmt --check, git diff --check passed. `/tmp/pui-final-release-clippy.log`. No application profiling performed.

Earlier isolated attempts in `.worktrees/unified-master-release-02798173` stalled or terminated; that blocker report remains there as historical uncommitted evidence. Main checkout source drift was avoided by hashes once edits were finished. Complete later pass above supersedes prior local validation blocker, not evidence of its underlying cause.

## Hosted CI and rollout limits

CircleCI previously documented exhausted credits (`free-plan-no-credits-available`, pipeline28/job31); current unauthenticated API404 does not establish hosted result. No billing changes or repeated hosted runs. Local full validation is green; hostedCI is not claimed green.

Bundle build/signature/runtime/CEF/SVG/install evidence will be recorded below after completion. No running-app restart or chat/settings reset is planned. AccessKit disabled; physical/manual Git UI acceptance remains unperformed. No real user repository mutation except authorized source commit/push. Development cron remains paused.
