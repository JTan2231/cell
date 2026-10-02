fn main() -> std::process::ExitCode {
    cell_install::simple::main_with_deployment(
        &clockwork::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        clockwork::installation::deploy,
    )
}
