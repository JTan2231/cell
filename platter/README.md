# Platter

Platter prepares private job briefs and tailored resumes from Cast opportunities
and Vita career material. Agents research Cell, Wrought and Krisis locally to
write the projects section alongside Jackson work-experience bullets. Platter
retains prepared content and email outcomes.

## Example

With an initialized library and its dependencies ready:

```sh
platter prepare CAST_JOB_ID
platter run-ad-hoc 'https://jobs.ashbyhq.com/COMPANY/JOB_ID' --id OCCURRENCE_ID
platter status
platter export ARTIFACT_ID /absolute/chosen/resume.pdf
```

`prepare` does not send. `run-ad-hoc` prepares one selected URL and sends its
frozen packet. Uncertain sends remain held for recovery.

## CI

Commit the intended changes, then submit them from the Cell root:

```sh
./ci.sh submit COMMIT
```

The [CI manager](../ci_manager/README.md) integrates, validates, attempts bounded
repairs, deploys, and emails the outcome.

## Further documentation

- [Preparation, editions, and delivery](chancery/manuals/packet-prepare.md)
- [Vita career library](chancery/manuals/vita.md)
- [Stored records](docs/data-model.md)
- [Installation and migration](chancery/manuals/install-operate.md)
