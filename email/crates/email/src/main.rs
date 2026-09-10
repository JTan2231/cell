fn main() {
    if let Some(snapshot) = iatreion_api::requested_on_demand_snapshot_json(
        "email",
        env!("CARGO_PKG_VERSION"),
        "email/transport",
        "email.message.send",
    ) {
        println!("{snapshot}");
        return;
    }
    email::main_entry();
}
