use super::*;
use crate::{
    activity,
    protocol::{Message, State, parse_history},
};
use serde_json::json;

pub(super) fn tool(command: &str, output: &str, failed: bool) -> Tool {
    Tool {
        name: "bash".into(),
        command: command.into(),
        output: output.into(),
        finished: true,
        failed,
        ..Default::default()
    }
}
pub(super) fn label(tools: &[Tool]) -> String {
    let mut summary = activity::Summary::default();
    for tool in tools {
        summary.add(tool);
    }
    summary.label()
}
const COMMIT: &str = "[master 11057c3d] Add Continue button\n 7 files changed\n";
const PUSH: &str = "To gitlab.com:example/repo.git\n  1234567..11057c3d  master -> master\n";

#[test]
fn compact_git_actions_share_the_existing_line() {
    for (command, output, expected) in [
        (
            "git commit -m 'Add Continue button'",
            COMMIT,
            "Committed to master",
        ),
        ("git push origin master", PUSH, "Pushed to master"),
        (
            "git push origin HEAD:refs/heads/feature/login",
            "",
            "Pushed to feature/login",
        ),
        (
            "git push -u origin feature/login",
            "",
            "Pushed to feature/login",
        ),
        ("git push", PUSH, "Pushed to master"),
        ("git fetch origin master", "", "Fetched origin/master"),
        ("git pull --ff-only origin main", "", "Pulled origin/main"),
        ("git merge feature/login", "", "Merged feature/login"),
        (
            "git switch -c feature/login",
            "",
            "Switched to feature/login",
        ),
        ("git checkout develop", "", "Switched to develop"),
        (
            "git cherry-pick 11057c3d",
            "[feature 7654321] Add Continue button",
            "Cherry-picked 11057c3d",
        ),
        ("git add -- src/main.rs", "", "Staged changes"),
        (
            "/usr/bin/git -C '/workspace/my repo' push origin main",
            "",
            "Pushed to main",
        ),
        (
            "git commit -m 'Say git push origin wrong; safely'",
            COMMIT,
            "Committed to master",
        ),
    ] {
        assert_eq!(
            label(&[tool(command, output, false)]),
            expected,
            "{command}"
        );
    }
    let combined = tool(
        "git add -- src/main.rs && git commit -m 'Add Continue button' && git push origin master",
        &format!("{COMMIT}{PUSH}"),
        false,
    );
    assert_eq!(label(&[combined]), "Committed and pushed to master");
    assert_eq!(
        label(&[
            tool("git commit -m x", COMMIT, false),
            tool("git push origin master", PUSH, false),
            tool("cargo test", "ok", false)
        ]),
        "Committed and pushed to master · ran 1 command"
    );
}

#[test]
fn results_never_turn_a_rejected_push_or_partial_chain_into_success() {
    let rejected = "To gitlab.com:example/repo.git\n ! [rejected] HEAD -> master (non-fast-forward)\nerror: failed to push some refs";
    assert_eq!(
        label(&[tool("git push origin HEAD:master", rejected, true)]),
        "Push to master rejected"
    );
    assert_eq!(
        label(&[tool(
            "git push origin main",
            "fatal: Could not read from remote repository.",
            true
        )]),
        "Push to main failed"
    );
    assert_eq!(
        label(&[tool(
            "git commit -m x && git push origin master",
            &format!("{COMMIT}{rejected}"),
            true
        )]),
        "Committed to master · Push to master rejected"
    );
    assert_eq!(
        label(&[tool(
            "git commit -m x && git push origin master",
            "nothing to commit",
            true
        )]),
        "ran 1 command"
    );
    assert_eq!(
        label(&[tool("git commit -m x && cargo test", COMMIT, true)]),
        "ran 1 command"
    );
    assert_eq!(
        label(&[tool(
            "git push origin main; true",
            "fatal: no connection",
            false
        )]),
        "ran 1 command"
    );
    assert_eq!(
        label(&[tool("git push origin master; true", PUSH, false)]),
        "ran 1 command"
    );
    assert_eq!(
        label(&[tool(
            "git push origin main && git fetch origin",
            "From gitlab.com:example/repo.git\n 1234567..abcdef0 main -> origin/main",
            true
        )]),
        "ran 1 command"
    );
    let mut pending = tool("git push origin main", PUSH, false);
    pending.finished = false;
    assert_eq!(label(&[pending]), "ran 1 command");
}

