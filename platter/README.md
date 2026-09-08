# Platter

Platter prepares private job briefs and tailored resumes from Cast opportunities
and CRM career material. It changes only the Jackson work-experience bullets
in the supplied resume. It retains prepared content and email outcomes.

## Example

With an initialized library and its dependencies ready:

```sh
platter prepare CAST_JOB_ID
platter status
platter export ARTIFACT_ID /absolute/chosen/resume.pdf
```

Preparing a packet does not send it. Preview freezes an edition; `send` submits
that edition through Email. Uncertain sends remain held for recovery.

## Check

From the Cell root:

```sh
./ci.sh platter
```

## Further documentation

- [Preparation, editions, and delivery](chancery/manuals/packet-prepare.md)
- [Stored records](docs/data-model.md)
- [Installation and migration](chancery/manuals/install-operate.md)
