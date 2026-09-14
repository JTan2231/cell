//! Annals owns library and Clockwork lifecycle; cell-install owns program files.
mod installation;
#[path = "../sqlite.rs"]
mod sqlite;

fn main() -> std::process::ExitCode {
    installation::main()
}
