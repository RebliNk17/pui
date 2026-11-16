use super::*;
fn task() -> Task {
    Task::new(
        "/fixture".into(),
        "provider".into(),
        "model".into(),
        "off".into(),
    )
}
#[test]
fn bounded_user_note_captures_observed_phase_without_state_transition() {
    let mut task = task();
    task.phase = Phase::Blocked;
    let before = task.phase;
    task.add_checkpoint("  literal <script> 世界\nnext step  ")
        .unwrap();
    assert_eq!(task.phase, before);
    assert_eq!(task.checkpoints[0].phase, before);
    assert_eq!(task.checkpoints[0].text, "literal <script> 世界\nnext step");
    assert!(task.runs.is_empty());
    for _ in 1..32 {
        task.add_checkpoint("note").unwrap();
    }
    assert!(task.add_checkpoint("overflow").is_err());
    for text in ["".to_owned(), "x".repeat(2049), "bad\0control".into()] {
        assert!(self::task().add_checkpoint(&text).is_err());
    }
}
#[test]
fn summary_is_latest_bounded_unicode_literal_and_not_a_task_status() {
    let mut task = task();
    assert!(task.checkpoint_summary().is_none());
    task.add_checkpoint("older").unwrap();
    task.add_checkpoint(&format!("{}\nline\ttail", "世".repeat(130)))
        .unwrap();
    let phase = task.phase;
    let summary = task.checkpoint_summary().unwrap();
    assert!(summary.starts_with("User note · "));
    assert!(summary.ends_with('…'));
    assert!(summary.chars().count() < 140);
    assert!(!summary.contains('\n'));
    assert_eq!(task.phase, phase);
}
#[test]
fn saved_checkpoint_roundtrip_and_legacy_defaults_restore_data_not_execution() {
    let mut task = task();
    task.add_checkpoint("handoff").unwrap();
    let bytes = serde_json::to_vec(&task).unwrap();
    let restored: Task = serde_json::from_slice(&bytes).unwrap();
    restored.validate_checkpoints().unwrap();
    assert_eq!(restored.checkpoints[0].text, "handoff");
    assert!(restored.runs.is_empty());
    let mut value = serde_json::to_value(&task).unwrap();
    value.as_object_mut().unwrap().remove("checkpoints");
    let legacy: Task = serde_json::from_value(value).unwrap();
    assert!(legacy.checkpoints.is_empty());
    let home = std::env::temp_dir().join(format!("pui-checkpoints-{}", uuid::Uuid::new_v4()));
    crate::tasks::store::save(&home, &task).unwrap();
    assert!(
        std::fs::read(home.join(format!("{}.json", task.id)))
            .unwrap()
            .windows(7)
            .any(|w| w == b"handoff")
    );
}
