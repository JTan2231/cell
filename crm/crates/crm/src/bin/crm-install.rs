fn main() -> std::process::ExitCode {
    cell_install::simple::main(
        &crm::installation::specification(),
        env!("CARGO_PKG_VERSION"),
    )
}
