# Predecessor installation evidence

These retired source files document the former shell installation and format-three/four release layout. They are not entry points, are not packaged in new releases, and are not run by CI. Use the Rust `annals-install` commands for current installation, decisions provisioning, attended system migration, and product-journal recovery. Historical retained releases remain independently hash-verified by the product-owned legacy reader; their source remains here only to explain and test predecessor evidence.

The former system LaunchDaemon and user LaunchAgent plist templates remain in `../launchd/` because the Rust migration and installer compare their complete rendered documents before claiming ownership.
