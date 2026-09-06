fn main() -> std::process::ExitCode {
    cell_install::simple::main(
        &clockwork::installation::specification(),
        env!("CARGO_PKG_VERSION"),
    )
}
