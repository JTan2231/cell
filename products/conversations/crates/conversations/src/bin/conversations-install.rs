fn main() -> std::process::ExitCode {
    cell_install::simple::main(
        &conversations::installation::specification(),
        env!("CARGO_PKG_VERSION"),
    )
}
