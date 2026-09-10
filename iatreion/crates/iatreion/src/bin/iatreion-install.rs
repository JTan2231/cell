fn main() -> std::process::ExitCode {
    cell_install::simple::main(
        &iatreion::installation::specification(),
        env!("CARGO_PKG_VERSION"),
    )
}
