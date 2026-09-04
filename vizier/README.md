# Vizier

Vizier is a short-lived local CLI for delegating a finite, contract-bounded
implementation. It freezes the caller's implementation brief, terminology
snapshot, ordered Markdown contract units, Git source revision, and gate
commands; plans the work; delegates isolated implementation packets; obtains
independent reviews; and returns an exact reviewed Git candidate.

Vizier coordinates work. It does not interpret Markdown requirements, approve
its own edits, move the caller's branch, push, release, deploy, or change
Pratica or Semantics. Every Vizier-created Nucleus role request explicitly
uses requester-owned `gpt-5.6-sol` with reasoning effort `max`.

## Basic use

Initialize the private ledger, then submit a run:

```sh
vizier init
vizier run submit \
  --repository /absolute/path/to/repository \
  --brief /private/path/brief.md \
  --terminology /private/path/terminology.md \
  --contract api=/private/path/api-contract.md \
  --contract storage=/private/path/storage-contract.md \
  --gate product-ci='./ci.sh'
```

`run submit` drives the run synchronously. If the process is interrupted, use
`vizier run show RUN_ID` and `vizier run resume RUN_ID`. A successful run is
bound to the exact integrated candidate that passed its configured gates and
independent final review. Use `vizier document show DOCUMENT_ID` to read an
exact retained document referenced by a run or attempt; routine run views do
not print document bodies.

See [docs/README.md](docs/README.md) for the public contracts and operating
guides.
