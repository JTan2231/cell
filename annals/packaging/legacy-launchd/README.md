# Predecessor installation evidence

These retired source files document the former shell installation and
format-three/four release layout. They are not entry points. New releases do
not package them, and CI does not run them. Use the Rust `annals-install`
commands for installation, decisions provisioning, attended system migration,
and product-journal recovery. The product-owned legacy reader independently
verifies hashes for retained historical releases. These sources remain to
explain and test the predecessor format.

The former system LaunchDaemon and user LaunchAgent plist templates remain in
`../launchd/`. The Rust migration and installer compare their complete rendered
documents before claiming ownership.
