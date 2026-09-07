//! Exercise retained-material migration, `SQLite` preview and authorized Email
//! delivery without changing the installed library or its eligibility fields.
use anyhow::{Context, Result, ensure};
use platter::{ad_hoc, migration, store::Store};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 5,
        "usage: ad_hoc_canary SOURCE_ROOT SNAPSHOT_ROOT DAY OCCURRENCE_ID EMAIL_EXECUTABLE"
    );
    let source = PathBuf::from(&args[0]);
    let snapshot = PathBuf::from(&args[1]);
    let email = PathBuf::from(&args[4]);
    if !snapshot.exists() {
        migration::snapshot(&source, &snapshot)?;
    }
    let before = Store::open_read_only(&snapshot)?.jobs()?;
    let edition = ad_hoc::preview(&snapshot, &args[2], &args[3], &[], None)?;
    println!(
        "Preview: {} ({} PDF attachments)",
        edition.subject,
        edition.attachments.len()
    );
    let edition = ad_hoc::send(&snapshot, &args[2], &args[3], Some(&email))?;
    let receipt = edition
        .receipt
        .as_deref()
        .context("Email acceptance receipt missing")?;
    ensure!(edition.status == "sent", "edition was not accepted");
    let store = Store::open_read_only(&snapshot)?;
    ensure!(
        serde_json::to_value(&before)? == serde_json::to_value(store.jobs()?)?,
        "ad hoc delivery changed eligibility"
    );
    let retained = store
        .edition(&format!("ad-hoc/{}", args[3]))?
        .context("edition receipt was not retained")?;
    ensure!(
        retained.receipt == edition.receipt && retained.status == "sent",
        "stored acceptance differs from the send result"
    );
    println!("{receipt}");
    println!(
        "Retained: {}",
        snapshot.join(platter::store::DATABASE).display()
    );
    println!("Job eligibility unchanged; installed source library unchanged.");
    Ok(())
}
