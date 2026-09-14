# Cell

Cell is a collection of tools that use artificial intelligence (AI) to
organize information and carry out ongoing work.

Think of it as a small office on a computer. Different tools keep records,
research questions, prepare documents, and coordinate assignments. For example:

- [Annals](annals/README.md) keeps documents and helps you find ideas and
  the original passages that support them.
- [Conatus](conatus/README.md) records what you want and helps you see how
  your decisions relate to those wants.
- [Platter](platter/README.md) uses job descriptions and recorded career
  experience to prepare tailored résumés and short briefs.
- [Mentor](mentor/README.md) sends a daily software design exercise by email
  and gives feedback on your answer.

I am exploring how AI assistants with different responsibilities can work
together. Many human problems need more explanation and context than a fixed
form can capture. Most workflows here pass readable text documents between
assistants, with each tool responsible for its own work and records.

## Check

```sh
./ci.sh
```

To check one product while iterating, for example:

```sh
./ci.sh nucleus
```

## Further documentation

Each product directory has a README and detailed documentation. For shared
topology, compatibility, and the sequence of changes, start with the
[Nucleus ecosystem operator manual](nucleus/docs/operator-manual.md).
