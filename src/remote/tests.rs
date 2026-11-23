use super::*;
use crate::protocol::{InlineVisual, Message, SlashCommand, Tool};
use std::{
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};

#[test]
fn sha1_matches_rfc3174_and_rfc6455_accept_key() {
    let hex = |bytes: [u8; 20]| -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() };
    assert_eq!(
        hex(ws::sha1(b"")),
        "da39a3ee5e6b4b0d3255bfef95601890afd80709"
    );
    assert_eq!(
        hex(ws::sha1(b"abc")),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    assert_eq!(
        ws::accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
    );
}

#[test]
fn websocket_frame_encode_and_masked_decode_roundtrip() {
    let payload = "hello from pui mobile 🚀";
    let mut masked = vec![0x81u8, 0x80 | (payload.len() as u8)];
    let mask = [0x12, 0x34, 0x56, 0x78];
    masked.extend_from_slice(&mask);
    for (i, b) in payload.as_bytes().iter().enumerate() {
        masked.push(b ^ mask[i % 4]);
    }
    let mut cursor = std::io::Cursor::new(masked);
    assert_eq!(
        ws::read_message(&mut cursor).unwrap(),
        ws::WsMessage::Text(payload.to_owned())
    );

    let encoded = ws::encode_frame(0x1, payload.as_bytes());
    let mut cursor2 = std::io::Cursor::new(encoded);
    assert_eq!(
        ws::read_message(&mut cursor2).unwrap(),
        ws::WsMessage::Text(payload.to_owned())
    );
}

#[test]
fn tailscale_ip_range_and_pairing_uri_work() {
    assert!(tailscale::is_tailscale_ipv4(Ipv4Addr::new(100, 64, 0, 1)));
    assert!(tailscale::is_tailscale_ipv4(Ipv4Addr::new(100, 101, 42, 7)));
    assert!(tailscale::is_tailscale_ipv4(Ipv4Addr::new(
        100, 127, 255, 254
    )));
    assert!(!tailscale::is_tailscale_ipv4(Ipv4Addr::new(
        100, 63, 255, 255
    )));
    assert!(!tailscale::is_tailscale_ipv4(Ipv4Addr::new(100, 128, 0, 1)));
    assert!(!tailscale::is_tailscale_ipv4(Ipv4Addr::new(
        192, 168, 1, 10
    )));

    let uri = tailscale::pairing_uri("100.88.12.3", 4817, "abc123", "Shai's Mac", false);
    assert!(uri.starts_with("pui://pair?v=1&host=100.88.12.3&port=4817&token=abc123"));
    assert!(uri.contains("name=Shai%27s%20Mac"));
    assert!(uri.ends_with("&tls=0"));
}

#[test]
fn qr_encoder_produces_valid_matrix_for_pairing_uri() {
    let token = generate_token();
    let uri = tailscale::pairing_uri("100.88.12.3", 4817, &token, "Shai MacBook Pro", false);
    let matrix = qr::encode(&uri).expect("pairing URI should fit in QR versions 1..=10");
    assert!(matrix.size >= 21 && (matrix.size - 17) % 4 == 0);
    assert_eq!(matrix.modules.len(), matrix.size * matrix.size);
    // Top-left finder pattern center (3,3) and corners (0,0), (6,6) must be dark.
    assert!(matrix.get(0, 0));
    assert!(matrix.get(3, 3));
    assert!(matrix.get(6, 6));
    assert!(!matrix.get(1, 1));
    // Dark module at (8, size - 8).
    assert!(matrix.get(8, matrix.size - 8));

    #[cfg(target_os = "macos")]
    {
        let quiet = 4usize;
        let scale = 6u32;
        let dim = ((matrix.size + quiet * 2) as u32) * scale;
        let mut img = image::GrayImage::from_pixel(dim, dim, image::Luma([255u8]));
        for y in 0..matrix.size {
            for x in 0..matrix.size {
                if matrix.get(x, y) {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            img.put_pixel(
                                ((x + quiet) as u32) * scale + dx,
                                ((y + quiet) as u32) * scale + dy,
                                image::Luma([0u8]),
                            );
                        }
                    }
                }
            }
        }
        let png_path =
            std::env::temp_dir().join(format!("pui-qr-verify-{}.png", uuid::Uuid::new_v4()));
        img.save(&png_path).unwrap();
        let output = std::process::Command::new("swift")
            .args([
                "-e",
                &format!(
                    r#"import Foundation; import CoreImage; let url = URL(fileURLWithPath: "{}"); let ci = CIImage(contentsOf: url)!; let det = CIDetector(ofType: CIDetectorTypeQRCode, context: nil, options: [CIDetectorAccuracy: CIDetectorAccuracyHigh])!; let feats = det.features(in: ci) as? [CIQRCodeFeature] ?? []; print(feats.first?.messageString ?? "DECODE_FAILED")"#,
                    png_path.display()
                ),
            ])
            .output()
            .expect("run swift qr decoder");
        let _ = std::fs::remove_file(&png_path);
        let decoded = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        assert_eq!(
            decoded, uri,
            "CoreImage must decode the generated QR code identically"
        );
    }
}

