//! Exercise `SQLite` preview and authorized Email delivery in a prepared
//! isolated library without changing its eligibility fields.
use anyhow::{Context, Result, ensure};
use platter::{ad_hoc, store::Store};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 4,
        "usage: ad_hoc_canary STATE_ROOT DAY OCCURRENCE_ID EMAIL_EXECUTABLE"
    );
    let state = PathBuf::from(&args[0]);
    let email = PathBuf::from(&args[3]);
    let before = Store::open_read_only(&state)?.jobs()?;
    let edition = ad_hoc::preview(&state, &args[1], &args[2], &[], None)?;
    println!(
        "Preview: {} ({} PDF attachments)",
        edition.subject,
        edition.attachments.len()
    );
    let edition = ad_hoc::send(&state, &args[1], &args[2], Some(&email))?;
    let receipt = edition
        .receipt
        .as_deref()
        .context("Email acceptance receipt missing")?;
    ensure!(edition.status == "sent", "edition was not accepted");
    let store = Store::open_read_only(&state)?;
    ensure!(
        serde_json::to_value(&before)? == serde_json::to_value(store.jobs()?)?,
        "ad hoc delivery changed eligibility"
    );
    let retained = store
        .edition(&format!("ad-hoc/{}", args[2]))?
        .context("edition receipt was not retained")?;
    ensure!(
        retained.receipt == edition.receipt && retained.status == "sent",
        "stored acceptance differs from the send result"
    );
    println!("{receipt}");
    println!(
        "Retained: {}",
        state.join(platter::store::DATABASE).display()
    );
    println!("Job eligibility unchanged.");
    Ok(())
}
