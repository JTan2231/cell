# Usher

Usher reports how products declare membership in Cell. It reads product
identity, Semantics participation, and Chancery introductions from a selected
checkout. Products own their declarations. Semantics owns registration and
terminology; Chancery owns complete bundle validation and installed discovery.

Recognition reads local files, computes findings, and exits. It retains no
product state and makes no service, network, or model call. A declaration
does not establish installation or runtime readiness.

Read a feature with `chancery show ID`. Read a procedure and its required
contracts with `chancery resolve ID`. These commands read release documentation;
they do not run Usher or its installer.

## Features

| ID | Read this to understand |
| --- | --- |
| `usher.recognition.inspect` | Inventory, identity and evidence rules, membership findings, report/check output, the separate operational declaration projection, consistency, privacy, and limits. |
| `usher.installation` | The separate Rust installer, retained release identity, fixed runtime paths, owned selectors, recorded metadata, and supported retained-release recovery. |

## Operations

Use `usher.install.operate` to build an exact candidate, inspect installation,
install or recover owned selectors, and inspect the selected release. Installation
does not assess the membership of a checkout.

Use `usher.develop.change` to change recognition, output, packaging, or contracts.
The procedure keeps declaration authority, compatibility, and validation
obligations explicit.

## How the features fit together

`usher report` returns evidence for each inventoried product. `usher check`
reports incomplete findings and returns a failure status when required
declarations are incomplete. Both use the same declaration boundary and global
collision checks. The optional Rust operational projection supplies Iatreion
with declared status units without invoking their commands.

`usher-install` publishes the recognition binary, installer, recovery
executable, and this provider bundle as one immutable release.
The public commands and Chancery provider follow the same atomic selection.
Recovery changes that installation selection, not the declarations it reads.

Related references provide navigation. Version-bounded dependencies determine
the required contract reading and documentation compatibility. They do not
add a runtime dependency on Chancery or authorize another system operation.
