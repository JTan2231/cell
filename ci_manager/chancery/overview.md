# Cell CI manager

The CI manager delivers committed Cell changes through one serial current-user
queue. It retains integration, validation, repair, source acceptance, deployment,
and outcome notification as separate evidence. It does not advance development
`main` or treat an accepted commit as proof of installed product health.

Use `ci-manager.queue.operate` for submission, inspection, cancellation, recovery,
and manager maintenance. New macOS jobs require the persistent signing selection
described by `ci-manager.signing.operate`. That feature owns initial certificate
setup, identity inspection, deliberate configuration changes, and key recovery.

The installed manager pins its host preparation and signing code. Source edits
change candidates; they do not replace the running manager or its signing
selection. Product deployment does not install the manager itself. Read the
selected entry before operating either surface.
