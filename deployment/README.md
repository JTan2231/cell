# Cell deployment instructions

Deploy Cell products through CI and its installed Telete manager. Telete validates
committed source, prepares signed production candidates, and executes the selected
products' ordered instructions. The shared Rust `cell-install` and
`cell-maintenance` libraries remain here for product installers and maintenance.

## Submit a deployment

Run from the Cell root:

```sh
./ci.sh submit COMMIT --deploy nucleus --deploy email
./ci.sh status JOB
```

Omit `--deploy` to deploy the products selected by the change. Repeat `--deploy`
to choose an exact deployment set. Telete merges the submission with its accepted
source; it does not restore an older source tree. An explicit deployment selection
continues through CI even when the submitted commit is already accepted. A
submission without that selection reports `already_included` for accepted source.
Use a new request ID for a new delivery. Reuse `--request-id KEY` only with the
same commit, deployment selection, settings, and other job choices.

Add `--settings /absolute/private/settings.json` with an explicit deployment set
when product setup requires choices. The JSON object maps canonical product IDs
to product-owned objects. Every settings key must belong to the deployment set.
Use `krisis` for its settings, including when `--deploy decisions` selects its
alias. Telete freezes the values at submission. Later file changes do not change
the job. Refer to private credential files instead of putting credential bytes
in settings. Omitted settings use each product's existing configuration or defaults.

Read [Telete queue operation](../infrastructure/telete/chancery/manuals/queue-operate.md)
for test selection, signing, prerequisites, and retained outcomes. Source release
publication has its own [release command](../pipeline/README.md); it performs no
build or deployment.

## Declare instructions

The committed product descriptor supplies `PRODUCT_DIR`. Its
`PRODUCT_DIR/deployment/manifest.json` contains `schema: 1`, a canonical `product`,
an integer `order`, and an ordered `steps` list. Telete sorts products by order,
then name. It executes instructions in their declared order. Each instruction
has a unique `id` within its product.

| Kind | Fields | Effect |
| --- | --- | --- |
| `run` | `argv`; optional `cwd`, `env`, `stdin`, `timeout_seconds` | Run a literal argument list without an implied shell. |
| `copy` | `source`, `destination`; optional integer `mode` | Copy to a temporary sibling, then rename into place. |
| `link` | `target`, `destination` | Create a temporary symbolic link, then rename into place. |

Filesystem paths must be absolute after expansion. Arguments, paths, and explicit
environment values can use `{product}`, `{candidate_dir}`, `{source_root}`,
`{run_dir}`, and `{home}`. Double a brace to preserve it literally. `stdin` is
literal text, except that `deployment_request` selects the installer input.
Omitted stdin is empty. An omitted timeout imposes no execution deadline. A
directory copy preserves its entries and modes; optional `mode` changes only the
copied root. Placement cannot atomically replace an existing nonempty directory.

For example:

```json
{
  "schema": 1,
  "product": "email",
  "order": 4,
  "steps": [
    {
      "id": "install",
      "kind": "run",
      "argv": ["{candidate_dir}/bin/email-install", "deploy"],
      "stdin": "deployment_request"
    }
  ]
}
```

The `deployment_request` input has `schema: 2`, `product`, `run_id`, `run_dir`,
`source_root`, `candidate_dir`, candidate metadata, `selected_products`, the
product's `settings`, and the selected products' `dependency_candidates` and
`dependency_settings`. Telete passes settings unchanged. These maps do not add
dependencies or perform compatibility checks. Product installation commands own
program selection, state, configuration, runtime pins, schedules, and services.

The shared installer retains UUID release archives and copies payloads to regular
files under each product's fixed `install/runtime` directory. Public executable
links resolve to that tree. Provider directory links select an archived bundle.
Product lifecycle operations quiesce affected execution before publication.
Replacement is atomic per file, not across the runtime tree. Product recovery
restores a compatible retained payload to the same runtime paths.

## Interpret and recover an outcome

Read the retained Telete job result. Deployment receipts use `schema: 2` and retain
the source, request, run, selected products, and instruction results. Command
results retain exit status and output, with private logs retained separately. Telete does not parse
product output to establish application success.

`succeeded` means every instruction completed. `failed` means an instruction
failed; earlier effects remain and later instructions do not run. `interrupted`
means effects are uncertain. Telete does not retry an uncertain instruction or
infer product rollback. Accepted source remains accepted after deployment failure.
Instruction completion does not establish product health or domain success.

Inspect the retained instruction and use the owning product's recovery interface
before acknowledging an interrupted deployment:

```sh
./ci.sh acknowledge-deployment REQUEST
./ci.sh recover JOB
```

Acknowledgement requires exclusive deployment ownership. It releases the retained
operation without undoing effects, running remaining instructions, or establishing
product health. Use a new request ID for a new delivery after recovery decisions.
Retained legacy manual receipts do not authorize a new manual deployment. Shared
signing guards continue to fence their unresolved effects.