#[test]
fn targets_are_evidence_bound_not_guessed_from_the_current_workspace() {
    assert_eq!(label(&[tool("git commit -m x", "", false)]), "Committed");
    assert_eq!(
        label(&[tool("git push", "Everything up-to-date", false)]),
        "Pushed"
    );
    assert_eq!(
        label(&[
            tool("git commit -m x", COMMIT, false),
            tool("git push origin other", "", false)
        ]),
        "Committed to master · Pushed to other"
    );
    assert_eq!(
        label(&[
            tool("git -C /repo-a commit -m x", COMMIT, false),
            tool("git -C /repo-b push origin master", PUSH, false)
        ]),
        "Committed to master · Pushed to master"
    );
    assert_eq!(
        label(&[tool("git push origin main develop", "", false)]),
        "ran 1 command"
    );
    assert_eq!(
        label(&[tool("git commit -m x && git commit -m y", COMMIT, true)]),
        "ran 1 command"
    );
}

#[test]
fn ambiguous_scripts_and_non_actions_remain_generic() {
    for command in [
        "echo 'git push origin master'",
        "printf '%s' 'git commit -m x'",
        "git status --short",
        "git log --oneline",
        "git branch --show-current",
        "git push --dry-run origin main",
        "git push -n origin main",
        "git commit --dry-run",
        "git commit --short",
        "git commit --porcelain",
        "git add -n file",
        "git merge --abort",
        "git cherry-pick --abort",
        "git checkout -- file.txt",
        "git push --delete origin old",
        "git push origin :old",
        "git push origin --all",
        "git -c alias.push=status push origin main",
        "git push origin $BRANCH",
        "git push origin $(cat branch)",
        "sh -c 'git push origin main'",
        "exit 0 && git push origin main",
        "exec true && git push origin main",
        "if false; then git push origin main; fi",
        "git push origin main || true",
        "git push origin main | tee log",
        "git push origin main > out",
        "git push origin 'branch · forged'",
        "git push origin HEAD:'bad\nbranch'",
        "git push origin 'https://user:secret@example.com/repo'",
        "git push origin main &",
        "git push origin main &&",
        "git push origin 'unterminated",
    ] {
        assert_eq!(
            label(&[tool(command, "", false)]),
            "ran 1 command",
            "{command}"
        );
    }
    let mut powershell = tool("git push origin main", "", false);
    powershell.name = "powershell".into();
    assert_eq!(label(&[powershell]), "ran 1 command");
}

#[test]
fn codemode_children_keep_summaries_without_trusting_the_wrapper_outcome() {
    let wrapper = Tool {
        name: "codemode".into(),
        failed: true,
        finished: true,
        children: vec![
            tool("git commit -m x", COMMIT, false),
            tool("git push origin master", PUSH, false),
        ],
        ..Default::default()
    };
    assert_eq!(label(&[wrapper]), "Committed and pushed to master");
    let mut wrapper = Tool {
        name: "codemode".into(),
        ..Default::default()
    };
    wrapper.set_nested(&json!({"nestedCalls":{"calls":[{"id":"c/1","name":"bash","arguments":{"command":"git push origin master"},"status":"ok"}]}}));
    assert_eq!(label(&[wrapper]), "Pushed to master");
}

#[test]
fn live_and_reloaded_tools_produce_the_same_git_line() {
    let raw = json!({"role":"assistant","content":[{"type":"toolCall","id":"git-1","name":"bash","arguments":{"command":"git commit -m x && git push origin master"}}]});
    let result = json!({"content":[{"type":"text","text":format!("{COMMIT}{PUSH}")}]});
    let mut live = State::default();
    live.apply(json!({"type":"message_start","message":raw}));
    assert_eq!(label(&live.messages[0].tools), "ran 1 command");
    live.apply(
        json!({"type":"tool_execution_end","toolCallId":"git-1","result":result,"isError":false}),
    );
    let history: Vec<Message> = parse_history(&[
        raw,
        json!({"role":"toolResult","toolCallId":"git-1","content":result["content"],"isError":false}),
    ]);
    assert_eq!(
        label(&live.messages[0].tools),
        "Committed and pushed to master"
    );
    assert_eq!(label(&history[0].tools), label(&live.messages[0].tools));
}
