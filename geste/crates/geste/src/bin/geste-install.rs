fn main() -> std::process::ExitCode {
    cell_install::simple::main(
        &geste::installation::specification(),
        env!("CARGO_PKG_VERSION"),
    )
}
