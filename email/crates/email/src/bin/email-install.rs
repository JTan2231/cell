fn main() -> std::process::ExitCode {
    cell_install::simple::main_with_lifecycle(
        &email::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        email::installation::lifecycle,
    )
}
