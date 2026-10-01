# Develop Weaver

Use this procedure for an authorized change to Weaver prompts, software,
interfaces, storage, or installation. Read `chancery resolve weaver.develop.change`
for its required contracts. `weaver.narrative.write`
owns authoring behavior and `weaver.lifecycle` owns configuration, state,
readiness, maintenance, and release guarantees.

## Establish the change

1. Read repository instructions, the registered `weaver-narrative` Semantics
   repository, the installed Nucleus manual, and the affected feature contracts.
   The predecessor `weaver` Semantics project is retired.

   ```sh
   semantics repository show weaver-narrative
   nucleus manual
   chancery show weaver.narrative.write
   chancery show weaver.lifecycle
   ```

2. Establish the user-approved endpoint before implementation. Stop when a
   change requires a new scope or authority. Keep one free-form direction, one
   authoring job per document, and Markdown output. Editorial interpretation
   stays in the prompt. Do not add source inventories, persistent feed consumers,
   citations, factual validators, or revived retired workflows without a new
   user decision.
3. Identify affected public meanings, exact request decoding, immutable tool
   identities, prompt selection, state compatibility, consumers, and release
   documentation. Use `annals_api::Client`, `nucleus-client`, and the
   provider-owned `weaver::api::Client` types. Do not copy provider wire types or
   integrate with private storage. Annals owns sources and Nucleus owns
   authentication and execution.

## Change and verify

1. Implement the smallest change that meets the agreed endpoint. Preserve
   exact caller-ID replay, one pending mailbox reply until acknowledgement,
   atomic Markdown/reply storage, and saved output after runtime failure. A
   changed tool contract needs a new immutable identity with historical decoding.
   Do not reconstruct retained requests from newer prompts or add a fallback
   execution route.
2. Keep batch concurrency inside one runner. Use short synchronous SQLite
   operations and hold no transaction across an asynchronous wait. Preserve
   each observation deadline from its original Nucleus attempt start, including
   on resume. Queue time consumes neither authoring deadline.
3. Update the owning feature pages, normalized claims, provider index, overview,
   procedures, descriptor, and adapter when the affected boundary changes.
   Keep related documentation as entry points. Publish the complete provider
   bundle with matching program bytes. Update the Nucleus operator manual only
   for shared boundaries. Keep the root Cell README unchanged unless explicitly
   requested.
4. Run focused checks for the affected behavior. Cover source failures and short
   pages, duplicate/conflicting IDs, pending-call restart, quota deferral,
   queued capacity, cancellation, lost jobs, and saved output followed by runtime
   failure when relevant. For batch changes, verify bounded overlap, independent
   failures and cancellation, and queue time separately from active execution.
   Test installation through its maintained coordinator interface. Do not use
   model jobs as deployment readiness probes.
5. Validate changed source documentation with `chancery validate` and review its
   overview, focused `show` pages, and dependency closure. Preserve intentional
   unsupported or unspecified claims and external reliance gaps. Structural
   validation does not prove prose completeness or runtime readiness.

## Edit prompt selection

1. Read `cell.prompts.weaver` through Bazaar's supported `get` interface. Select
   the caller's private database; use an explicit
   `bazaar --database /absolute/private/bazaar.sqlite3` prefix when Weaver uses
   `CELL_BAZAAR_DATABASE`. The authoring feature owns selection shape, freeze,
   missing-state failure, and immutable toolset semantics.
2. Append the changed component and retain the returned positive version.

   ```sh
   bazaar update PROMPT_ID --file /absolute/prompt.txt
   ```

3. Publish a complete selection after all referenced component text exists.

   ```sh
   bazaar update cell.prompts.weaver --file /absolute/selection.json
   ```

4. Read the resulting selection and verify its intended exact component
   versions. Keep selection version 1 and historical text. To roll back, append
   the prior complete selection content. Keep private text out of logs. A text
   append alone does not change the selected prompt set, and saved work retains
   its original request.

## Deliver the authorized change

1. Commit the intended change when delivery is authorized and submit it from
   the Cell root.

   ```sh
   ./ci.sh submit COMMIT
   ```

   The installed manager integrates, validates, attempts bounded repairs,
   deploys the exact accepted source, and sends its deterministic outcome
   email. Its fixed base and candidate select changed-product and platform
   validation. Focused checks do not replace that manager job.
2. Inspect the retained manager outcome. Confirm the agreed validation and
   installation endpoint through its exact evidence. A source edit, accepted
   commit, selected installation, and outcome email are separate results.
   Follow `weaver.install.operate` for installation or recovery and preserve
   unproved holds. No delivery completion latency is promised.
3. Verify the installed overview, each changed `show` page, and the relevant
   `resolve` closure after authorized deployment. Register the installed command
   inventory with `weaver --register-usage` when programs change.

Implementation authority alone does not authorize remote Git publication,
deployment outside the submitted job, state deletion, source mutation,
narrative publication or email, or another attempt at failed authoring work.
Keep private requests, sources and runtime records protected. CI repair can read
candidate source and diagnostics through Nucleus; the manager owns its defined
outcome email. Unknown state, ownership, or unsupported providers stop work.
