use super::*;
use crate::tasks::{Attempt, Check, Contract, Criterion, Finding, Review, Verdict};
#[path = "apply_additional_tests.rs"]
mod additional;
fn git_ok(root: &Path, args: &[&str]) {
    git(root, args).unwrap();
}
struct Fixture {
    task: Task,
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self::dirty(false)
    }
    fn dirty(dirty: bool) -> Self {
        let home = std::env::temp_dir().join(format!("pui-apply-{}", uuid::Uuid::new_v4()));
        let root = home.join("original");
        fs::create_dir_all(&root).unwrap();
        git_ok(&root, &["init", "-q"]);
        for (name, value) in [
            ("main.txt", "baseline\n"),
            ("other.txt", "unrelated\n"),
            ("delete.txt", "delete baseline\n"),
        ] {
            fs::write(root.join(name), value).unwrap();
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
                "fixture baseline",
            ],
        );
        if dirty {
            fs::write(root.join("main.txt"), "dirty user baseline\n").unwrap();
            fs::write(root.join("other.txt"), "staged original\n").unwrap();
            git_ok(&root, &["add", "other.txt"]);
            fs::write(root.join("other.txt"), "unstaged original\n").unwrap();
            fs::write(root.join("user.txt"), "untracked original\n").unwrap();
        }
        let head = text(git(&root, &["rev-parse", "HEAD"]).unwrap())
            .unwrap()
            .trim()
            .to_owned();
        let (candidate, isolated) =
            workspace::prepare(&root, "fixture", &home.join("worktrees"), &head).unwrap();
        let mut task = Task::new(root, "p".into(), "m".into(), "off".into());
        task.workspace = Some(candidate);
        task.base_head = head;
        task.isolated = isolated;
        task.dirty_source = dirty;
        task.preparation_receipt = Some(
            capture_receipt(
                &task.source,
                task.workspace.as_ref().unwrap(),
                &task.base_head,
                &task.id,
                &home,
            )
            .unwrap(),
        );
        let mut f = Self { task, home };
        f.review();
        f
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
        let mut attempt = Attempt::new(0);
        attempt.fingerprint = Some(workspace::fingerprint(self.candidate()).unwrap());
        attempt.checks.push(Check {
            criterion: "C1".into(),
            command: "test fixture".into(),
            exit_code: Some(0),
            output: "fixture".into(),
            truncated: false,
            cancelled: false,
            duration_ms: 1,
        });
        attempt.review = Some(Review {
            summary: "fixture reviewed".into(),
            criteria: vec![Finding {
                id: "C1".into(),
                status: Verdict::Pass,
                evidence: "fixture evidence".into(),
            }],
        });
        self.task.approved = Some(contract);
        self.task.attempts = vec![attempt];
        self.task.phase = Phase::Ready;
        crate::tasks::store::save(&self.home, &self.task).unwrap();
    }
    fn edit(&mut self) {
        fs::write(self.candidate().join("main.txt"), "builder\n").unwrap();
        fs::write(self.candidate().join("new.txt"), "new\n").unwrap();
        fs::remove_file(self.candidate().join("delete.txt")).unwrap();
        self.review();
    }
    fn index(&self) -> Vec<u8> {
        fs::read(
            canonical_output(self.original(), &["rev-parse", "--git-dir"])
                .unwrap()
                .join("index"),
        )
        .unwrap()
    }
    fn fault(&self, plan: &Plan, fault: transaction::Fault) -> ApplyOutcome {
        check(&self.task, plan).unwrap();
        transaction::transact(&self.home, &self.task, plan, fault).unwrap()
    }
}
#[test]
fn dirty_baseline_builder_net_add_modify_delete_preserves_unrelated_staged_unstaged_untracked_and_index()
 {
    let mut f = Fixture::dirty(true);
    f.edit();
    let index = f.index();
    let plan = prepare(&f.task).unwrap();
    assert_eq!(plan.changes.len(), 3);
    assert!(plan.blockers.is_empty());
    assert_eq!(
        plan.changes
            .iter()
            .find(|c| c.path == "main.txt")
            .unwrap()
            .before
            .as_deref(),
        Some("dirty user baseline\n")
    );
    assert!(plan.summary().contains("builder net"));
    let result = apply(&f.home, &f.task, &plan).unwrap();
    assert_eq!(
        result.status,
        TransactionStatus::Applied,
        "{}",
        result.detail
    );
    assert_eq!(
        fs::read_to_string(f.original().join("main.txt")).unwrap(),
        "builder\n"
    );
    assert_eq!(
        fs::read_to_string(f.original().join("new.txt")).unwrap(),
        "new\n"
    );
    assert!(!f.original().join("delete.txt").exists());
    assert_eq!(
        fs::read_to_string(f.original().join("other.txt")).unwrap(),
        "unstaged original\n"
    );
    assert_eq!(
        fs::read_to_string(f.original().join("user.txt")).unwrap(),
        "untracked original\n"
    );
    assert_eq!(f.index(), index);
    assert_eq!(
        inspect_recovery(&f.home, &f.task).unwrap()[0].status,
        TransactionStatus::Applied
    );
}
#[test]
fn unrelated_edits_after_preparation_allowed_but_after_confirmation_stale() {
    let mut f = Fixture::new();
    f.edit();
    fs::write(f.original().join("other.txt"), "user late\n").unwrap();
    git_ok(f.original(), &["add", "other.txt"]);
    fs::write(f.original().join("user-new.txt"), "user\n").unwrap();
    let plan = prepare(&f.task).unwrap();
    assert!(plan.blockers.is_empty());
    let index = f.index();
    apply(&f.home, &f.task, &plan).unwrap();
    assert_eq!(f.index(), index);
    assert_eq!(
        fs::read_to_string(f.original().join("other.txt")).unwrap(),
        "user late\n"
    );
    let mut f = Fixture::new();
    f.edit();
    let plan = prepare(&f.task).unwrap();
    fs::write(f.original().join("unrelated.txt"), "drift\n").unwrap();
    assert!(apply(&f.home, &f.task, &plan).is_err());
}
#[test]
fn conflict_and_collision_block_before_any_write() {
    let mut f = Fixture::new();
    f.edit();
    fs::write(f.original().join("main.txt"), "user\n").unwrap();
    fs::write(f.original().join("new.txt"), "untracked user\n").unwrap();
    let plan = prepare(&f.task).unwrap();
    assert_eq!(plan.changes.iter().filter(|c| c.conflict).count(), 2);
    assert!(apply(&f.home, &f.task, &plan).is_err());
    assert!(f.original().join("delete.txt").exists());
}
#[test]
fn stale_candidate_task_index_and_altered_payload_block() {
    let mut f = Fixture::new();
    f.edit();
    let plan = prepare(&f.task).unwrap();
    let mut altered = plan.clone();
    altered.changes[0].after = Some("tampered\n".into());
    assert!(apply(&f.home, &f.task, &altered).is_err());
    f.task.objective = "changed task".into();
    crate::tasks::store::save(&f.home, &f.task).unwrap();
    assert!(apply(&f.home, &f.task, &plan).is_err());
    f.review();
    let plan = prepare(&f.task).unwrap();
    git_ok(f.original(), &["add", "other.txt"]); // Git may retain identical index; force real staged data.
    fs::write(f.original().join("other.txt"), "staged drift\n").unwrap();
    git_ok(f.original(), &["add", "other.txt"]);
    assert!(apply(&f.home, &f.task, &plan).is_err());
    f.review();
    let plan = prepare(&f.task).unwrap();
    fs::write(f.candidate().join("main.txt"), "candidate drift\n").unwrap();
    assert!(apply(&f.home, &f.task, &plan).is_err());
}
#[test]
fn transaction_failure_rolls_back_every_written_target_and_preserves_index() {
    for n in 1..=3 {
        let mut f = Fixture::dirty(true);
        f.edit();
        let plan = prepare(&f.task).unwrap();
        let index = f.index();
        let result = f.fault(&plan, transaction::Fault::FailAfter(n));
        assert_eq!(
            result.status,
            TransactionStatus::RolledBack,
            "{}",
            result.detail
        );
        assert_eq!(
            fs::read_to_string(f.original().join("main.txt")).unwrap(),
            "dirty user baseline\n"
        );
        assert_eq!(
            fs::read_to_string(f.original().join("delete.txt")).unwrap(),
            "delete baseline\n"
        );
        assert!(!f.original().join("new.txt").exists());
        assert_eq!(f.index(), index);
    }
}
#[test]
fn external_edit_during_failure_is_preserved_and_never_masked_as_success() {
    let mut f = Fixture::new();
    f.edit();
    let plan = prepare(&f.task).unwrap(); // sorted order: delete.txt, main.txt, new.txt
    let result = f.fault(&plan, transaction::Fault::ExternalAfter(2));
    assert_eq!(
        result.status,
        TransactionStatus::RecoveryRequired,
        "{}",
        result.detail
    );
    assert_eq!(
        fs::read_to_string(f.original().join("main.txt")).unwrap(),
        "external concurrent edit\n"
    );
    assert_eq!(
        fs::read_to_string(f.original().join("delete.txt")).unwrap(),
        "delete baseline\n"
    );
    let claim = crate::tasks::store::lock(&f.home, &f.task.id).unwrap();
    let retried = recover_owned(
        &f.home,
        &claim,
        &f.task,
        &result.transaction_id,
        RecoveryAction::RollBack,
    )
    .unwrap();
    assert_eq!(retried.status, TransactionStatus::RecoveryRequired);
    assert_eq!(
        fs::read_to_string(f.original().join("main.txt")).unwrap(),
        "external concurrent edit\n"
    );
}
#[test]
fn interruption_inspection_is_nonmutating_and_recovery_requires_explicit_action() {
    let mut f = Fixture::new();
    f.edit();
    let plan = prepare(&f.task).unwrap();
    let result = f.fault(&plan, transaction::Fault::InterruptAfter(2));
    assert_eq!(result.status, TransactionStatus::Interrupted);
    let found = inspect_recovery(&f.home, &f.task).unwrap();
    assert_eq!(found[0].status, TransactionStatus::Interrupted);
    assert_eq!(
        fs::read_to_string(f.original().join("main.txt")).unwrap(),
        "builder\n"
    );
    let claim = crate::tasks::store::lock(&f.home, &f.task.id).unwrap();
    let recovered = recover_owned(
        &f.home,
        &claim,
        &f.task,
        &result.transaction_id,
        RecoveryAction::RollBack,
    )
    .unwrap();
    assert_eq!(
        recovered.status,
        TransactionStatus::RolledBack,
        "{}",
        recovered.detail
    );
    assert_eq!(
        fs::read_to_string(f.original().join("main.txt")).unwrap(),
        "baseline\n"
    );
    assert!(f.original().join("delete.txt").exists());
    let again = recover_owned(
        &f.home,
        &claim,
        &f.task,
        &result.transaction_id,
        RecoveryAction::RollBack,
    )
    .unwrap();
    assert_eq!(again.status, TransactionStatus::RolledBack);
}
#[test]
fn legacy_baseline_tampering_binary_modes_and_staged_only_are_blocked() {
    let mut f = Fixture::new();
    f.edit();
    let receipt = f.task.preparation_receipt.take().unwrap();
    assert!(prepare(&f.task).unwrap_err().contains("receipt"));
    f.task.preparation_receipt = Some(receipt.clone());
    fs::write(receipt.directory.join("baseline.json"), "tampered").unwrap();
    assert!(prepare(&f.task).is_err());
    let mut f = Fixture::new();
    fs::write(f.candidate().join("main.txt"), [0, 255]).unwrap();
    f.review();
    let plan = prepare(&f.task).unwrap();
    assert!(!plan.blockers.is_empty());
    assert!(apply(&f.home, &f.task, &plan).is_err());
    git_ok(f.candidate(), &["add", "main.txt"]);
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
#[test]
fn writer_claim_must_be_actual_correct_exclusive_handle_and_authoritative_record() {
    let mut f = Fixture::new();
    f.edit();
    let plan = prepare(&f.task).unwrap();
    let path = f.home.join(format!("{}.lock", f.task.id));
    let _ = crate::tasks::store::lock(&f.home, &f.task.id).unwrap();
    let unlocked = fs::File::open(path).unwrap();
    assert!(apply_owned(&f.home, &unlocked, &f.task, &plan).is_err());
    let unrelated = fs::File::open(f.home.join(format!("{}.json", f.task.id))).unwrap();
    assert!(apply_owned(&f.home, &unrelated, &f.task, &plan).is_err());
    let stale = f.task.clone();
    f.task.note = "new authority".into();
    crate::tasks::store::save(&f.home, &f.task).unwrap();
    assert!(apply(&f.home, &stale, &plan).is_err());
}
#[cfg(unix)]
#[test]
fn symlink_typechange_and_parent_swap_never_follow_external_links() {
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
    let mut f = Fixture::new();
    fs::remove_file(f.candidate().join("main.txt")).unwrap();
    fs::create_dir(f.candidate().join("main.txt")).unwrap();
    fs::write(f.candidate().join("main.txt/child"), "typechange\n").unwrap();
    f.review();
    assert!(prepare(&f.task).is_err());
    let f = Fixture::new();
    let outside = f.home.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "untouched\n").unwrap();
    symlink(&outside, f.original().join("linked")).unwrap();
    assert!(parent_available_test(f.original(), "linked/secret.txt").is_err());
    assert_eq!(
        fs::read_to_string(outside.join("secret.txt")).unwrap(),
        "untouched\n"
    );
}
#[cfg(unix)]
fn parent_available_test(root: &Path, path: &str) -> Result<(), String> {
    if transaction::parent_available(root, path) {
        Ok(())
    } else {
        Err("refused".into())
    }
}
#[test]
fn subfolder_scope_captures_root_payload_and_identity() {
    let mut f = Fixture::new();
    fs::create_dir(f.original().join("sub")).unwrap();
    fs::create_dir(f.candidate().join("sub")).unwrap();
    f.task.source = f.original().join("sub");
    f.task.workspace = Some(f.candidate().join("sub"));
    f.task.preparation_receipt = Some(
        capture_receipt(
            &f.task.source,
            f.task.workspace.as_ref().unwrap(),
            &f.task.base_head,
            &f.task.id,
            &f.home,
        )
        .unwrap(),
    );
    let candidate_root = workspace::fingerprint_root(f.candidate()).unwrap();
    fs::write(candidate_root.join("main.txt"), "builder at root\n").unwrap();
    f.review();
    let plan = prepare(&f.task).unwrap();
    assert!(plan.blockers.is_empty());
    assert_eq!(
        apply(&f.home, &f.task, &plan).unwrap().status,
        TransactionStatus::Applied
    );
}
