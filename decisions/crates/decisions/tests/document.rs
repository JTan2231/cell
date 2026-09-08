#![allow(clippy::unwrap_used)] // Fixture setup and expected successful operations must fail the test immediately.

#[path = "support/document_source.rs"]
mod support;

use decisions::document::{Classification, Error, MAX_PROMPT_BYTES, Snapshot};
use serde_json::{Value, json};

#[test]
fn validation_checks_shape_and_header_format_without_content_vetoes() {
    for summary in [
        "No tests, builds, or formatters were run.",
        "Store the API token in /Users/joey/config; contact joey@example.com.",
        "The Moon is made of cheese. This is a second sentence. Here is a third.",
        "Use **Krisis** & <Annals>.",
        &"é".repeat(1_000),
    ] {
        Classification::parse(&json!({"is_decision": true, "summary": summary}).to_string())
            .unwrap();
    }
    for input in [
        r#"{"is_decision":false}"#,
        r#"{"is_decision":"true","summary":"Yes."}"#,
        r#"{"is_decision":true,"summary":null}"#,
        r#"{"is_decision":true,"summary":" "}"#,
        r#"{"is_decision":true,"summary":"First\nsecond"}"#,
        r#"{"is_decision":false,"summary":"No."}"#,
        r#"{"is_decision":false,"summary":null,"context":null}"#,
        r#"{"is_decision":false,"is_decision":true,"summary":null}"#,
    ] {
        assert!(Classification::parse(input).is_err(), "accepted {input}");
    }
    assert!(
        Classification::parse(
            &json!({"is_decision": true, "summary": "é".repeat(1_001)}).to_string()
        )
        .is_err()
    );
}

#[test]
fn snapshot_and_document_preserve_all_source_text_and_turn_order() {
    let source = support::conversation();
    let snapshot = Snapshot::capture(source.clone(), "target").unwrap();
    let prompt: Value = serde_json::from_str(&snapshot.prompt().unwrap()).unwrap();
    assert_eq!(prompt["conversation"][0]["classify_this_exchange"], false);
    assert_eq!(prompt["conversation"][1]["classify_this_exchange"], true);
    let classification = Classification {
        is_decision: true,
        summary: Some("Use **Krisis** & <Annals>.".to_owned()),
    };
    let markdown = snapshot.render(&classification).unwrap().unwrap();
    assert!(
        markdown.starts_with("# Use \\*\\*Krisis\\*\\* \\& \\<Annals\\>\\.\n\n## Conversation\n")
    );
    let mut offset = 0;
    for (turn_index, turn) in source.turns.iter().enumerate() {
        for (message_index, message) in turn.messages.iter().enumerate() {
            assert_eq!(
                prompt["conversation"][turn_index]["messages"][message_index]["text"],
                message.text
            );
            let quoted = format!("> {}\n", message.text.replace('\n', "\n> "));
            let relative = markdown[offset..].find(&quoted).unwrap();
            offset += relative + quoted.len();
        }
    }
    assert!(!markdown.contains("Unknown."));
    let restored = Snapshot::parse(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    assert_eq!(restored.render(&classification).unwrap(), Some(markdown));
}

#[test]
fn negative_verdict_has_no_artifact() {
    let snapshot = Snapshot::capture(support::conversation(), "target").unwrap();
    let classification = Classification::parse(r#"{"is_decision":false,"summary":null}"#).unwrap();
    assert_eq!(snapshot.render(&classification).unwrap(), None);
}

#[test]
fn later_history_is_excluded_and_source_failures_are_not_negative_verdicts() {
    let source = support::conversation();
    let snapshot = Snapshot::capture(source.clone(), "earlier").unwrap();
    assert_eq!(snapshot.conversation().turns.len(), 1);
    assert!(!snapshot.prompt().unwrap().contains("Saved."));
    assert!(Snapshot::capture(source.clone(), "absent").is_err());
    let mut incomplete = source.clone();
    incomplete.turns[1].completed_at = None;
    assert!(Snapshot::capture(incomplete, "target").is_err());
    let mut duplicate = source.clone();
    duplicate.turns.push(source.turns[1].clone());
    assert!(Snapshot::capture(duplicate, "target").is_err());
    let mut wrong_identity = source.clone();
    wrong_identity.turns[0].messages[0].reference.thread_id = "other".to_owned();
    assert!(Snapshot::capture(wrong_identity, "target").is_err());
    let mut child = source;
    child.thread.parent_thread_id = Some("parent".to_owned());
    assert!(Snapshot::capture(child, "target").is_err());
}

#[test]
fn history_is_not_cut_to_the_old_message_or_text_limits() {
    let mut source = support::conversation();
    let template = source.turns[0].messages[0].clone();
    for index in 0..100 {
        let mut message = template.clone();
        message.reference.item_id = format!("extra-{index}");
        source.turns[0].messages.push(message);
    }
    let snapshot = Snapshot::capture(source.clone(), "target").unwrap();
    assert_eq!(snapshot.conversation().turns[0].messages.len(), 102);
    let prompt: Value = serde_json::from_str(&snapshot.prompt().unwrap()).unwrap();
    assert_eq!(
        prompt["conversation"][0]["messages"]
            .as_array()
            .unwrap()
            .len(),
        102
    );
    source.turns[0].messages[0].text = "x".repeat(MAX_PROMPT_BYTES);
    let snapshot = Snapshot::capture(source, "target").unwrap();
    assert!(matches!(
        snapshot.prompt(),
        Err(Error::PromptTooLarge { .. })
    ));
    assert_eq!(
        snapshot.conversation().turns[0].messages[0].text.len(),
        MAX_PROMPT_BYTES
    );
}
