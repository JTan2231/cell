# Linux installation

See [configuration](system-installation.md#configuration) before enabling the timer.

The packaged units use this layout:

```text
/usr/local/bin/annals
/usr/local/bin/annals-usage
/usr/local/bin/nucleus
/etc/annals/config.toml
/etc/annals/usage.toml
/var/lib/annals/
|-- annals.db
`-- Library/Application Support/Nucleus/
    `-- nucleus.sock          # supplied by the separately deployed Nucleus service
/var/spool/annals/
|-- .queue.json
|-- .run.lock
|-- .control.lock
|-- .paused       # present while operator-paused
|-- .maintenance  # present only during managed maintenance
|-- incoming/
|-- queued/
|-- processing/
|-- done/
|-- duplicates/
|-- failed/
`-- skipped/
```

The state directory must be writable by the service account because SQLite may
create WAL and shared-memory sidecars next to the Annals library. The Nucleus
socket must be reachable by the `annals` service account; deploy and validate
that service separately before enabling the inbox timer. The packaged paths
assume Nucleus also runs as `annals` with `HOME=/var/lib/annals`, so its
standard socket and the credential home selected by delegated login agree.

Build Annals, create a non-login service account, and install the files:

```sh
cargo build --release --package annals --package annals-usage

sudo groupadd --system annals
sudo useradd --system --gid annals --home-dir /var/lib/annals \
  --shell /usr/sbin/nologin annals
sudo install -m 0755 ../target/release/annals /usr/local/bin/annals
sudo install -m 0755 ../target/release/annals-usage \
  /usr/local/bin/annals-usage

sudo install -d -o root -g annals -m 0750 /etc/annals
sudo install -d -o annals -g annals -m 0700 /var/lib/annals
sudo install -d -o annals -g annals -m 0710 /var/spool/annals
sudo install -d -o annals -g annals -m 0770 \
  /var/spool/annals/incoming
sudo install -d -o annals -g annals -m 0700 \
  /var/spool/annals/queued \
  /var/spool/annals/processing \
  /var/spool/annals/done \
  /var/spool/annals/duplicates \
  /var/spool/annals/failed \
  /var/spool/annals/skipped

sudo install -o root -g annals -m 0640 \
  packaging/systemd/annals.toml /etc/annals/config.toml
sudo install -o root -g annals -m 0640 \
  packaging/systemd/usage.toml /etc/annals/usage.toml
sudo install -o root -g root -m 0644 \
  packaging/systemd/annals-inbox.service \
  /etc/systemd/system/annals-inbox.service
sudo install -o root -g root -m 0644 \
  packaging/systemd/annals-inbox.timer \
  /etc/systemd/system/annals-inbox.timer
```

Adjust the executable and socket paths in the configs and service if Nucleus
or Annals is installed elsewhere. Authenticate through the Nucleus service so
login, account reads, and model jobs remain under its single credential
authority:

```sh
sudo -u annals env HOME=/var/lib/annals \
  ANNALS_USAGE_CONFIG=/etc/annals/usage.toml \
  /usr/local/bin/annals-usage login --device-auth
```

The configured Nucleus service owns its private credential directory. Do not
give Annals a second Codex home or invoke Codex directly as a fallback.

Initialize the library and enable the timer:

```sh
sudo -u annals env HOME=/var/lib/annals \
  /usr/local/bin/annals --config /etc/annals/config.toml init

sudo -u annals env HOME=/var/lib/annals \
  /usr/local/bin/annals-usage doctor \
  --config /etc/annals/usage.toml

sudo systemctl daemon-reload
sudo systemctl enable --now annals-inbox.timer
```

The timer runs two minutes after boot and five minutes after the previous
service activation becomes inactive. `Type=oneshot` prevents systemd from
starting a second copy of the service, and the Annals inbox lock also protects
against a concurrent manual invocation. The unit deliberately has no systemd
start timeout because one liaison can run for up to 60 minutes and an
activation may continue draining for as long as runnable work remains. An
unexpected processing failure still ends that activation nonzero after the
failed job is archived.

The packaged service sets
`ANNALS_USAGE_CONFIG=/etc/annals/usage.toml`. Both configs select the same
Nucleus socket, and no companion ledger is opened.

Inspect or trigger it with:

```sh
sudo systemctl start annals-inbox.service
sudo systemctl status annals-inbox.timer annals-inbox.service
sudo journalctl -u annals-inbox.service
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox status
```

See [inbox operations](inbox.md) for status fields, pause, priority,
interruption, retry, and delivery history. Run those commands as the `annals`
service account with the explicit config path shown above.

## Supply source files

For an explicit priority handoff that also keeps the source file in place, use:

```sh
sudo -u annals /usr/local/bin/annals \
  --config /etc/annals/config.toml inbox enqueue --priority ./report.md
```

Do not also copy that file into `incoming/`; the enqueue command has already
created its queued job envelope.

A direct copy is supported by the settling interval:

```sh
sudo -u annals cp -n ./report.md /var/spool/annals/incoming/report.md
```

For an atomic handoff, use a private staging directory beside the inbox. The
final move is on the same filesystem and keeps the original basename:

```sh
sudo install -d -o annals -g annals -m 0700 /var/spool/annals/staging
sudo -u annals cp ./report.md /var/spool/annals/staging/report.md
sudo -u annals mv /var/spool/annals/staging/report.md \
  /var/spool/annals/incoming/report.md
```

Do not overwrite an existing inbox pathname. Reusing a basename with different
bytes may also conflict with the immutable work label derived from that name;
Annals records that job as failed rather than silently changing the label.


## Maintenance

Stop `annals-inbox.timer` before maintenance. Let the active service finish
before replacing executables, configuration, or library state. Restart the
timer after readiness checks. Use `inbox pause` for ordinary dispatch control.
