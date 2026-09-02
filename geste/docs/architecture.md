# Architecture

Geste is a manually populated, local casebook for process precedent. Its
future-facing read loop is:

```text
incoming request
  -> agent writes a short structural query
  -> Geste returns lexical precedent candidates
  -> agent inspects report, graph, source basis, gaps, and applicability
  -> agent checks current operating contracts
  -> agent decides whether and how to reuse the precedent
```

The agent, not Geste, owns semantic analogy. Geste uses no model, embeddings,
vector database, synonym service, or Nucleus job.

## Authority

An episode is one deliberately bounded historical case. Geste owns its stable
`eN` identity, immutable revisions, authored shape and report fields,
settlement status as represented under Geste's validation rules, explicit
coverage gaps, source anchors, related-episode links, and read projections.

Its precise claim is limited:

> At the recorded cutoff, the named recorder supplied these source anchors and
> authored this episode revision's interpretation.

Geste does not establish that an upstream record exists, remains current, or
means what the report says. Conversations owns normalized conversation
history; Decisions owns enacted settlements and review; Semantics owns
maintained terminology; Annals owns retained works and evidence-grounded
concepts; Git owns repository history; Chancery presents current installed
contracts; Nucleus owns execution records but not requester domain success;
Todo owns later concerns and routing; each other product keeps its documented
domain authority.

Geste retains locators and optional revisions or digests, never upstream source
bodies. Locator-only anchors are visibly warned because they cannot freeze
mutable upstream state. All prose outside the structured settlement list is
labeled Geste-authored interpretation.

The source is selected by the claim being made:

| Source | Consult when | Geste keeps | Boundary |
| --- | --- | --- | --- |
| Conversations | Recovering what was said or locating a task or turn | Stable host/thread/turn/item locator and observation time | It cannot establish a decision or repository effect. |
| Decisions | Calling a choice an enacted or reviewed settlement | Event/decision locator and observed status | File activity or assistant prose cannot substitute. |
| Semantics | Using maintained project meaning | Project, concept, and exact semantic revision | HEAD without a revision is mutable; source code remains behavior authority. |
| Annals | Citing a retained work or evidence-grounded concept | Work digest or concept plus corpus revision | It is not current procedure or an action backlog. |
| Git | Establishing exact source or release effects | Repository plus full commit, tree, or tag-object ID; a tag name also needs its peeled object ID or a source digest | A movable tag name is not immutable and Git does not prove rationale, authorization, causality, or domain success. |
| Chancery | Establishing the operation contract used | Entry, provider release, and contract version | It does not prove readiness, execution, authorization, or success. |
| Nucleus | When one execution attempt materially matters | Job and bounded output locator | Runtime completion is not requester domain success or retry authority. |
| Todo | Linking the originating concern or remaining follow-up | Concern, routing, or todo identity and observed state | A pending proposal is not an accepted route or completed action. |
| Weaver or another product | Citing its domain artifact or state | Product-qualified identity and immutable revision/digest when available | That product's documented success rule remains authoritative. |

## Manual capture

The interactive agent uses Chancery to select and read relevant source
interfaces, authors a strict full-snapshot JSON document, and invokes
`episode create` or `episode revise`. The Geste process itself calls no Cell
product. This keeps the casebook readable when a source is unavailable and
avoids prematurely freezing continuous-observer policy.

A verified settlement requires an authority-role source whose system is
`decisions`, kind is `lifecycle_event`, and support target is that exact
settlement, and its `gap` is null. An unverified settlement points to an
explicit gap. This is a fail-closed status boundary: nearby prose or a Git
effect cannot supply user authority.

Revisions are complete snapshots. `revise --base N` rejects a stale observed
HEAD inside the append transaction. Earlier revisions remain readable.

## Projections

HEAD is selected as the greatest revision at read time. Search evaluates only
each episode's HEAD and applies the documented deterministic lexical weights.
The Markdown report is derived from one selected revision.

The graph is also derived. It distinguishes episode-authored claim nodes from
source-backed locator nodes and emits structural, support, explicit coverage-
gap, and episode-relation edges. Its source nodes carry the exact structured
revision, digest, observation time, and role, and the projection repeats the
manual-source boundary rather than implying that Geste resolved an anchor.
Repeated relations to one exact related revision reuse one node. The graph is
complete for one selected revision rather than an unbounded global graph or an
Annals-style concept hierarchy.

## Deferred work

Continuous ingestion, hooks, launchd workers, source watermarks, automatic
episode boundaries, live anchor verification, model-authored reports,
embeddings, automatically inferred resemblance, Annals export, and a graphical
UI are outside version 0.1. Chancery does not advertise them.
