fn main() -> std::process::ExitCode {
    cell_install::simple::main_with_deployment(
        &milieu::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        milieu::installation::deploy,
    )
}
