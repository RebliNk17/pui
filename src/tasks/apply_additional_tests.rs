use super::*;
#[test]
fn normal_apply_adds_no_untracked_recovery_artifacts_and_second_workflow_can_prepare() {
    let mut f = Fixture::new();
    f.edit();
    let plan = prepare(&f.task).unwrap();
    let result = apply(&f.home, &f.task, &plan).unwrap();
    assert_eq!(
        result.status,
        TransactionStatus::Applied,
        "{}",
        result.detail
    );
    assert_eq!(
        git(
            f.original(),
            &["ls-files", "--others", "--exclude-standard", "-z"]
        )
        .unwrap(),
        b"new.txt\0"
    );
    let head = f.task.base_head.clone();
    let (candidate, _) = workspace::prepare(
        f.original(),
        "second",
        &f.home.join("second-worktrees"),
        &head,
    )
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    assert!(capture_receipt(f.original(), &candidate, &head, &id, &f.home).is_ok());
}
#[test]
fn unchanged_binary_baseline_is_allowed_and_binary_builder_edits_are_not() {
    let mut f = Fixture::new();
    fs::write(f.original().join("image.png"), [0, 255, 10, 123]).unwrap();
    fs::write(f.candidate().join("image.png"), [0, 255, 10, 123]).unwrap();
    f.task.preparation_receipt = Some(
        capture_receipt(
            f.original(),
            f.candidate(),
            &f.task.base_head,
            &f.task.id,
            &f.home,
        )
        .unwrap(),
    );
    fs::write(f.candidate().join("main.txt"), "text builder\n").unwrap();
    f.review();
    let plan = prepare(&f.task).unwrap();
    assert!(plan.blockers.is_empty());
    assert_eq!(
        apply(&f.home, &f.task, &plan).unwrap().status,
        TransactionStatus::Applied
    );
}
#[test]
fn executable_mode_change_is_explicitly_blocked() {
    use std::os::unix::fs::PermissionsExt;
    let mut f = Fixture::new();
    fs::set_permissions(
        f.candidate().join("main.txt"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    f.review();
    assert!(
        prepare(&f.task)
            .unwrap()
            .blockers
            .iter()
            .any(|b| b.contains("Mode changes"))
    );
}
#[test]
fn candidate_head_drift_and_wrong_worktree_identity_block() {
    let mut f = Fixture::new();
    f.edit();
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
            "fixture head drift",
        ],
    );
    assert!(apply(&f.home, &f.task, &plan).is_err());
    let other = Fixture::new();
    f.task.workspace = Some(other.candidate().into());
    f.task.base_head = other.task.base_head.clone();
    f.review();
    assert!(prepare(&f.task).is_err());
}
#[test]
fn unrelated_handle_to_locked_claim_is_not_authority() {
    let mut f = Fixture::new();
    f.edit();
    let plan = prepare(&f.task).unwrap();
    let claim = crate::tasks::store::lock(&f.home, &f.task.id).unwrap();
    let impostor = fs::File::open(f.home.join(format!("{}.lock", f.task.id))).unwrap();
    assert!(apply_owned(&f.home, &impostor, &f.task, &plan).is_err());
    assert_eq!(
        apply_owned(&f.home, &claim, &f.task, &plan).unwrap().status,
        TransactionStatus::Applied
    );
}
#[test]
fn retained_rollback_of_applied_transaction_preserves_external_deletion() {
    let mut f = Fixture::new();
    f.edit();
    let plan = prepare(&f.task).unwrap();
    let result = apply(&f.home, &f.task, &plan).unwrap();
    assert_eq!(result.status, TransactionStatus::Applied);
    fs::remove_file(f.original().join("main.txt")).unwrap();
    let claim = crate::tasks::store::lock(&f.home, &f.task.id).unwrap();
    let recovered = recover_owned(
        &f.home,
        &claim,
        &f.task,
        &result.transaction_id,
        RecoveryAction::RollBack,
    )
    .unwrap();
    assert_eq!(recovered.status, TransactionStatus::RecoveryRequired);
    assert!(!f.original().join("main.txt").exists());
}
#[test]
fn malformed_recovery_operands_and_mismatched_transaction_refuse_without_mutation() {
    let mut f = Fixture::new();
    f.edit();
    let plan = prepare(&f.task).unwrap();
    let result = f.fault(&plan, transaction::Fault::InterruptAfter(2));
    let journal = result.retained_at.join("journal.json");
    let bytes = fs::read(&journal).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let before = workspace::fingerprint(f.original()).unwrap();
    let claim = crate::tasks::store::lock(&f.home, &f.task.id).unwrap();
    for (field, value) in [
        ("stage", "../outside"),
        ("backup", "/outside"),
        ("stage", "bad\0name"),
    ] {
        let mut malformed = original.clone();
        malformed["operations"][0][field] = serde_json::json!(value);
        fs::write(&journal, serde_json::to_vec(&malformed).unwrap()).unwrap();
        assert!(
            recover_owned(
                &f.home,
                &claim,
                &f.task,
                &result.transaction_id,
                RecoveryAction::RollBack
            )
            .is_err()
        );
        assert_eq!(workspace::fingerprint(f.original()).unwrap(), before);
    }
    let mut malformed = original.clone();
    malformed["transaction_id"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
    fs::write(&journal, serde_json::to_vec(&malformed).unwrap()).unwrap();
    assert!(
        recover_owned(
            &f.home,
            &claim,
            &f.task,
            &result.transaction_id,
            RecoveryAction::RollBack
        )
        .is_err()
    );
    assert_eq!(workspace::fingerprint(f.original()).unwrap(), before);
}
#[test]
fn interrupted_rollback_event_supersedes_old_applied_and_partial_latest_event_is_uncertain() {
    let mut f = Fixture::new();
    f.edit();
    let plan = prepare(&f.task).unwrap();
    let result = apply(&f.home, &f.task, &plan).unwrap();
    assert_eq!(
        inspect_recovery(&f.home, &f.task).unwrap()[0].status,
        TransactionStatus::Applied
    );
    let event = ApplyOutcome {
        transaction_id: result.transaction_id.clone(),
        status: TransactionStatus::Interrupted,
        detail: "rollback started before interruption".into(),
        retained_at: result.retained_at.clone(),
    };
    // Same ordered event that recover_owned syncs before its first mutation.
    transaction::append_event_for_test(&event);
    assert_eq!(
        inspect_recovery(&f.home, &f.task).unwrap()[0].status,
        TransactionStatus::Interrupted
    );
    fs::write(
        result.retained_at.join("event-00000000000000000002.json"),
        b"partial",
    )
    .unwrap();
    assert_eq!(
        inspect_recovery(&f.home, &f.task).unwrap()[0].status,
        TransactionStatus::Interrupted
    );
}
#[test]
fn recovery_history_scan_overflow_blocks_instead_of_omitting_uncertain_transactions() {
    let f = Fixture::new();
    let root = f.home.join("apply-v1");
    for i in 0..1001 {
        fs::write(root.join(format!("fixture-padding-{i}")), b"").unwrap();
    }
    assert!(
        inspect_recovery(&f.home, &f.task)
            .unwrap_err()
            .contains("scan budget")
    );
}
#[test]
fn missing_parent_addition_is_blocked_upfront() {
    let mut f = Fixture::new();
    fs::create_dir(f.candidate().join("newdir")).unwrap();
    fs::write(f.candidate().join("newdir/file.txt"), "new\n").unwrap();
    f.review();
    let plan = prepare(&f.task).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.contains("existing real parent"))
    );
    assert!(apply(&f.home, &f.task, &plan).is_err());
    assert!(!f.original().join("newdir").exists());
}
