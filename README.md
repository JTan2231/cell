# Cell

Cell is a production service. Its tools use artificial intelligence (AI) to
prepare materials, produce outputs for recipients, and carry those outputs
through delivery. Production definitions specify the materials, instructions,
intended output, timing, and delivery route. For example:

- [Annals](products/annals/README.md) keeps documents and helps you find ideas and
  the original passages that support them.
- [Conatus](products/conatus/README.md) records what you want and helps you see how
  your decisions relate to those wants.
- [Platter](products/platter/README.md) uses postings and recorded career
  experience to prepare opportunity packets and email editions.

I am exploring how AI assistants with different responsibilities can work
together. Many human problems need more explanation and context than a fixed
form can capture. Productions can pass readable text documents between tools.
Each tool owns its materials, acceptance rules, and records. Preparation,
submission acceptance, and final receipt are separate outcomes.

## CI

Commit the intended changes, then submit that commit:

```sh
./ci.sh submit COMMIT
```

The installed [Telete](infrastructure/telete/README.md) queues the commit, integrates
it privately, validates it, attempts bounded repairs, deploys, and emails the
outcome. `telete submit COMMIT` uses the same path.

## Further documentation

Each product directory has a README and detailed documentation. For shared
topology, compatibility, and the sequence of changes, start with the
[Nucleus ecosystem operator manual](infrastructure/nucleus/docs/operator-manual.md).
