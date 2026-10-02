fn main() -> std::process::ExitCode {
    cell_install::simple::main_with_deployment(
        &mantic::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        mantic::installation::deploy,
    )
}
