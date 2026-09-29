# Annals Usage

Annals Usage reports the model consumption caused by Annals examinations and
the current account allowance used by Annals. It is a separate companion CLI
deployed with Annals. It owns the live reporting interpretation and retains no
telemetry database, aggregate, account snapshot, or credentials.

Annals owns source deliveries, model-run attribution, library state, and inbox
job receipts. Nucleus owns execution, jobs, attempts, exact model output,
account access, and credentials. Annals Usage joins those authorities in memory
and reports incomplete coverage explicitly.

Read `chancery show ID` for one feature or procedure. Read `chancery resolve ID`
for its required contracts and explicit gaps. These commands read installed
documentation. They do not run a report, establish readiness, or authorize work.

## Features

| ID | Read this to understand |
| --- | --- |
| `annals-usage.consumption.inspect` | Delivery attribution, attempts and responses, token categories, coverage, live report output, Rust reporting interfaces, and rate-card comparisons. |
| `annals-usage.execution.operate` | Configuration, live account allowance, diagnostics, credential authority, and attended login recovery. |

The consumption report measures delivery-attributed model tokens. The budget
command reads the live account-global allowance. Other Codex activity shares
that allowance, and there is no supported exact conversion from delivery tokens
to a subscription percentage.

Each report rereads its current Annals and Nucleus authorities. Historical
recalculation depends on retaining those records. Account reads are live and
retain no companion snapshot. Installation, updates, backup, and deployment
remain Annals operations; use `annals.install.operate` for those procedures.

## Development

Use `annals-usage.develop.change` to change reporting interpretation,
configuration, diagnostics, or delegated authentication. The procedure keeps
Annals and Nucleus authority separate and identifies the required validation
and delivery route.

Required dependencies declare documentation compatibility and supply complete
contract reading. Related references provide navigation. Neither transfers
runtime authority or fills an unspecified upstream promise.
