fn main() -> std::process::ExitCode {
    cell_install::simple::main_with_lifecycle(
        &clew::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        clew::installation::lifecycle,
    )
}
