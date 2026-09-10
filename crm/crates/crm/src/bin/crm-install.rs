fn main() -> std::process::ExitCode {
    cell_install::simple::main_with_lifecycle(
        &crm::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        crm::installation::lifecycle,
    )
}
