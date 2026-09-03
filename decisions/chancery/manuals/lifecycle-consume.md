# Durable decision lifecycle stream

Decisions schema version 3 owns an append-only consumer stream. Candidate
admission writes `decision_admitted` in the same transaction as the candidate,
its stable sources, authority verdicts, observation attachment, and observation
completion. `review confirm` and `review dismiss` each append
`decision_reviewed` in the same transaction as the review row, candidate state,
and digest invalidation. A failed event append rolls back the owning domain
change. Repeated observation of the same stable candidate does not append a
second admission event; every committed review action does append a review
event.

Capture a new consumer's activation point without replaying history:

```sh
decisions events watermark --json
```

The JSON object is:

```json
{"stream":"decisions.lifecycle","envelope_version":1,"cursor":"OPAQUE"}
```

Persist that cursor in consumer-owned state, then poll:

```sh
decisions events read --after OPAQUE --json
```

`--limit` defaults to 100 and accepts 1 through 1000. A page has this shape:

```json
{
  "stream": "decisions.lifecycle",
  "envelope_version": 1,
  "after_cursor": "OPAQUE",
  "next_cursor": "OPAQUE",
  "watermark_cursor": "OPAQUE",
  "has_more": false,
  "events": [
    {"cursor": "OPAQUE", "event": {"event_version": 1}}
  ]
}
```

Events are strictly after `after_cursor`, ascending in append order, and bounded
to the page's `watermark_cursor`. Each item cursor denotes the position just
after that event. A consumer should commit its domain receipt and that item
cursor atomically; after uncertainty, replay the prior cursor and deduplicate by
`event_id`. `next_cursor` is the last returned item cursor, or the unchanged
input cursor for an empty page. `has_more` means another event existed within
the captured watermark. Cursors are opaque and checksum-protected against
accidental fabrication, but they are not secrets or authority. Do not parse
them.

Each nested event envelope has:

```text
event_id
event_version             always 1
event_kind                decision_admitted | decision_reviewed
occurred_at
decision:
  decision_id
  decided_at
  timestamp_precision     item | turn
  statement
  disposition             adopt | reject | forbid | defer | delegate | reopen | supersede
  confidence              high | medium
  rationale               string or null
  supersedes_decision_id  string or null
  review_state             unreviewed | confirmed | dismissed at this event
  authority_span          {start, end}
  sources[]:
    source_role            authority | context
    host_id, thread_id, turn_id, item_id
    message_role           user | assistant
    occurred_at
    timestamp_precision    item | turn
review:                    null for admission
  review_id
  action                   confirm | dismiss
  reviewed_at
  review_source
```

The source array puts the authority source first, followed by context in stable
time/item order. It carries machine anchors, not source text. There is no
transcript, working directory, file path, diff, command, reasoning, approval,
tool output, hook body, prompt, or email data.

Schema-3 migration backfills all retained candidate admissions first in stable
creation order, then retained reviews in stable review order. The existing
candidates, sources, reviews, runs, observations, and delivery state are
preserved. Version-one consumers always activate at the current watermark;
historical replay is not exposed by this contract. The backfill preserves
complete stream state for future compatibility without creating an implicit
history-import path. The stream has no acknowledgement and is not pruned;
consumer success, retry, and retention belong to the consumer.

“Not pruned” describes the current stream implementation, not a perpetual
retention guarantee. This contract promises no wall-clock bound from a user
decision to admission, no throughput or polling service level, and no future
pruning, migration, or deprecation window. It also defines no network access
surface. These remain explicitly unspecified rather than inferred from the
SQLite schema or current behavior.
