fn main() -> std::process::ExitCode {
    cell_install::simple::main_with_lifecycle(
        &clockwork::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        clockwork::installation::deployment_lifecycle,
    )
}
