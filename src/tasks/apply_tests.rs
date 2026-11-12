use super::*;
use crate::tasks::{Attempt, Check, Contract, Criterion, Finding, Review, Verdict};

fn git_ok(root: &Path, args: &[&str]) {
    git(root, args).unwrap();
}
struct Fixture {
    task: Task,
}
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!("pui-apply-{}", uuid::Uuid::new_v4()));
        let root = home.join("original");
        fs::create_dir_all(&root).unwrap();
        git_ok(&root, &["init", "-q"]);
        for (name, bytes) in [
            ("main.txt", b"baseline\n".as_slice()),
            ("other.txt", b"unrelated\n".as_slice()),
            ("delete.txt", b"delete baseline\n".as_slice()),
        ] {
            fs::write(root.join(name), bytes).unwrap();
        }
        git_ok(&root, &["add", "."]);
        git_ok(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "commit",
                "-qm",
                "baseline",
            ],
        );
        let head = text(git(&root, &["rev-parse", "HEAD"]).unwrap())
            .unwrap()
            .trim()
            .to_string();
        let candidate = home.join("candidate");
        let output = Command::new("git")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "worktree",
                "add",
                "--detach",
            ])
            .arg(&candidate)
            .arg(&head)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(output.status.success());
        let mut task = Task::new(root, "p".into(), "m".into(), "off".into());
        task.workspace = Some(candidate);
        task.base_head = head;
        task.isolated = true;
        let mut result = Self { task };
        result.review();
        result
    }
    fn original(&self) -> &Path {
        &self.task.source
    }
    fn candidate(&self) -> &Path {
        self.task.workspace.as_ref().unwrap()
    }
    fn review(&mut self) {
        let contract = Contract {
            criteria: vec![Criterion {
                id: "C1".into(),
                description: "fixture check".into(),
                command: "test fixture".into(),
            }],
        };
        let mut a = Attempt::new(0);
        a.fingerprint = Some(workspace::fingerprint(self.candidate()).unwrap());
        a.checks.push(Check {
            criterion: "C1".into(),
            command: "test fixture".into(),
            exit_code: Some(0),
            output: "fixture".into(),
            truncated: false,
            cancelled: false,
            duration_ms: 1,
        });
        a.review = Some(Review {
            summary: "fixture reviewed".into(),
            criteria: vec![Finding {
                id: "C1".into(),
                status: Verdict::Pass,
                evidence: "fixture evidence".into(),
            }],
        });
        self.task.approved = Some(contract);
        self.task.attempts = vec![a];
        self.task.phase = Phase::Ready;
    }
}
#[test]
fn preview_is_net_candidate_changes_and_does_not_mutate_original_index_or_files() {
    let mut f = Fixture::new();
    fs::write(f.candidate().join("main.txt"), "candidate\n").unwrap();
    fs::write(f.candidate().join("new.txt"), "new\n").unwrap();
    f.review();
    // Unrelated staged, unstaged and untracked edits in the original stay intact.
    fs::write(f.original().join("other.txt"), "staged user edit\n").unwrap();
    git_ok(f.original(), &["add", "other.txt"]);
    fs::write(f.original().join("other.txt"), "unstaged user edit\n").unwrap();
    fs::write(f.original().join("user.txt"), "untracked user\n").unwrap();
    let status = git(f.original(), &["status", "--porcelain=v1", "-z"]).unwrap();
    let index = fs::read(
        canonical_output(f.original(), &["rev-parse", "--git-dir"])
            .unwrap()
            .join("index"),
    )
    .unwrap();
    let before = workspace::fingerprint(f.original()).unwrap();
    let plan = prepare(&f.task).unwrap();
    assert_eq!(plan.changes.len(), 2);
    assert!(plan.changes.iter().all(|c| !c.conflict));
    assert!(
        plan.changes
            .iter()
            .all(|c| c.path != "other.txt" && c.path != "user.txt")
    );
    assert_eq!(plan.patch_hash, digest(plan.patch_payload().as_bytes()));
    assert!(plan.summary().contains("HEAD-relative"));
    assert!(plan.summary().contains("not permission to apply"));
    check(&f.task, &plan).unwrap();
    assert_eq!(apply(&f.task, &plan).unwrap_err(), TRANSACTIONAL_BLOCKER);
    assert_eq!(workspace::fingerprint(f.original()).unwrap(), before);
    assert_eq!(
        fs::read(plan.identity.original_git_dir.join("index")).unwrap(),
        index
    );
    assert_eq!(
        git(f.original(), &["status", "--porcelain=v1", "-z"]).unwrap(),
        status
    );
}
#[test]
fn original_target_conflicts_and_untracked_collisions_are_explicit() {
    let mut f = Fixture::new();
    fs::write(f.candidate().join("main.txt"), "builder\n").unwrap();
    fs::write(f.candidate().join("new.txt"), "builder new\n").unwrap();
    fs::write(f.original().join("main.txt"), "user\n").unwrap();
    fs::write(f.original().join("new.txt"), "user untracked\n").unwrap();
    f.review();
    let plan = prepare(&f.task).unwrap();
    assert_eq!(plan.changes.iter().filter(|c| c.conflict).count(), 2);
    assert!(plan.blockers.iter().any(|b| b.starts_with("Conflict:")));
    assert!(apply(&f.task, &plan).is_err());
    assert_eq!(
        fs::read_to_string(f.original().join("main.txt")).unwrap(),
        "user\n"
    );
}
#[test]
fn dirty_at_start_has_no_head_baseline_and_is_rejected() {
    let mut f = Fixture::new();
    f.task.dirty_source = true;
    assert!(prepare(&f.task).unwrap_err().contains("snapshot receipt"));
}
#[test]
fn stale_source_candidate_index_head_identity_and_altered_preview_are_rejected() {
    let mut f = Fixture::new();
    fs::write(f.candidate().join("main.txt"), "builder\n").unwrap();
    f.review();
    let plan = prepare(&f.task).unwrap();
    fs::write(f.original().join("unrelated-new.txt"), "new user\n").unwrap();
    assert!(check(&f.task, &plan).is_err());
    let plan = prepare(&f.task).unwrap();
    git_ok(f.original(), &["add", "unrelated-new.txt"]);
    assert!(check(&f.task, &plan).is_err());
    let plan = prepare(&f.task).unwrap();
    let mut altered = plan.clone();
    altered.changes[0].after = Some("tamper".into());
    assert!(check(&f.task, &altered).is_err());
    fs::write(f.candidate().join("main.txt"), "changed after review\n").unwrap();
    assert!(check(&f.task, &plan).is_err());
    f.review();
    let plan = prepare(&f.task).unwrap();
    std::mem::swap(&mut f.task.source, f.task.workspace.as_mut().unwrap());
    assert!(check(&f.task, &plan).is_err());
}
#[test]
fn deletion_binary_mode_and_staged_only_changes_are_never_supported() {
    let mut f = Fixture::new();
    fs::remove_file(f.candidate().join("delete.txt")).unwrap();
    fs::write(f.candidate().join("main.txt"), [0, 0xff, 1]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            f.candidate().join("other.txt"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    f.review();
    let plan = prepare(&f.task).unwrap();
    assert!(plan.changes.iter().all(|c| c.unsupported.is_some()));
    let binary = plan.changes.iter().find(|c| c.path == "main.txt").unwrap();
    assert_eq!(binary.after, None);
    assert_eq!(binary.after_hash, Some(digest(&[0, 0xff, 1])));
    assert!(plan.changes.iter().any(|c| c.kind == ChangeKind::Delete));
    assert!(apply(&f.task, &plan).is_err());
    git_ok(f.candidate(), &["add", "main.txt"]);
    // Staged-only payload not visible in net bytes is explicitly blocked.
    fs::write(f.candidate().join("main.txt"), "baseline\n").unwrap();
    f.review();
    assert!(
        prepare(&f.task)
            .unwrap()
            .blockers
            .iter()
            .any(|b| b.contains("index differs"))
    );
}
#[cfg(unix)]
#[test]
fn symlink_typechange_external_parent_and_protected_paths_are_rejected() {
    use std::os::unix::fs::symlink;
    let mut f = Fixture::new();
    fs::remove_file(f.candidate().join("main.txt")).unwrap();
    symlink(
        f.original().join("main.txt"),
        f.candidate().join("main.txt"),
    )
    .unwrap();
    f.review();
    assert!(prepare(&f.task).is_err());
    assert_eq!(
        fs::read_to_string(f.original().join("main.txt")).unwrap(),
        "baseline\n"
    );
    for path in [
        "../outside",
        "/absolute",
        ".git/config",
        "safe/.ENV.local",
        "key.pem",
        "x\\y",
        "a\nb",
    ] {
        assert!(name(path.as_bytes()).is_err());
    }
    let mut f = Fixture::new();
    fs::write(f.candidate().join(".env.local"), "not a secret fixture\n").unwrap();
    f.review();
    assert!(prepare(&f.task).is_err());
    let f = Fixture::new();
    let outside = f.original().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "must not read fixture\n").unwrap();
    symlink(&outside, f.candidate().join("linked")).unwrap();
    let mut total = 0;
    assert!(read(f.candidate(), "linked/secret.txt", &mut total).is_err());
    assert_eq!(total, 0);
}
#[test]
fn head_directory_case_ambiguity_and_wrong_linked_worktree_are_rejected() {
    let mut f = Fixture::new();
    fs::write(f.candidate().join("main.txt"), "candidate\n").unwrap();
    f.review();
    let plan = prepare(&f.task).unwrap();
    git_ok(
        f.candidate(),
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "--allow-empty",
            "-qm",
            "changed HEAD",
        ],
    );
    assert!(check(&f.task, &plan).is_err());
    let f = Fixture::new();
    fs::remove_file(f.candidate().join("main.txt")).unwrap();
    fs::create_dir(f.candidate().join("main.txt")).unwrap();
    fs::write(f.candidate().join("main.txt/nested"), "typechange\n").unwrap();
    let mut total = 0;
    assert!(read(f.candidate(), "main.txt", &mut total).is_err());
    assert!(prepare(&f.task).is_err());
    let mut f = Fixture::new();
    fs::write(f.candidate().join("MAIN.TXT"), "case conflict\n").unwrap();
    f.review();
    // On case-insensitive systems this aliases main.txt and is a modification;
    // on case-sensitive systems it creates a second path and must be refused.
    if git(
        f.candidate(),
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
    .unwrap()
    .split(|b| *b == 0)
    .any(|p| p == b"MAIN.TXT")
    {
        assert!(prepare(&f.task).is_err());
    }
    let other = Fixture::new();
    f.task.workspace = Some(other.candidate().to_path_buf());
    f.task.base_head = text(git(other.candidate(), &["rev-parse", "HEAD"]).unwrap())
        .unwrap()
        .trim()
        .into();
    f.review();
    assert!(prepare(&f.task).is_err());
}
#[test]
fn non_ready_failed_or_stale_evidence_is_not_previewable() {
    let mut f = Fixture::new();
    f.task.phase = Phase::Building;
    assert!(prepare(&f.task).is_err());
    f.task.phase = Phase::Ready;
    f.task.attempts[0].invalidated = true;
    assert!(prepare(&f.task).is_err());
}
