fn main() {
    let exit_code = crm::main_entry();
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}
