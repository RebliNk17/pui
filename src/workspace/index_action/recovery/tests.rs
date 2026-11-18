use super::*;
#[test]
fn reviewed_restore_refuses_changed_target_backup_or_index_and_preserves_current_bytes() {
    for changed in 0..4 {
        let root = fs::canonicalize(crate::workspace::tests::fixture()).unwrap();
        let cancel = AtomicBool::new(false);
        let discard = prepare(&root, "README.md", " M", Action::Discard, &cancel).unwrap();
        let recovery = discard.recovery_path();
        super::super::apply(discard, &cancel).unwrap();
        let review = inspect(&root, "README.md", &recovery, &cancel).unwrap();
        let current = fs::read(root.join("README.md")).unwrap();
        match changed {
            1 => fs::write(root.join("README.md"), "concurrent").unwrap(),
            2 => fs::write(&recovery, "tampered").unwrap(),
            3 => {
                let p = root.join(".git/index");
                let mut b = fs::read(&p).unwrap();
                b.push(0);
                fs::write(p, b).unwrap();
            }
            _ => {}
        }
        if changed == 0 {
            let backup = review
                .index_path
                .with_file_name(recovery_name("README.md", &current));
            apply(review, &cancel).unwrap();
            assert_eq!(fs::read(backup).unwrap(), current);
            assert!(recovery.is_file());
        } else {
            let before = fs::read(root.join("README.md")).unwrap();
            assert!(apply(review, &cancel).is_err());
            assert_eq!(fs::read(root.join("README.md")).unwrap(), before);
        }
    }
}
