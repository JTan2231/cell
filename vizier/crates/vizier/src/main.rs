fn main() {
    if let Err(error) = vizier::run() {
        eprintln!("{}: {}", error.code(), error.message());
        std::process::exit(1);
    }
}
