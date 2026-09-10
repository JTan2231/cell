use std::fs;
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture {
    home: PathBuf,
}

impl Fixture {
    fn new() -> std::io::Result<Self> {
        let home = std::env::temp_dir()
            .canonicalize()?
            .join(format!("emt-migration-{}", uuid::Uuid::now_v7()));
        fs::DirBuilder::new().mode(0o700).create(&home)?;
        Ok(Self { home })
    }

    fn command(&self, args: &[&str], owner: Option<&str>) -> std::io::Result<Output> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_emt"));
        command
            .args(["--json"])
            .args(args)
            .current_dir(&self.home)
            .env("HOME", &self.home)
            .env_remove("CELL_DEPLOYMENT_RUN_ID");
        if let Some(owner) = owner {
            command.env("CELL_DEPLOYMENT_RUN_ID", owner);
        }
        command.output()
    }

    fn success(&self, args: &[&str], owner: Option<&str>) -> std::io::Result<()> {
        let output = self.command(args, owner)?;
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

#[test]
fn coordinator_can_backup_drained_state_inside_emt_root() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    fixture.success(&["init"], None)?;
    let root = fixture.home.join("Library/Application Support/EMT");
    let database = root.join("emt.sqlite3");
    let config = root.join("config.json");
    let original_database = fs::read(&database)?;
    let original_config = fs::read(&config)?;
    let owner = "migration-test";
    fixture.success(&["maintenance", "hold", owner], None)?;

    let backup = root.join(format!("emt-pre-migration-{owner}.sqlite"));
    let args = [
        "migrate",
        "--backup",
        backup.to_str().ok_or("backup path is not UTF-8")?,
    ];
    fixture.success(&args, Some(owner))?;
    fixture.success(&args, Some(owner))?;

    let config_backup = backup.with_extension("config.json");
    assert_eq!(fs::read(&backup)?, original_database);
    assert_eq!(fs::read(&config_backup)?, original_config);
    for path in [&backup, &config_backup] {
        assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
    }

    for live_path in [&database, &config] {
        let output = fixture.command(
            &[
                "migrate",
                "--backup",
                live_path.to_str().ok_or("live path is not UTF-8")?,
            ],
            Some(owner),
        )?;
        assert!(!output.status.success());
    }
    assert_eq!(fs::read(database)?, original_database);
    assert_eq!(fs::read(config)?, original_config);
    fixture.success(&["maintenance", "release", owner], None)?;
    Ok(())
}
