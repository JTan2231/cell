//! Annals owns library and Clockwork lifecycle; cell-install owns program files.
mod installation;

fn main() -> std::process::ExitCode {
    installation::main()
}
