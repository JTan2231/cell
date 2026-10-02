use krisis_api::lifecycle::{DecisionEventEnvelope, DecisionEventPage};

#[test]
fn retained_version_one_envelopes_decode_through_the_provider_export()
-> Result<(), Box<dyn std::error::Error>> {
    let raw = r#"{"stream":"decisions.lifecycle","envelope_version":1,"after_cursor":"opaque-before","next_cursor":"opaque-event","watermark_cursor":"opaque-watermark","has_more":false,"events":[{"cursor":"opaque-event","event":{"event_id":"de_review_1_v1","event_version":1,"event_kind":"decision_reviewed","occurred_at":2,"decision":{"decision_id":"d1","decided_at":1,"timestamp_precision":"item","statement":"Use stable identities.","disposition":"adopted","confidence":"high","rationale":null,"supersedes_decision_id":null,"review_state":"confirmed","authority_span":{"start":0,"end":5},"sources":[{"source_role":"authority","host_id":"h","thread_id":"t","turn_id":"u","item_id":"i","message_role":"user","occurred_at":1,"timestamp_precision":"item"}]},"review":{"review_id":"r_1","action":"confirm","reviewed_at":2,"review_source":"cli"}}}]}"#;
    let provider: DecisionEventPage<serde_json::Value> = serde_json::from_str(raw)?;
    let consumer: DecisionEventPage<DecisionEventEnvelope> =
        serde_json::from_slice(&serde_json::to_vec(&provider)?)?;
    assert_eq!(consumer.events[0].event.decision.sources[0].item_id, "i");
    assert_eq!(
        consumer.events[0]
            .event
            .review
            .as_ref()
            .map(|review| review.review_id.as_str()),
        Some("r_1")
    );
    assert_eq!(
        serde_json::to_value(consumer)?,
        serde_json::from_str::<serde_json::Value>(raw)?
    );
    Ok(())
}
