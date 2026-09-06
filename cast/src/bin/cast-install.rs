fn main() -> std::process::ExitCode {
    cell_install::simple::main(
        &cast::installation::specification(),
        env!("CARGO_PKG_VERSION"),
    )
}
