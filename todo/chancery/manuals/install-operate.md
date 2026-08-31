# Operate the Todo installation and database

Todo is a synchronous local CLI with a SQLite database. It has no Todo daemon.
The user installation additionally owns one daily email LaunchAgent; Nucleus
is a separate service used only by model-assisted research.

## Database initialization and migration

Create a new database without replacing anything:

```sh
/Users/joey/.local/bin/todo --database <DATABASE_PATH> init
```

Ordinary commands never migrate implicitly. For a version-one database, stop
callers and choose an absolute backup path that does not exist:

```sh
/Users/joey/.local/bin/todo --database <DATABASE_PATH> migrate \
  --backup <ABSENT_ABSOLUTE_BACKUP_PATH>
```

Todo creates and retains a complete SQLite backup before the version-two
transaction. Failure leaves the original usable. Against a current database,
migrate is a true no-op and does not touch the supplied backup path.

Migration preserves identities, lifecycle, sources, notes, and historical
direction. It does not infer cross-todo relationships, assessment facts,
accepted design, implementation, or closure evidence.

## Deploy or update on macOS

Build and test a candidate, verify Nucleus, and invoke the product deployer
only with explicit installed-state authority:

```sh
cd /Users/joey/rust/cell
./todo/ci.sh
./todo/packaging/macos/deploy-user.sh \
  --binary <ABSOLUTE_TODO_BINARY> \
  --email-from <SENDER> \
  --email-to <RECIPIENT>
```

A fresh install requires both email-address flags. An update may preserve
existing complete email configuration by omitting both; supplying only one is
an error.

The deployer stages a content-addressed release, records whether the email
LaunchAgent is loaded, quiesces it, creates a private transaction directory,
runs the candidate's backup-bearing migration, switches the release selector,
validates the installed CLI, then installs and bootstraps the final plist. A
pre-commit failure restores the captured database, release, frontend,
configuration, plist, and loaded-service state. The previous release remains
available through the installation selector.

Nucleus authentication and state are never part of Todo rollback. The Resend
key remains in the process environment rather than Todo config or plist.

## Validate operation

```sh
/Users/joey/.local/bin/nucleus health
/Users/joey/.local/bin/todo email preview
/Users/joey/.local/bin/todo email send
launchctl print "gui/$(id -u)/org.todo.daily-email"
```

Email send is externally consequential and requires separate authorization;
preview can validate content without disclosure. The LaunchAgent runs at local
09:00 without RunAtLoad and may run after wake. It cannot submit while the Mac
is off or the user is logged out.

Todo database, backups, config, logs, and email content are private. Do not
delete retained state, overwrite backups, publish a release, or change Resend
account state merely as part of installation diagnosis.
