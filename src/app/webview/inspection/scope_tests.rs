use super::super::transport::{self, State};
use super::*;
use serde_json::json;

fn tree() -> Value {
    json!({"frameTree":{"frame":{"id":"top","loaderId":"top-load","url":"https://example.test/","securityOrigin":"https://example.test"},"childFrames":[
        {"frame":{"id":"child","parentId":"top","loaderId":"child-load","url":"https://example.test/frame","securityOrigin":"https://example.test"}},
        {"frame":{"id":"cross","parentId":"top","loaderId":"cross-load","url":"https://other.test/","securityOrigin":"https://other.test"}},
        {"frame":{"id":"blank","parentId":"top","loaderId":"blank-load","url":"about:blank","securityOrigin":"https://example.test"}}
    ]}})
}
fn scope() -> Scope {
    Scope {
        frame_id: Some("child".into()),
        shadow_host: None,
    }
}
fn send(state: &mut State, id: i32, value: &Value) -> Option<transport::Command> {
    transport::receive(state, id, true, value.to_string().as_bytes())
}
fn refresh(state: &mut State) {
    let command = transport::inventory(state, state.epoch).unwrap();
    assert_eq!(command.method, "Page.getFrameTree");
    assert!(send(state, command.id, &tree()).is_none());
    assert_eq!(state.frames.len(), 2);
    assert_eq!(state.frames[1].label, "Same-origin frame 1");
}
fn begin(state: &mut State, selected: &Scope) -> transport::Command {
    let request = transport::command_scoped(state, state.epoch, "h1", selected).unwrap();
    let world = send(state, request.id, &tree()).unwrap();
    assert_eq!(world.params["frameId"], "child");
    assert_eq!(world.params["grantUniveralAccess"], false);
    let read = send(state, world.id, &json!({"executionContextId":4})).unwrap();
    assert_eq!(read.method, "Runtime.evaluate");
    assert!(
        read.params["expression"]
            .as_str()
            .unwrap()
            .ends_with(",\"https://example.test\")")
    );
    read
}
fn facts() -> Value {
    json!({"result":{"value":{"tag":"h1","text":"public","text_note":"bounded","generated_selector":"h1","selector_note":"not stable","rect":[0,0,1,1],"display":"block","visibility":"visible","position":"static","color":"black","font_size":"16px"}}})
}
#[test]
fn inventory_is_bounded_same_origin_explicit_and_refuses_ambiguous_inheritance() {
    let frames = inventory(&tree()).unwrap();
    assert_eq!(choices(&frames).len(), 2);
    assert_eq!(selected(&frames, &frames, &scope()), Some("child"));
    for id in ["cross", "blank", "missing"] {
        assert!(
            selected(
                &frames,
                &frames,
                &Scope {
                    frame_id: Some(id.into()),
                    shadow_host: None
                }
            )
            .is_none()
        );
    }
    assert!(selected(&frames, &[], &scope()).is_none());
    for field in ["id", "parentId", "loaderId", "url", "securityOrigin"] {
        let mut value = tree();
        value["frameTree"]["childFrames"][0]["frame"][field] = json!("changed");
        if let Some(changed) = inventory(&value) {
            assert!(selected(&changed, &frames, &scope()).is_none(), "{field}");
        }
    }
    let mut value = tree();
    value["frameTree"]["frame"]["loaderId"] = json!("new-parent");
    assert!(selected(&inventory(&value).unwrap(), &frames, &scope()).is_none());
    let mut value = tree();
    value["frameTree"]["childFrames"] = json!({"not":"an array"});
    assert!(inventory(&value).is_none());
    let mut value = tree();
    let child = value["frameTree"]["childFrames"][0].clone();
    value["frameTree"]["childFrames"] = json!(vec![child; 33]);
    assert!(inventory(&value).is_none());
    let mut nested = json!({"frame":{"id":"depth9","parentId":"depth8","loaderId":"l"}});
    for depth in (0..9).rev() {
        nested = json!({"frame":{"id":format!("depth{depth}"),"parentId":if depth == 0 {Value::Null} else {json!(format!("depth{}",depth-1))},"loaderId":"l"},"childFrames":[nested]});
    }
    assert!(inventory(&json!({"frameTree":nested})).is_none());
    assert!(
        !Scope {
            frame_id: Some("x\n".into()),
            shadow_host: None
        }
        .valid()
    );
    assert!(
        !Scope {
            frame_id: None,
            shadow_host: Some("[value]".into())
        }
        .valid()
    );
}
#[test]
fn scoped_transport_guards_inventory_origin_document_and_parent_identity_before_publishing() {
    let mut state = State::default();
    refresh(&mut state);
    let selected = scope();
    let read = begin(&mut state, &selected);
    let verify = send(&mut state, read.id, &facts()).unwrap();
    assert_eq!(verify.method, "Page.getFrameTree");
    assert!(state.facts.is_none());
    send(&mut state, verify.id, &tree());
    assert_eq!(state.facts.as_ref().unwrap().1.text, "public");
    assert_eq!(state.scope, selected);
    for field in ["loaderId", "securityOrigin"] {
        let read = begin(&mut state, &selected);
        let verify = send(&mut state, read.id, &facts()).unwrap();
        let mut changed = tree();
        changed["frameTree"]["frame"][field] = json!("changed");
        send(&mut state, verify.id, &changed);
        assert!(state.facts.is_none());
        assert!(state.message.as_ref().unwrap().contains("navigated"));
    }
}
#[test]
fn malformed_cancel_navigation_and_late_scope_responses_cannot_publish_or_retarget() {
    for stage in ["tree", "world", "read", "verify"] {
        let mut state = State::default();
        refresh(&mut state);
        let command = transport::command_scoped(&mut state, 0, "h1", &scope()).unwrap();
        let command = if stage == "tree" {
            command
        } else {
            let world = send(&mut state, command.id, &tree()).unwrap();
            if stage == "world" {
                world
            } else {
                let read = send(&mut state, world.id, &json!({"executionContextId":2})).unwrap();
                if stage == "read" {
                    read
                } else {
                    send(&mut state, read.id, &facts()).unwrap()
                }
            }
        };
        state.cancel(Some("Navigation"));
        assert!(state.frames.is_empty());
        assert_eq!(state.scope, Scope::default());
        assert!(send(&mut state, command.id, &tree()).is_none());
        assert!(state.facts.is_none());
        refresh(&mut state);
        let read = begin(&mut state, &scope());
        assert!(send(&mut state, command.id, &facts()).is_none());
        assert!(state.pending());
        transport::receive(
            &mut state,
            read.id,
            true,
            b"{\"result\":{\"value\":{\"error\":\"scope\"}}}",
        );
        assert!(state.facts.is_none());
        assert!(state.message.as_ref().unwrap().contains("scope"));
    }
    let mut state = State::default();
    let request = transport::command_scoped(&mut state, 0, "h1", &scope()).unwrap();
    assert!(send(&mut state, request.id, &tree()).is_none());
    assert!(state.message.as_ref().unwrap().contains("inventoried"));
    refresh(&mut state);
    let request = transport::inventory(&mut state, 0).unwrap();
    transport::receive(&mut state, request.id, true, &[b' '; 16385]);
    assert!(state.message.as_ref().unwrap().contains("16 KiB"));
    assert!(state.facts.is_none());
}
#[test]
fn scoped_picker_is_explicit_and_passes_shadow_and_origin_guards_to_native_reader() {
    let mut state = State::default();
    refresh(&mut state);
    let selected = Scope {
        shadow_host: Some("article.host".into()),
        ..scope()
    };
    let dom = transport::pick_scoped(&mut state, 0, &selected).unwrap();
    let overlay = send(&mut state, dom.id, &json!({})).unwrap();
    let arm = send(&mut state, overlay.id, &json!({})).unwrap();
    send(&mut state, arm.id, &json!({}));
    let frame = transport::event(
        &mut state,
        "Overlay.inspectNodeRequested",
        b"{\"backendNodeId\":7}",
    )
    .unwrap();
    let world = send(&mut state, frame.id, &tree()).unwrap();
    let resolve = send(&mut state, world.id, &json!({"executionContextId":9})).unwrap();
    assert_eq!(resolve.params["executionContextId"], 9);
    let read = send(
        &mut state,
        resolve.id,
        &json!({"object":{"objectId":"picked-node"}}),
    )
    .unwrap();
    let function = read.params["functionDeclaration"].as_str().unwrap();
    assert!(function.contains("(node,\"article.host\",\"https://example.test\")"));
    assert_eq!(read.params["executionContextId"], 9);
    assert_eq!(read.params["arguments"][0]["objectId"], "picked-node");
    assert!(read.params.get("objectId").is_none());
    assert!(function.contains("rootOf(selected) !== root"));
    assert!(function.contains("ownerDocument"));
    let verify = send(&mut state, read.id, &facts()).unwrap();
    send(&mut state, verify.id, &tree());
    assert!(state.facts.is_some());
    assert!(state.take_cleanup());
}
