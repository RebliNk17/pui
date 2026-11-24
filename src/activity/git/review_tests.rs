use super::tests::{label, tool};

#[test]
fn uncertain_paths_and_push_sources_never_combine() {
    let commit = "[master 11057c3d] x";
    let push = "To example:repo.git\n 1234567..11057c3d HEAD~1 -> master";
    assert_eq!(
        label(&[tool(
            "git commit --allow-empty -m x; git switch other; git push --force origin HEAD:master",
            &format!("{commit}\nSwitched to branch 'other'\n{push}"),
            false
        )]),
        "Committed to master · Pushed to master · ran 1 command"
    );
    assert_eq!(
        label(&[tool(
            "git commit --allow-empty -m x && git push origin HEAD~1:master",
            &format!("{commit}\n{push}"),
            false
        )]),
        "ran 1 command"
    );
    assert_eq!(
        label(&[
            tool("git commit -m x", commit, false),
            tool("cd -- /repo-b && git push origin master", push, false)
        ]),
        "Committed to master · ran 1 command"
    );
    assert_eq!(
        label(&[
            tool("cd ~/repo && git commit -m x", commit, false),
            tool("cd '~/repo' && git push origin master", push, false)
        ]),
        "Pushed to master · ran 1 command"
    );
    assert_eq!(
        label(&[tool("cd /repo-a; cd sub && git commit -m x", commit, false)]),
        "ran 1 command"
    );
    assert_eq!(
        label(&[
            tool("git commit -m x", commit, false),
            tool("some-script", "", false),
            tool("git push origin master", "", false)
        ]),
        "Committed to master · Pushed to master · ran 1 command"
    );
}

#[test]
fn shared_dry_run_output_cannot_prove_another_push_succeeded() {
    let output = "To example:repo.git\n 1234567..abcdef0 main -> main\nfatal: cannot change to missing directory";
    assert_eq!(
        label(&[tool(
            "git push --dry-run origin main; git -C /definitely-missing push origin main",
            output,
            true
        )]),
        "ran 1 command"
    );
    assert_eq!(
        label(&[tool(
            "echo '[master abcdef0] fake'; git commit -m real",
            "[master abcdef0] fake\nnothing to commit",
            true
        )]),
        "ran 1 command"
    );
}

#[test]
fn directory_transitions_do_not_combine_different_repositories() {
    let commit = "[master 11057c3d] x";
    let push = "To example:repo.git\n 1234567..11057c3d master -> master";
    assert_eq!(
        label(&[
            tool("cd /repo-a && cd sub && git commit -m x", commit, false),
            tool(
                "cd /repo-b && cd sub && git push origin master",
                push,
                false
            )
        ]),
        "Committed to master · Pushed to master"
    );
    assert_eq!(
        label(&[
            tool("git -C /repo-a -C /shared commit -m x", commit, false),
            tool("git -C /repo-b -C /shared push origin master", push, false)
        ]),
        "Committed and pushed to master"
    );
}

#[test]
fn dry_run_flag_bundles_are_never_reported_as_staging() {
    for command in [
        "git add -nA .",
        "git add -An .",
        "git add -nv .",
        "git add --refresh .",
        "git add --help",
    ] {
        assert_eq!(
            label(&[tool(command, "", false)]),
            "ran 1 command",
            "{command}"
        );
    }
    assert_eq!(label(&[tool("git add -Av .", "", false)]), "Staged changes");
}

#[test]
fn head_source_uses_real_destination_and_detached_commits_have_no_branch() {
    assert_eq!(
        label(&[tool(
            "git push origin HEAD",
            "To example:repo.git\n 1234567..abcdef0 HEAD -> master",
            false
        )]),
        "Pushed to master"
    );
    assert_eq!(
        label(&[tool(
            "git push origin HEAD",
            "To example:repo.git\n ! [rejected] HEAD -> master (non-fast-forward)",
            true
        )]),
        "Push to master rejected"
    );
    assert_eq!(label(&[tool("git push origin HEAD", "", false)]), "Pushed");
    assert_eq!(
        label(&[tool(
            "git commit --allow-empty -m x",
            "[detached HEAD 11057c3d] x",
            false
        )]),
        "Committed"
    );
}
