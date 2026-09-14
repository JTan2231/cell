# Develop Weaver

Read the `weaver-narrative` Semantics repository and the installed Nucleus
manual before changes. Keep Weaver's input as one free-form direction. The
authoring prompt owns editorial interpretation. The product owns saved
Markdown and minimum execution recovery state in one SQLite table.

Use `annals_api::Client` for accepted-document reads and `nucleus-client` for
execution. Keep source pagination in the reader interaction. Do not add source
inventories, persistent feed consumers, citations, or factual validators without
a new user decision. Nucleus authentication remains provider-owned.

Preserve exact submission identity and one pending mailbox response until
acknowledgement. Save Markdown and its reply atomically. Treat terminal runtime
failure separately from a saved document. A changed tool contract needs a new
immutable identity; retain decoding for existing requests. The initial toolset
is `weaver/narrative/1` and the database schema is 1.

Run `./ci.sh` from Cell after changes. The default gate selects the changed
products and required platform checks. Test source failures, short pages,
duplicate and conflicting submissions, pending-call restart, quota deferral,
queued capacity, cancellation, lost jobs, and saved output followed by runtime
failure. Test the installer through its maintained coordinator interface.
Do not use model jobs as deployment readiness probes.

Update the owned manuals, normalized Chancery claims, descriptor, and adapter
when their behavior changes. Update the Nucleus operator manual for shared
boundaries. Keep the root Cell README unchanged unless explicitly requested.
Register the product through its own content-addressed provider selector.

Implementation authority alone does not authorize Git publication, deployment,
state deletion, source mutation, email, or a new attempt at failed work. Follow
the user's authorized endpoint and the shared deployment playbook. Unknown
state or unsupported providers remain errors; do not add execution fallbacks.
