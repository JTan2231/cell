fn main() {
    if let Some(snapshot) = iatreion_api::requested_status_snapshot_json(
        "todo",
        env!("CARGO_PKG_VERSION"),
        vec![
            iatreion_api::on_demand_unit("todo", "todo/application", "todo.umbrella.manage"),
            iatreion_api::declared_unit(
                "todo",
                "todo/daily-email",
                Some("todo/daily-email"),
                iatreion_api::Intent::Active,
                "todo.install.operate",
            ),
        ],
        false,
    ) {
        println!("{snapshot}");
        return;
    }
    std::process::exit(todo::main_entry());
}
