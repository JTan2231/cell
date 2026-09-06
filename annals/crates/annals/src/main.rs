fn main() {
    let exit_code = annals::run_cli();
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}
