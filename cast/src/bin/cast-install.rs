fn main() -> std::process::ExitCode {
    cell_install::simple::main_with_lifecycle(
        &cast::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        cast::installation::lifecycle,
    )
}
