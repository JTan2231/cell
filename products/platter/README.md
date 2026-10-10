# Platter

Platter is a production service for private opportunity packets and email
editions. It prepares briefs and resumes from Milieu opportunities and Vita
career material. Weaver supplies project bullets. Platter retains accepted
production materials, frozen editions, and authorized email submission outcomes.

## Example

With an initialized library and ready dependencies:

```sh
platter prepare MILIEU_OPPORTUNITY_ID
platter status
platter export ARTIFACT_ID /absolute/private/chosen/resume.pdf
```

Preparation does not send. Use the preparation operation for an authorized
URL-selected, daily, or retained-material send.

## Documentation

Read the [product overview](chancery/overview.md), or use `chancery product platter`
for the installed release. Feature contracts own supported behavior:

- [Materials and templates](chancery/manuals/materials.md)
- [Packet preparation](chancery/manuals/preparation.md)
- [Editions and delivery](chancery/manuals/editions.md)
- [Prepared opportunity reads](chancery/manuals/opportunity-explore.md)
- [Maintenance and release](chancery/manuals/maintenance.md)

Use [prepare and delivery procedures](chancery/manuals/packet-prepare.md) or
[installation and maintenance procedures](chancery/manuals/install-operate.md).
`chancery show ID` reads one page; `chancery resolve ID` includes required
contracts. These reads do not probe readiness or authorize execution.

Submit committed changes through the [Telete](../../infrastructure/telete/README.md) with
`./ci.sh submit COMMIT` from the Cell root.
