fn option(arguments: &[String], name: &str) -> Option<std::path::PathBuf> {
    arguments[1..]
        .chunks_exact(2)
        .filter(|pair| pair[0] == name)
        .map(|pair| std::path::PathBuf::from(&pair[1]))
        .next_back()
}

fn main() -> std::process::ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|value| matches!(value.as_str(), "install" | "recover"))
    {
        let home = option(&arguments, "--home")
            .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from));
        let result = home
            .ok_or_else(|| cell_install::Error::new("HOME is unavailable"))
            .and_then(|home| {
                if arguments[0] == "recover" {
                    let release = option(&arguments, "--release")
                        .ok_or_else(|| cell_install::Error::new("--release is required"))?;
                    clockwork::installation::require_runtime_recovery(&release)?;
                }
                clockwork::installation::require_quiescent_installation(&home)
            });
        if let Err(error) = result {
            eprintln!("{error}");
            return std::process::ExitCode::FAILURE;
        }
    }
    cell_install::simple::main_with_deployment(
        &clockwork::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        clockwork::installation::deploy,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn preflight_uses_the_same_last_option_as_the_installer() {
        let arguments = [
            "recover",
            "--release",
            "/first",
            "--home",
            "/old-home",
            "--release",
            "/last",
            "--home",
            "/new-home",
        ]
        .map(str::to_owned);
        assert_eq!(super::option(&arguments, "--release"), Some("/last".into()));
        assert_eq!(
            super::option(&arguments, "--home"),
            Some("/new-home".into())
        );
    }
}
