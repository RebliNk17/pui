/**
 * Pui Cloud Agent workflow extension.
 * Loaded automatically by `pui-daemon` for every cloud session running in an isolated
 * `pui_agent/<slug>-<rand6>` worktree under Auto mode.
 */
export default function (pi: any) {
  const branch = (process.env.PUI_CLOUD_BRANCH || '').trim();
  const baseBranch = (process.env.PUI_CLOUD_BASE_BRANCH || 'main').trim() || 'main';
  if (!branch) return;

  const instructions = [
    `Pui Cloud Autonomous Worktree Mode:`,
    `- You are working in an isolated Git worktree on branch \`${branch}\` (base branch: \`${baseBranch}\`).`,
    `- Always keep all edits inside this worktree on \`${branch}\`. Never switch to, commit on, or push to \`${baseBranch}\`, \`main\`, or \`master\`.`,
    `- Auto mode is enabled and pre-approves edits inside your worktree, builds/tests, \`git add\`, \`git commit\`, \`git push -u origin ${branch}\`, and \`gh pr create\`.`,
    `- When your task is complete and verified with focused tests/checks:`,
    `  1. Stage and commit your changes with a clear conventional commit message on \`${branch}\`.`,
    `  2. Push your branch to origin: \`git push -u origin ${branch}\`.`,
    `  3. Open a Pull Request targeting \`${baseBranch}\` using \`gh pr create --base ${baseBranch} --head ${branch} --title "<concise title>" --body "<summary and verification>"\`.`,
    `  4. In your final response, include the branch name (\`${branch}\`), commit SHA, and PR URL (or note if \`gh\` is not authenticated yet).`,
    `- If the user only asked a read-only question and no files were modified, do not create an empty commit or PR.`,
  ].join('\n');

  pi.on('before_agent_start', (e: any) => ({
    systemPrompt: `${e.systemPrompt}\n\n${instructions}`,
  }));
}
