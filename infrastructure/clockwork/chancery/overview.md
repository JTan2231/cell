# Clockwork

Clockwork starts scheduled non-agent programs for the current macOS user.
New product launch images use fixed regular files in the product
`install/runtime` tree. Clockwork uses a fixed broker executable there, while
retained releases preserve recovery bytes and definition identity.

A product registers an immutable definition and selects it with a stable
`owner/name` binding. launchd invokes a short-lived broker. Clockwork checks
admission and pinned top-level images, supervises one child, and retains
runtime evidence. The product owns its work, idempotency, retries, secrets,
output, recovery, and domain success. launchd owns timer delivery.

Public `clockwork` commands print plain text by default. Pass `--json` for
the existing machine response. Private broker receipts remain JSON.

Read one feature with `chancery show ID`. Read an operating route and all its
required contracts with `chancery resolve ID`. These commands read the installed
publication. They do not invoke Clockwork, check live readiness, or authorize
product work, deployment, continuation, or mail.

## Features

| ID | Read this to understand |
| --- | --- |
| `clockwork.definitions` | Strict immutable manifests, schedules, pinned launch images, literal context, identity, and artifact-verification limits. |
| `clockwork.bindings` | Stable selection, generated LaunchAgents, disabling, cutover journals, compensation, and timer-delivery limits. |
| `clockwork.activations` | Admission, overlap, one direct child, timeout, loss proof, process history, doctor, and read-only status evidence. |
| `clockwork.incidents` | Product failure reports, pending episodes, delayed automatic halts, immediate explicit halts, incident feed, and exact approval of future admission. |
| `clockwork.notifications` | Consecutive service checks, basic pause alerts, transport uncertainty, EMT routing, grace, and durable delivery ownership. |
| `clockwork.installation` | Content releases, selectors, release metadata, coordinated broker refresh, retained state, migration, rollback, and detach. |

## Operating routes

Use `clockwork.schedule.operate` to register, select, disable, inspect, or run a
product schedule; inspect incidents and explicitly approved continuation;
operate retained alerts and optional EMT handoff; or migrate quiescent state.
This broad entry retains its capability kind and stable ID. Contract five
includes plain text defaults and in-place transactional migration. It preserves
the shared service-health delay before new automatic halts. Its manual owns
procedure order and action-critical checkpoints; its
required feature contracts supply the detailed explanations.

Use `clockwork.install.operate` to deploy through CI and Telete,
coordinate broker refresh, recover a verified retained release, detach owned
selectors, or explicitly migrate quiescent state. Underlying program setup APIs
are separate from product definition selection and database migration.

Use `clockwork.develop.change` to change Clockwork while preserving scope,
compatibility, privacy, recovery, and validation obligations.

## How the features fit together

A definition fixes the product release, schedule, process context, direct
artifacts, output paths, and failure policy. A binding chooses that definition.
The broker resolves the binding only after the transition gate opens, then pins
one definition at admission. Per-key overlap prevents another direct child.
Process records describe execution; the product separately determines success.

A schema-two abend applies the product's declared policy. The default records
a pending failure episode and allows later activations before the shared
service-health threshold. By default, five consecutive failed read-only checks, at least 60
seconds apart, establish the halt and alert eligibility together. Healthy worker
observations, later successful activations without an abend, and inactive intent
clear pending episodes. An explicit halt remains immediate. Only exact user
approval clears an established halt; selection, disable, installation and
notification recovery never approve it.

Clockwork retains check progress and basic metadata; Iatreion owns observed facts. EMT can
claim initial notification after retaining its own email. Email owns credential
loading and provider submission. Transport never controls the scheduling halt.

The release contains this complete overview, feature contracts, and procedures.
Its stable provider follows the selected Clockwork program release. Persistent
runtime state and exact broker paths in generated plists remain independent.
Related references provide navigation. Only declared required dependencies
participate in compatible complete resolution; those edges do not create
runtime calls or transfer product authority.

Use `nucleus manual` for shared topology, maintenance, and cross-product recovery
order. Use Clockwork's contracts for its supported local behavior. No catalog
or installation check proves launchd delivery or a product-domain result.
