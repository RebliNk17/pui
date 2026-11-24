use super::*;

fn snippet(label: &str, text: &str) -> Snippet {
    Snippet {
        label: label.into(),
        text: text.into(),
        ask: None,
    }
}

#[test]
fn captures_live_outside_the_draft_and_compose_after_typed_text() {
    let mut state = Draft::default();
    let mut draft = String::from("Can I hide it?");
    state
        .append(
            &draft,
            snippet("src/a.rs:1–2 · selected source", "fn a() {}"),
        )
        .unwrap();
    state
        .add_quote(snippet(
            "Assistant reply · selected rendered text (not Markdown source)",
            "it comes back",
        ))
        .unwrap();
    assert_eq!(
        draft, "Can I hide it?",
        "captures never enter the typed draft"
    );
    draft.push('!');
    state.clear(); // Editing call sites keep cards.
    assert!(state.has_quotes());
    assert_eq!(
        state.compose(&draft),
        "Can I hide it?!\n\n[Context: src/a.rs:1–2 · selected source]\nfn a() {}\n[/Context]\n\n[Context: Assistant reply · selected rendered text (not Markdown source)]\nit comes back\n[/Context]"
    );
    assert_eq!(
        state.compose("  "),
        "[Context: src/a.rs:1–2 · selected source]\nfn a() {}\n[/Context]\n\n[Context: Assistant reply · selected rendered text (not Markdown source)]\nit comes back\n[/Context]"
    );
    state.clear_quotes();
    assert_eq!(state.compose("x"), "x");
}

#[test]
fn restored_messages_turn_well_formed_blocks_back_into_cards() {
    let mut state = Draft::default();
    let rest = state.adopt_quotes(
        "[Context: Quoted assistant reply]\nwhole reply\n[/Context]\n\n[Context: Terminal · visible viewport]\n$ ls\n[/Context]\n\nWhy? [Context: inline] stays",
    );
    assert_eq!(
        state.labels(),
        vec!["Quoted assistant reply", "Terminal · visible viewport"]
    );
    assert_eq!(rest, "Why? [Context: inline] stays");
    let mut plain = Draft::default();
    assert_eq!(
        plain.adopt_quotes("[Context: x]\nunterminated"),
        "[Context: x]\nunterminated"
    );
    assert!(!plain.has_quotes());
}

#[test]
fn card_count_and_size_are_bounded_without_touching_existing_cards() {
    let mut state = Draft::default();
    for i in 0..MAX_CAPTURES {
        state
            .add_quote(snippet("label", &format!("body {i}")))
            .unwrap();
    }
    assert!(state.add_quote(snippet("label", "one more")).is_err());
    assert_eq!(state.quote_ids().len(), MAX_CAPTURES);
    let mut big = Draft::default();
    assert!(
        big.add_quote(snippet("label", &"x".repeat(MAX_DRAFT_BYTES)))
            .is_err()
    );
    assert!(!big.has_quotes());
}

#[test]
fn card_titles_are_short_and_typed() {
    assert_eq!(
        card_title("Assistant reply · selected rendered text").0,
        "Pi reply"
    );
    assert_eq!(
        card_title("src/main.rs:4–5 · selected source").0,
        "src/main.rs:4–5"
    );
    assert!(matches!(
        card_title("Terminal · visible viewport").1,
        Icon::Terminal
    ));
    assert!(matches!(
        card_title("Browser Console · https://x").1,
        Icon::Globe
    ));
}
