# Architecture

Pratica is a durable protocol for negotiating system-integration terms. It
exists because an entrant can state what it expects, while only the steward of
a concerned system can reconcile those expectations with that system's current
implementation, published contracts, and unresolved choices.

The product is a short-lived CLI with private SQLite state. Commands that need
judgment synchronously use Nucleus. Pratica installs no daemon or schedule and
does not watch target repositories.

## Authorities

| Authority | Owns | Does not own |
| --- | --- | --- |
| Entrant party | Its complete proposed terms and assent | The stewarded system's present behavior |
| Steward party | Whether one exact terms snapshot accurately represents what its scope currently provides, refuses, or would require changed on one exact basis | Authority to implement or deploy the described change |
| Pratica | Integration and track identity, offer order, exact terms bytes and digests, party roster, current assent, staleness, agreement seals, basis records, attempts, reviews, and exports | Meaning inside the Markdown or truth of target source material |
| Target system | Its implementation, behavioral contracts, lifecycle, and change authority | Pratica's negotiation history |
| Nucleus | Agent admission, authentication, execution, job history, and durable tool mailbox | Negotiation success, accepted terms, retry policy, or target-system changes |
| Codex agent | A bounded proposed steward, composition, or conformance response | Party identity, assent, implementation authority, or domain success |
| Chancery | Installed capability discovery and version-matched operating documentation | Runtime execution or negotiation state |

## Slice 1: negotiation ledger

An integration names one entrant and groups the independently negotiable system
concerns for one design. A track fixes exactly two required parties: that
entrant and one registered steward scope version. Its first submitted terms
snapshot opens a negotiation.

Every offer is a new immutable identity containing the exact bounded UTF-8
Markdown bytes supplied by its author and their SHA-256 digest. Pratica does not
trim, normalize, render, parse, or relationally decompose the contract. An
identical byte sequence submitted again is still a distinct offer; its new
identity prevents old assent from being resurrected.

Submitting an offer requires the caller's expected current offer. The new offer
becomes head, its author implicitly assents to it, and every other party's
assent becomes stale. `assent` is a separate event that agrees to the unchanged
current head. Without that distinction, two parties would endlessly invalidate
one another merely by accepting the same terms. Withdrawal removes only the
named party's assent to the named current offer. Cancellation closes an
unsealed negotiation without inventing agreement.

Pratica seals an agreement only when the negotiation is open, the offer remains
the current head, every required party currently assents to that exact offer,
and the steward basis guards still match. The seal is immutable. A later offer
is an amendment negotiation rather than a rewrite of history.

This slice supplies deterministic manual commands, history, status, reports,
exports, and optimistic-concurrency failures without invoking Nucleus.

## Slice 2: steward requester

A steward is a logical system-of-concern scope, not necessarily a repository or
deployable project. Registration freezes one versioned manifest containing the
scope identity, represented party, charter, bounded source catalog, basis
identity, and Nucleus policy. Updating a scope creates another version; an open
track keeps its selected version.

`steward respond` freezes the current negotiation head and steward basis before
creating one Nucleus job. The job has requester program `pratica`, a stable
attempt correlation, a neutral private working directory, no workspace access,
no local execution, no web search, no launch context, and only the immutable
`pratica/steward-response/1` toolset. Managed tools list, search, and read the
frozen source catalog. Source bodies are untrusted evidence, never agent
instructions.

The agent must submit exactly one structured response:

- `assent`: the current terms accurately state the steward scope's position;
- `counterproposal`: one complete replacement Markdown terms snapshot plus a
  review explanation; or
- `blocked`: the source basis exposes a contradiction, missing authority, or
  unresolved choice that prevents a responsible response.

“Steward response” is the umbrella term; a counterproposal is only one possible
response. Pratica validates the accepted call, rechecks the frozen offer and
basis, and commits the domain event atomically. Final model prose is not the
deliverable. Nucleus completion without an accepted Pratica call is not domain
success. If the domain transition committed and the harness later failed,
Pratica preserves the transition and does not repeat it.

