# Integrate a source with the Annals corpus

Integration asks Annals' constrained AI reader to examine one immutable work
against one frozen corpus revision and propose how its ideas and exact
supporting quotations fit the current map. The proposed interpretation is
provisional and evidence-grounded; Annals does not decide truth or claim a
uniquely correct decomposition.

## Start an examination

Integrate a file or an existing work:

```sh
/Users/joey/.local/bin/annals integrate <UTF8_INPUT> --name <LABEL>
/Users/joey/.local/bin/annals integrate --work <LABEL>
```

Both forms deliberately examine the selected work even when its bytes were
retained earlier. Annals freezes HEAD, submits one closed Nucleus job, and
expects its liaison to record one reconciliation through Annals' validated
tools. The liaison has only the scoped work/corpus interfaces supplied by
Annals. Its final prose is diagnostic and is not parsed as the result. Direct
integration requires a `general` library; a decisions library is dispatched
only from producer-accepted inbox jobs or their explicit retry children.

Annals may reuse the newest successful examination for the exact same work,
base revision, prompt version, model, and reasoning effort. Force a fresh
reading only when that is intended:

```sh
/Users/joey/.local/bin/annals integrate --work <LABEL> --reexamine
```

## Pending versus applied

The safe default does not apply a material proposal:

```sh
/Users/joey/.local/bin/annals integrate --work <LABEL>
/Users/joey/.local/bin/annals change show --work <LABEL>
/Users/joey/.local/bin/annals change validate --work <LABEL>
```

When immediate application is explicitly authorized, `--apply` atomically
commits the projected corpus transition:

```sh
/Users/joey/.local/bin/annals integrate <UTF8_INPUT> --name <LABEL> --apply
```

Without it, a material result remains pending. A projected state mechanically
equal to the base is recorded with no corpus change; it creates no commit and
does not advance the revision. A pending reconciliation can apply only while
HEAD still equals its base.

## Success, failure, and authority

Annals domain state is authoritative: success means a valid reconciliation was
recorded, and application success means Annals committed the resulting state.
Nucleus job completion alone is insufficient. Conversely, a later Nucleus
runtime failure does not erase a reconciliation Annals already recorded.

Draft validation can return named operations for revision while retaining
independently valid operations. A liaison may discard an irreparable draft;
abandoned and discarded drafts remain audit records but create no
reconciliation.

The complete work and frozen corpus context can enter the immutable Nucleus
request and raw protocol state. Protect both the Annals library and Nucleus
state as sensitive. Use Annals Usage, not this capability, to inspect the
resulting model consumption.
