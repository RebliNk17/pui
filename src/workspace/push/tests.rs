use super::*;
fn fixture() -> PathBuf {
    let root = std::fs::canonicalize(super::super::tests::fixture()).unwrap();
    let branch = String::from_utf8(super::super::tests::git(
        &root,
        &["symbolic-ref", "--short", "HEAD"],
    ))
    .unwrap();
    super::super::tests::git(
        &root,
        &[
            "config",
            "remote.origin.url",
            "git@example.invalid:team/repo.git",
        ],
    );
    super::super::tests::git(
        &root,
        &[
            "config",
            &format!("branch.{}.remote", branch.trim()),
            "origin",
        ],
    );
    super::super::tests::git(
        &root,
        &[
            "config",
            &format!("branch.{}.merge", branch.trim()),
            "refs/heads/master",
        ],
    );
    root
}
#[test]
fn local_review_exact_destination_without_remote_contact_or_repo_mutation() {
    let root = fixture();
    let before = super::super::tests::snapshot(&root);
    let s = prepare(&root, &AtomicBool::new(false)).unwrap();
    assert_eq!(s.remote, "origin");
    assert_eq!(s.destination, "refs/heads/master");
    assert_eq!(s.endpoint, "git@example.invalid:team/repo.git");
    assert_eq!(super::super::tests::snapshot(&root), before);
}
#[test]
fn custom_routing_multiple_endpoints_and_embedded_credentials_refuse() {
    for case in 0..4 {
        let root = fixture();
        match case {
            0 => {
                super::super::tests::git(&root, &["config", "remote.pushDefault", "other"]);
            }
            1 => {
                super::super::tests::git(
                    &root,
                    &[
                        "config",
                        "--add",
                        "remote.origin.pushurl",
                        "https://one.invalid/repo",
                    ],
                );
                super::super::tests::git(
                    &root,
                    &[
                        "config",
                        "--add",
                        "remote.origin.pushurl",
                        "https://two.invalid/repo",
                    ],
                );
            }
            2 => {
                super::super::tests::git(
                    &root,
                    &[
                        "config",
                        "remote.origin.url",
                        "https://secret-token@example.invalid/repo",
                    ],
                );
            }
            _ => {
                super::super::tests::git(&root, &["config", "remote.origin.mirror", "true"]);
            }
        }
        assert!(prepare(&root, &AtomicBool::new(false)).is_err());
    }
}
#[test]
fn transport_policy_refuses_helpers_queries_and_passwords() {
    for bad in [
        "ext::touch marker",
        "file:///tmp/repo",
        "https://token@host/path",
        "ssh://user:password@host/path",
        "https://host/path?token=secret",
        "git@host:path#secret",
    ] {
        assert!(endpoint(bad).is_err());
    }
    for good in [
        "https://host/team/repo.git",
        "ssh://host/team/repo.git",
        "git@host:team/repo.git",
    ] {
        assert_eq!(endpoint(good).unwrap(), good);
    }
}
