fn main() -> std::process::ExitCode {
    cell_install::simple::main_with_lifecycle(
        &bazaar::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        bazaar::installation::lifecycle,
    )
}
