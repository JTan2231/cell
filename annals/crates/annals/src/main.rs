fn main() {
    if let Some(snapshot) = iatreion_api::requested_status_snapshot_json(
        "annals",
        env!("CARGO_PKG_VERSION"),
        vec![
            iatreion_api::declared_unit(
                "annals",
                "annals/inbox",
                Some("annals/inbox"),
                iatreion_api::Intent::Active,
                "annals.inbox.operate",
            ),
            iatreion_api::declared_unit(
                "annals",
                "annals/decisions-inbox",
                Some("annals/decisions-inbox"),
                iatreion_api::Intent::Active,
                "annals.inbox.operate",
            ),
        ],
        false,
    ) {
        println!("{snapshot}");
        return;
    }
    let exit_code = annals::run_cli();
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}