#[test]
fn session_origin_roundtrips_through_jsonl_entry() {
    let origin = SessionOrigin {
        platform: OriginPlatform::Android,
        device: Some("Pixel 9 Pro".into()),
        client: Some("pui-mobile".into()),
        created_at: Some(1_763_913_600_000),
    };
    let entry = origin.custom_jsonl_entry();
    let parsed = SessionOrigin::from_jsonl_entry(&entry).expect("parse origin");
    assert_eq!(parsed, origin);
    assert_eq!(parsed.badge_tooltip(), "Started from Android (Pixel 9 Pro)");
}

fn send_http(addr: SocketAddr, request: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect remote server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_owned())
        .unwrap_or_default();
    (status, body)
}

#[test]
fn live_server_auth_sessions_completions_files_and_ws_push() {
    let dir = std::env::temp_dir().join(format!("pui-remote-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.join("report.html"), "<h1>Hi</h1>").unwrap();
    let outside = std::env::temp_dir().join(format!("pui-outside-{}.txt", uuid::Uuid::new_v4()));
    std::fs::write(&outside, "secret").unwrap();

    let token = "test-secret-token-0123456789abcdef0123456789abcdef";
    let mut server = RemoteServer::new(
        RemoteConfig {
            enabled: false,
            bind_mode: BindMode::Localhost,
            port: 0,
            token: token.into(),
            host_name: "Test Mac".into(),
            use_tls_in_qr: false,
            public_host: String::new(),
        },
        || {},
    );
    // Start directly on ephemeral localhost port (0).
    server.config.enabled = true;
    server.config.port = 0;
    server.start_listener();
    let addr = server.bound_addr.expect("ephemeral bind");

    let mut msg = Message {
        role: "assistant".into(),
        text: "Done".into(),
        ..Default::default()
    };
    msg.tools.push(Tool {
        id: "t1".into(),
        name: "pui_visual".into(),
        visual: Some(InlineVisual {
            title: "Demo Card".into(),
            html: "<div>ok</div>".into(),
            height: Some(240),
        }),
        ..Default::default()
    });
    let (artifacts, allowed_artifacts) = extract_artifacts(&[msg.clone()]);
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].kind, "visual_html");

    let session_path = "/sessions/s1.jsonl".to_owned();
    let mut details = BTreeMap::new();
    details.insert(
        session_path.clone(),
        SessionDetail {
            path: session_path.clone(),
            title: "Phone chat".into(),
            cwd: dir.to_string_lossy().into_owned(),
            project: dir.to_string_lossy().into_owned(),
            status: "Idle".into(),
            ready: true,
            origin: SessionOrigin {
                platform: OriginPlatform::Android,
                device: Some("Pixel 9".into()),
                client: Some("pui-mobile".into()),
                created_at: None,
            },
            messages: vec![msg],
            commands: vec![SlashCommand {
                name: "review".into(),
                description: Some("Review code".into()),
            }],
            artifacts,
            revision: 1,
            ..Default::default()
        },
    );
    server.update_snapshot(RemoteSnapshot {
        revision: 1,
        host_name: "Test Mac".into(),
        tailscale_ip: Some("100.64.0.9".into()),
        active_path: Some(session_path.clone()),
        projects: vec![dir.clone()],
        sessions: vec![SessionSummary {
            path: session_path.clone(),
            title: "Phone chat".into(),
            project: dir.to_string_lossy().into_owned(),
            project_name: "pui-remote-test".into(),
            cwd: dir.to_string_lossy().into_owned(),
            status: "Idle".into(),
            busy: false,
            unread: false,
            has_pending_dialog: false,
            model: "claude-sonnet".into(),
            modified_ms: 1000,
            created_ms: Some(900),
            origin: SessionOrigin {
                platform: OriginPlatform::Android,
                device: Some("Pixel 9".into()),
                client: Some("pui-mobile".into()),
                created_at: None,
            },
            parent_session: None,
            is_active: true,
        }],
        details,
        allowed_artifacts,
        usage: crate::usage::CachedUsage {
            providers: vec![crate::usage::ProviderUsage {
                id: "anthropic".into(),
                name: "Claude".into(),
                plan: Some("Max".into()),
                windows: vec![crate::usage::UsageWindow {
                    label: "Current session".into(),
                    used: 24.0,
                    resets: Some(1_763_920_000_000.0),
                    detail: None,
                }],
                models: Vec::new(),
                notes: Vec::new(),
                error: None,
            }],
            ..Default::default()
        },
    });

    // 1. Unauthenticated /v1/health succeeds, /v1/sessions fails with 401.
    let (code, body) = send_http(addr, "GET /v1/health HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert_eq!(code, 200);
    assert!(body.contains("\"protocol_version\":1"));

    let (code, _) = send_http(addr, "GET /v1/sessions HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert_eq!(code, 401);

    // 2. Authenticated /v1/sessions returns Android origin.
    let (code, body) = send_http(
        addr,
        &format!(
            "GET /v1/sessions HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\r\n"
        ),
    );
    assert_eq!(code, 200);
    assert!(body.contains("\"platform\":\"android\""));
    assert!(body.contains("Pixel 9"));

    // 2b. Authenticated /v1/usage returns per-provider rate limits.
    let (code, body) = send_http(
        addr,
        &format!(
            "GET /v1/usage HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\r\n"
        ),
    );
    assert_eq!(code, 200);
    assert!(body.contains("\"name\":\"Claude\""));
    assert!(body.contains("\"label\":\"Current session\""));

    // 3. Slash command completions include both Pui local commands and Pi commands.
    let (code, body) = send_http(
        addr,
        &format!(
            "GET /v1/completions/commands?path={}&q= HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\r\n",
            http::percent_encode(&session_path)
        ),
    );
    assert_eq!(code, 200);
    assert!(body.contains("\"name\":\"compact\""));
    assert!(body.contains("\"name\":\"review\""));

    // 4. @ file mention completions & file tree & traversal rejection.
    let cwd_q = http::percent_encode(&dir.to_string_lossy());
    let (code, body) = send_http(
        addr,
        &format!(
            "GET /v1/completions/files?cwd={cwd_q}&q=main HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\r\n"
        ),
    );
    assert_eq!(code, 200);
    assert!(body.contains("src/main.rs"));

    let (code, body) = send_http(
        addr,
        &format!(
            "GET /v1/files/content?cwd={cwd_q}&path=src/main.rs HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\r\n"
        ),
    );
    assert_eq!(code, 200);
    assert!(body.contains("fn main()"));

    let (code, _) = send_http(
        addr,
        &format!(
            "GET /v1/files/content?cwd={cwd_q}&path=../{} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\r\n",
            outside.file_name().unwrap().to_string_lossy()
        ),
    );
    assert_eq!(code, 403);

    // 5. POST /v1/sessions queues CreateSession with Android origin.
    let payload = json!({
        "cwd": dir.to_string_lossy(),
        "permission_mode": "auto",
        "prompt": "Hello from phone",
        "origin": { "platform": "android", "device": "Pixel 9", "client": "pui-mobile" }
    })
    .to_string();
    let (code, _) = send_http(
        addr,
        &format!(
            "POST /v1/sessions HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}",
            payload.len()
        ),
    );
    assert_eq!(code, 202);
    match server.cmds.try_recv().expect("queued create command") {
        RemoteCommand::CreateSession { origin, prompt, .. } => {
            assert_eq!(origin.platform, OriginPlatform::Android);
            assert_eq!(origin.device.as_deref(), Some("Pixel 9"));
            assert_eq!(prompt.as_deref(), Some("Hello from phone"));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    // 6. WebSocket handshake receives hello + initial sessions_updated.
    let mut ws_stream = TcpStream::connect(addr).unwrap();
    ws_stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let upgrade = format!(
        "GET /v1/ws?token={token} HTTP/1.1\r\n\
         Host: localhost\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    );
    ws_stream.write_all(upgrade.as_bytes()).unwrap();
    let mut hdr = Vec::new();
    let mut b = [0u8; 1];
    while !hdr.ends_with(b"\r\n\r\n") {
        ws_stream.read_exact(&mut b).unwrap();
        hdr.push(b[0]);
    }
    let hdr_str = String::from_utf8_lossy(&hdr);
    assert!(hdr_str.contains("101 Switching Protocols"));
    assert!(hdr_str.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));

    let msg1 = ws::read_message(&mut ws_stream).unwrap();
    let msg2 = ws::read_message(&mut ws_stream).unwrap();
    match (msg1, msg2) {
        (ws::WsMessage::Text(hello), ws::WsMessage::Text(sessions)) => {
            assert!(hello.contains("\"type\":\"hello\""));
            assert!(sessions.contains("\"type\":\"sessions_updated\""));
            assert!(sessions.contains("Phone chat"));
        }
        other => panic!("unexpected WS messages: {other:?}"),
    }

    server.stop_listener();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&outside);
}
