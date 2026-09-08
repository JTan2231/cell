# Bundled Mentor exercises

`corpus.json` is a retained snapshot of the authored problems and shared
evaluation contract from `/Users/joey/ts/mentor`, generated on 2026-09-07. It
contains all 58 direct problem Markdown files except `problems/README.md`, plus
`rubric/system-design.md`. The source repository remains the authoring location.
The mail service uses the bundled snapshot without requiring the desktop app,
its database, the source checkout, Python, or Bun at runtime.

The source checkout had existing uncommitted edits when this snapshot was
created. A package version or Git revision would not identify those exact
authored bytes. `source_version` records the source package's descriptive
version; the digest identifies the retained content.

Canonical corpus SHA-256:

```text
6bcb880eb2bfb371d069738a6a1a5b4b56f52f80172e90baf4b4e360486e463a
```

The digest covers the compact UTF-8 JSON serialization of the `Corpus` value,
with fields in the order defined in `src/corpus.rs`, nested fields in their
declared order, and problem array order preserved. It excludes presentation
indentation and the JSON file's final newline. `Corpus::digest()` computes it.
CRLF and CR in authored Markdown are normalized to LF during export, matching
the desktop corpus loader. Other problem and rubric text is retained unchanged.

## Artifact contract

Schema version 1 contains:

- `schema_version`: the bundle format version, currently `1`.
- `source_product`: `mentor`, identifying the authoring product.
- `source_version`: the source package's version at export time.
- `rubric`: its explicit contract `version` and complete `markdown`.
- `problems`: each problem's stable `id`, title, and complete `markdown`.

A problem ID is the authored filename without `.md`. IDs retain their meaning
across content updates. Editing a problem does not make it a new exercise for
mailing history; changing an ID is an identity change. The bundle includes no
drafts, source paths, model answers, or evaluation history. Problem Markdown is
the sole source of problem-specific requirements, and the shared rubric is the
evaluation contract. The rubric is retained for grading; it is not part of the
daily problem email.

The importer accepts only schema 1, the known fields, a Mentor source version,
and nonempty problem collections with unique lowercase ASCII slug IDs. It
requires titles to match the sole level-one heading, nonempty problem bodies,
and a versioned rubric with its canonical title and eight ordered, uniquely
named dimensions. Input is bounded to 8 MiB, 1,000 problems, and 256 KiB per
Markdown document. Markdown must use LF line endings and contain no NUL bytes.

An updated bundle is an explicit content refresh. Retain the exact imported
bundle used for each assignment so a later edit cannot change the prompt or
rubric used to critique an earlier answer. A refreshed bundle does not reset
the record of previously mailed problem IDs.

## Refreshing authored content

From the `mentor` product directory, create a new artifact with:

```sh
python3 scripts/export-corpus.py \
  --source-root /Users/joey/ts/mentor \
  --output content/corpus.json
```

The exporter reads only the source package version, authored problems, and
shared rubric. It does not modify the source repository or access desktop
state. Its current expected collection size is 58. A deliberate source corpus
expansion requires updating that expectation. When refreshing the artifact,
update the snapshot date and digest above to describe the newly retained
content. Generation reports the digest; it does not run a test suite or build.
