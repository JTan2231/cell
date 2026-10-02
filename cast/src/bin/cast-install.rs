fn main() -> std::process::ExitCode {
    cell_install::simple::main_with_deployment(
        &cast::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        cast::installation::deploy,
    )
}
