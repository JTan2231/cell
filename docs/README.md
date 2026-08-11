# Annals base design

## Status

This directory describes the intended first implementation of Annals. It is a
design baseline, not a promise that every possible search feature belongs in
version one.

The foundational decisions are:

1. A library contains one or more rooted trees.
2. Every node has a short navigation title and an arbitrary body string. The
   title is user-supplied operational metadata, not generated topic content.
3. Internal nodes are topic views. A complete tree ends in source nodes;
   temporary topic leaves are allowed while editing and reported by validation.
4. SQLite is the canonical store and FTS5 is the search engine.
5. Retrieval searches indexed units directly. It does not route from the root
   downward.
6. Tree structure is used for search scope, breadcrumbs, result grouping, and
   modest reranking.
7. Search indexes are derived data and can always be rebuilt from canonical
   rows.
8. Embeddings, generated content, synchronization, and a server mode are not
   part of the base design.

## Documents

- [Architecture](architecture.md) defines the system boundaries and major
  components.
- [Data model](data-model.md) defines the SQLite schema and tree invariants.
- [Search](search.md) defines the embedding-free retrieval and ranking
  pipeline.
- [CLI](cli.md) defines the proposed command surface and output contracts.
- [Implementation plan](implementation-plan.md) proposes a small Rust stack,
  milestones, and acceptance criteria.

## Terminology

**Library**
: One SQLite database managed by Annals.

**Tree**
: A rooted hierarchy within a library.

**Node**
: A stable object in a tree with a navigation title and an arbitrary body
  string. The body is the topic view or source text.

**Topic node**
: A node whose children present the same subject in greater detail.

**Source node**
: A leaf containing source text or metadata referring to source material.

**Search unit**
: The text indexed as one retrieval record. A short body normally produces one
  unit; a long body may produce several passage-sized units. Search units do
  not alter the visible tree.

**Direct match**
: A match against a node's own indexed text.

**Supporting match**
: Relevance inherited from a matching descendant and used only to make an
  ancestor a useful navigation result.

## Explicit non-goals

The first implementation does not need:

- embeddings or a vector index;
- automatic topic assignment or text generation;
- a graph database or a general DAG;
- a client/server database;
- collaborative editing or synchronization;
- plugins, an ORM, or an async runtime;
- a second full-text index such as Tantivy.

Those choices can be revisited only after the SQLite/FTS5 implementation has a
measured limitation.
