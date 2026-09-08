# Installed artifacts

The Rust `mentor-install` command uses `cell-install::simple` and the shared
content-addressed transaction format. Its product specification is maintained
in `src/installation.rs`; there is no product-specific shell deployer or
credential-loading frontend.

A release contains `bin/mentor`, `bin/mentor-install`, `package/install`, and the
version-matched `share/chancery/mentor` provider. The corpus is embedded in the
Mentor executable. Public selectors are installed under `~/.local/bin` and the
user's Chancery provider registry.

Clockwork definitions are generated only by the explicit `mentor schedule
enable` operation. They retain the exact installed executable path and hash,
not the mutable command selector. The generated definition is retained under
`~/Library/Application Support/MentorMail/schedules/` and registered through
Clockwork's public API. Clockwork owns the generated LaunchAgent.
