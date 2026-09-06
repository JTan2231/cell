fn main() {
    let exit_code = chancery::run_cli();
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}
