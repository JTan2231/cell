# Deployment instruction executor

Cell builds declared release artifacts and executes the selected products' ordered
instructions. The executor copies opaque files, creates declared links, and runs
literal commands. It does not inspect application state, judge health, verify
artifact contents, infer compatibility, or recover a product.

Ordinary delivery uses `./ci.sh submit COMMIT`. The installed CI manager selects
an exact commit and product set, prepares production artifacts, and calls this
executor. Validation and application diagnostics have separate owners.

## Select an execution

Run from the Cell root:

```sh
./deploy.sh plan nucleus email --source-commit FULL_COMMIT
./deploy.sh start nucleus email --source-commit FULL_COMMIT --request-id MY_REQUEST
./deploy.sh status --request-id MY_REQUEST
```

A caller request ID requires the complete commit ID. A foreground manual call
without a request ID gets a new identity. Reuse the same request ID after an
uncertain submission. Its source, selected products, settings, and prepared build
input must match. Replay returns the retained operation and executes no command.

Selection includes exactly the requested canonical products. The committed
`PRODUCT/deployment/manifest.json` supplies a literal numeric order. Ties use the
product name. The executor does not add dependencies, installed companions, or
maintenance products. The caller owns a complete selection and its execution order.

Use `--settings /absolute/private/settings.json` to pass product-owned settings.
The executor passes each object's contents without interpreting them. Refer to
private credential files rather than placing credentials in command arguments.
The optional `--prepared-build` and `--prepared-build-snapshot-file` carry the
caller's prepared artifact paths and recorded build input. The executor uses the
builder's path metadata. It does not run artifact verification commands.

## Declare instructions

Each product declaration contains `schema: 1`, its canonical `product`, integer
`order`, and an ordered `steps` list. Each step has a unique `id` within its
product. The supported kinds are:

| Kind | Fields | Effect |
| --- | --- | --- |
| `run` | `argv`; optional `cwd`, `env`, `stdin`, `timeout_seconds` | Run the literal argument list. No shell is implied. |
| `copy` | `source`, `destination`; optional integer `mode` | Copy the declared file or tree to a temporary sibling, then rename it into place. |
| `link` | `target`, `destination` | Create a temporary symbolic link, then rename it into place. |

Filesystem paths must be absolute after expansion. Arguments, paths, and explicit
environment values can use `{product}`, `{candidate_dir}`, `{source_root}`,
`{run_dir}`, and `{home}`. Escape a literal brace by doubling it. `stdin` is
literal text, except that `deployment_request` selects the product input described
below. Omitted stdin is empty. An omitted timeout imposes no execution deadline.
A directory copy preserves its entries and modes; optional `mode` changes only
the copied root. Placement cannot atomically replace an existing nonempty directory.

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

The `deployment_request` helper sends one JSON object with `schema: 2`, `product`,
`run_id`, `run_dir`, `source_root`, `candidate_dir`, the candidate's opaque metadata,
`selected_products`, the product's `settings`, and the selected products'
`dependency_candidates` and `dependency_settings`. The names of those maps are
product installer inputs. Their presence does not imply dependency discovery or
compatibility checks. The native product installer owns resource placement and
any setup, maintenance, service, migration, or scheduling commands it requires.
There is no required seven-operation adapter protocol.

## Interpret the receipt

New receipts use `schema: 2` and `manifest_executor: 1`. They retain source and
request correlation, the exact selected product set, and compact step records.
Each record has an `id`, `kind`, `product`, and execution state. Completed records
have timestamps and elapsed seconds. Command records also retain child identity, exit status, and private
stdout/stderr paths. Output is opaque: an exit-zero command succeeds even if its
text describes an application problem. The executor does not parse product JSON.

`state: succeeded` means all declared placements completed and commands exited
zero. It makes no application-health, domain-success, or installed-integrity
claim. `state: failed` means a placement failed or a command exited nonzero. The
executor stops at that instruction. Earlier effects remain; later instructions
are not run. No automatic retry, rollback, or product repair follows.

`state: interrupted` means execution has no completed result or reached a declared
timeout. Application effects remain unknown. On timeout, the executor terminates
and waits for its direct child. A child that does not stop is killed and waited
for. Descendants retain the host execution lock while they run. The executor
does not claim that termination undoes their application effects.

The executor records a command's child identity before opening its execution
gate. One durable attempt record precedes effects, and one completed result
follows them. It retains the manifest once and a compact growing step receipt,
with no duplicated product snapshots or application phase journal.

## Reconcile an interrupted execution

Observe the retained execution after the process stops:

```sh
./deploy.sh reconcile --request-id MY_REQUEST
```

Reconciliation runs no instruction. It preserves an uncertain step as `unknown`
and keeps executor admission closed. Determine the application state through the
owning product's interfaces. Complete any authorized product recovery separately.

Release executor admission explicitly after those decisions:

```sh
./deploy.sh acknowledge --request-id MY_REQUEST
```

Acknowledgement requires the host execution lock to be free. It records the
acknowledgement and releases only the executor's admission marker. The retained
result stays `interrupted`; acknowledgement does not establish application recovery
or successful deployment. A new attempt requires a new request ID.

Completed receipts and per-step output remain under `deployments/operations` and
`deployments/runs` on the configured external work volume. Successful and failed
executions remove their own temporary worktree and prepared copies. Interrupted
executions retain their working material. Installed product releases and live
application state are not pruned. Existing schema-one terminal receipts remain
readable. Unfinished schema-one operations are unsupported; inspect retained
effects and use explicit product procedures.
