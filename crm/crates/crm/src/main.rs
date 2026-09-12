fn main() {
    if let Some(snapshot) = iatreion_api::requested_on_demand_snapshot_json(
        "crm",
        env!("CARGO_PKG_VERSION"),
        "crm/steward",
        "crm.steward.operate",
    ) {
        chancery_usage::observe("crm", "status-snapshot");
        println!("{snapshot}");
        return;
    }
    let exit_code = crm::main_entry();
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}