One attempt has one Nucleus attempt. `attempt retry` is an explicit new Pratica
attempt with a new job identity and retained predecessor; ambiguous admission
may only reuse the byte-equivalent request under the same job ID.

## Slice 3: integration umbrella

One integration may contain several bilateral tracks because different systems
own different contracts and change at different rates. A new offer in one track
does not stale assent or agreements in another. Retiring a track preserves its
history while removing it from current integration coverage.

`integration status` mechanically aggregates track state and exact agreement
references. `integration report` renders coverage, current heads, assents,
active agreement bases and freshness, the latest composition review, and
explicit limitations. Negotiation history, attempts, and conformance reviews
remain available through their dedicated commands. The report says only which
registered steward scopes have been considered; it never implies that all
possible systems of concern were discovered.

`integration review` freezes the selected current track agreements and submits
one independent Nucleus job using `pratica/composition-review/1`. The reviewer
may report contradictions between terms, uncovered assumptions, incompatible
lifecycles, or missing scope. Its accepted review is an immutable advisory
artifact. It cannot edit an offer, assent for a party, seal an agreement, add a
track, or declare the integration approved. A finding is resolved only through
the affected bilateral negotiation or an explicit integration change.

## Slice 4: conformance and amendment

A sealed design agreement records desired-state alignment, not implementation
proof. `agreement amend` opens a successor negotiation grounded in the prior
agreement and a complete new terms snapshot. The earlier agreement remains
sealed and inspectable.

`conformance review` compares one sealed agreement with one explicitly supplied
candidate basis manifest. It uses the immutable
`pratica/conformance-review/1` toolset and produces a separate immutable review
covering supported, contradicted, unproven, and out-of-scope terms. It does not
change agreement assent or applicability, and it does not implement, test,
deploy, or release the candidate. `agreement verify` checks the stored seal and
digests, then re-reads only the managed source paths already recorded in the
agreement basis and classifies present applicability as fresh, stale, or
unknown. It invokes no model or source adapter, does not alter the historical
agreement, and is not a general source crawl or behavioral test. Pratica may
append the fresh/stale/unknown verification observation while leaving the
agreement and its seal immutable.

This distinction creates two useful but different artifacts:

1. a design agreement: parties accepted these exact terms on this basis; and
2. a conformance review: evidence from this candidate basis supports or fails
   to support those terms.

Neither artifact authorizes target-system mutation.

## Frozen source and basis boundary

Callers supply steward manifests at registration and candidate manifests at
conformance review. Pratica reads only their named regular bounded files,
resolves them before admission, records stable identifiers and digests, and
serves captured bytes through managed tools. The model never receives arbitrary
filesystem paths or a shell. Pratica does not call Chancery, Conversations,
Semantics, Decisions, Git, or a target product at runtime. Later `agreement
verify` may re-read only the already-recorded local locators to classify basis
freshness; it performs no adapter discovery or mutation.

A basis can become stale independently of negotiation staleness. A new offer
stales other-party assent; a changed implementation or contract basis makes a
sealed agreement's current applicability unverified without rewriting the
historical seal. Callers supply the new basis through the owning system's
public contract and ask Pratica to verify or review it explicitly.

## Recovery and privacy

Pratica persists the exact typed Nucleus request, job correlation, selected
offer, selected basis, tool-call outcome, and commit state needed to distinguish
ambiguous transport from a new attempt. Duplicate tool delivery with identical
content returns the same result; conflicting reuse fails closed. A Nucleus
restart makes an unfinished Codex process lost and does not authorize retry.

Terms, source snapshots, prompts, tool calls, reviews, and exports may contain
private implementation and product-design material. Pratica and Nucleus state
must be protected according to that complete content. Release bundles and test
fixtures contain only synthetic material and never include negotiated contracts
or captured source bodies.
