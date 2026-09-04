# Inspect Pratica state

Pratica's database is authoritative for negotiation identities, exact terms,
event order, assent, seals, attempts, and reviews. Use its read commands rather
than inspecting SQLite directly.

```sh
/Users/joey/.local/bin/pratica steward list
/Users/joey/.local/bin/pratica steward show SCOPE --version VERSION
/Users/joey/.local/bin/pratica integration list
/Users/joey/.local/bin/pratica integration status INTEGRATION
/Users/joey/.local/bin/pratica integration report INTEGRATION
/Users/joey/.local/bin/pratica negotiation show NEGOTIATION
/Users/joey/.local/bin/pratica negotiation history NEGOTIATION
/Users/joey/.local/bin/pratica attempt show ATTEMPT
/Users/joey/.local/bin/pratica agreement list
/Users/joey/.local/bin/pratica agreement show AGREEMENT
/Users/joey/.local/bin/pratica conformance show REVIEW
```

Treat all identifiers as opaque. The current textual prefix is a readability
aid, not a parsing or compatibility contract.

Use `integration list` and `agreement list` to recover identities without
printing retained context or terms. They are complete newest-first body-free
projections. Steward list/show and registration output likewise omit charter
and source bodies and canonical source-origin paths; attempt inspection omits
canonical source origins. These views can still disclose sensitive metadata,
including parties, titles, relative locators, digests, byte counts, and
timestamps. Use detail, report, review, or export commands only when those
documented bodies are needed.

## Read the distinctions

An offer is one immutable complete Markdown snapshot. Its digest identifies
bytes, but assent names the unique offer identity. A later offer with identical
bytes does not inherit old assent.

An agreement is the immutable seal created while all fixed parties assented to
the current offer and the steward basis matched. A later source change can make
current applicability stale or unknown, but cannot rewrite that historical
seal.

An integration report aggregates only explicitly opened tracks. A composition
review is advisory and can identify contradictions or missing coverage. It is
not global approval. A conformance review compares one agreement to one supplied
candidate basis; it is not product testing or implementation.

## Verify and export

```sh
/Users/joey/.local/bin/pratica agreement verify AGREEMENT
/Users/joey/.local/bin/pratica agreement export AGREEMENT
/Users/joey/.local/bin/pratica agreement export AGREEMENT \
  --output /absolute/private/path/terms.md
```

Verification checks stored identities, exact terms digest, event and agreement
seal, party assent, and recorded basis guards. It also re-reads only the
already-recorded managed source paths without a model, adapter, crawl, or side
effect and classifies present basis applicability as fresh, stale, or unknown.
It does not change the historical seal or prove behavioral conformance.
Pratica retains the fresh, stale, or unknown observation as a separate immutable
basis-verification record.

Export emits the exact stored Markdown bytes and does not normalize or add a
header. A selected output path must be safe and absent; Pratica does not
overwrite an existing file.

## Failures and privacy

Run doctor against the same selected schema-two database when reads report schema,
integrity, digest, seal, or permission errors. Resolve current target behavior
through the target system's public contract; never repair history through raw
SQL.

Terms, source labels, reviews, citations, and stable identities can be private.
Terminal output and exports are caller-owned disclosures. Inspection commands
make no Nucleus or network call. `agreement verify` does read the recorded local
source locators and append its classification; it reports unavailable or
changed bytes rather than silently accepting them.
