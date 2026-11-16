use super::*;
#[cfg(unix)]
fn python(body: &str) -> Command {
    let mut command = Command::new("python3");
    command.args(["-c", body]);
    command
}
#[test]
#[cfg(unix)]
fn supervised_full_io_deadline_cancel_bad_protocol_and_budget_refusal() {
    let source = "x".repeat(MAX_SOURCE);
    for body in [
        "import time;time.sleep(30)",
        "import sys,time;sys.stdin.buffer.read(8);time.sleep(30)",
        "import sys,struct;sys.stdout.buffer.write(b'PSV1'+struct.pack('<II',1,1));sys.stdout.flush()",
        "import sys;sys.stdout.buffer.write(b'BAD');sys.stdout.flush()",
    ] {
        let start = Instant::now();
        assert!(
            run(
                &mut python(body),
                &source,
                None,
                |_| Ok(()),
                Duration::from_millis(100)
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    let cancel = AtomicBool::new(true);
    assert!(
        run(
            &mut python("raise Exception('must not run')"),
            "x",
            Some(&cancel),
            |_| Ok(()),
            Duration::from_secs(1)
        )
        .unwrap_err()
        .contains("cancelled")
    );
    let responder = "import sys,struct,time\nh=sys.stdin.buffer.read(8);n=struct.unpack('<I',h[4:])[0];sys.stdin.buffer.read(n);sys.stdout.buffer.write(b'PSV1'+struct.pack('<II',1,1));sys.stdout.flush();ack=sys.stdin.buffer.read(1);sys.stdout.buffer.write(b'\\x80\\0\\0\\x80');sys.stdout.flush()";
    let raster = run(
        &mut python(responder),
        "x",
        None,
        |len| {
            assert_eq!(len, 4);
            Ok(())
        },
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(raster.size, [1, 1]);
    assert_eq!(raster.rgba, [128, 0, 0, 128]);
    let mut reserved = false;
    let result = run(
        &mut python(responder),
        "x",
        None,
        |_| {
            reserved = true;
            Err("budget full".into())
        },
        Duration::from_secs(2),
    );
    assert!(reserved);
    assert_eq!(result.unwrap_err(), "budget full");
    for suffix in ["b''", "b'123'", "b'12345'"] {
        let body = responder.replace("b'\\x80\\0\\0\\x80'", suffix);
        assert!(
            run(
                &mut python(&body),
                "x",
                None,
                |_| Ok(()),
                Duration::from_secs(2)
            )
            .is_err()
        );
    }
}
