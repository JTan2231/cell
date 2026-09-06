fn main() -> std::process::ExitCode {
    cell_install::simple::main(
        &email::installation::specification(),
        env!("CARGO_PKG_VERSION"),
    )
}
