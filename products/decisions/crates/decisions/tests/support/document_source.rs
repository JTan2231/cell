#![allow(clippy::expect_used)] // The static source fixture must decode successfully.

use conversations::Conversation;
use serde_json::json;

pub fn conversation() -> Conversation {
    serde_json::from_value(json!({
        "thread": {
            "reference": {"hostId": "host", "threadId": "thread"},
            "sessionId": null, "name": "Source fixture", "preview": "", "cwd": null,
            "sourceKind": "appServer", "parentThreadId": null, "forkedFromId": null,
            "cliVersion": null, "archived": false, "ephemeral": false,
            "createdAt": 1, "updatedAt": 4, "runtimeStatus": "idle"
        },
        "turns": [
            {
                "reference": {"hostId": "host", "threadId": "thread", "turnId": "earlier"},
                "startedAt": 1, "completedAt": 2, "status": "completed",
                "messages": [
                    {"reference": {"hostId": "host", "threadId": "thread", "turnId": "earlier", "itemId": "u1"},
                     "role": "user", "text": "What should we do?", "timestamp": null, "timestampPrecision": "unknown"},
                    {"reference": {"hostId": "host", "threadId": "thread", "turnId": "earlier", "itemId": "a1"},
                     "role": "assistant", "text": "Keep the API token in /tmp/config.\n\n```rust\nlet café = true;\n```\n", "timestamp": 2, "timestampPrecision": "turn"}
                ]
            },
            {
                "reference": {"hostId": "host", "threadId": "thread", "turnId": "target"},
                "startedAt": 3, "completedAt": 4, "status": "completed",
                "messages": [
                    {"reference": {"hostId": "host", "threadId": "thread", "turnId": "target", "itemId": "u2"},
                     "role": "user", "text": "Yes. No tests, builds, or formatters were run.", "timestamp": 3, "timestampPrecision": "item"},
                    {"reference": {"hostId": "host", "threadId": "thread", "turnId": "target", "itemId": "a2"},
                     "role": "assistant", "text": "Saved.\n# A source heading", "timestamp": 4, "timestampPrecision": "turn"}
                ]
            }
        ]
    })).expect("valid conversation fixture")
}
