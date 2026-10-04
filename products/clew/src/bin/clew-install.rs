fn main() -> std::process::ExitCode {
    use clap::Parser as _;
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|value| value == "schedule-definition")
    {
        let args = clew::installation::ScheduleDefinitionArgs::parse_from(arguments);
        return match clew::installation::schedule_definition(args) {
            Ok(data) => {
                println!("{}", serde_json::json!({"ok":true,"data":data}));
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                println!(
                    "{}",
                    serde_json::json!({"ok":false,"error":{"detail":format!("{error:#}")}})
                );
                std::process::ExitCode::FAILURE
            }
        };
    }
    if arguments.is_empty() || arguments == ["--help"] || arguments == ["-h"] {
        println!(
            "clew-install {}\n\ndeploy < REQUEST.json\ninstall --binary ABS --bundle ABS [--home ABS] [--expected-current absent|releases/ID]\ninspect [--home ABS]\nrecover --release ABS [--home ABS] [--expected-current absent|releases/ID]\nschedule-definition --state-dir ABS --output ABS [--home ABS]\n\nDirect install selects program files only. Deploy also initializes state and publishes the owned schedule.",
            env!("CARGO_PKG_VERSION")
        );
        return std::process::ExitCode::SUCCESS;
    }
    if let Err(error) = require_inactive_schedules(&arguments) {
        println!(
            "{}",
            serde_json::json!({"ok":false,"error":{"detail":error.to_string()}})
        );
        return std::process::ExitCode::FAILURE;
    }
    cell_install::simple::main_with_deployment(
        &clew::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        clew::installation::deploy,
    )
}

fn require_inactive_schedules(arguments: &[String]) -> anyhow::Result<()> {
    if !arguments
        .first()
        .is_some_and(|operation| matches!(operation.as_str(), "install" | "recover"))
    {
        return Ok(());
    }
    let home = arguments[1..]
        .windows(2)
        .find(|pair| pair[0] == "--home")
        .map(|pair| std::path::PathBuf::from(&pair[1]))
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .ok_or_else(|| anyhow::anyhow!("HOME or --home is required"))?;
    anyhow::ensure!(home.is_absolute(), "installer home must be absolute");
    let client = clockwork::api::Client::new(home.join(".local/bin/clockwork")).with_home(&home);
    let key = "clew/daily-email";
    let schedule = clockwork::deployment::ScheduleState::capture_installed(&home, key)?;
    anyhow::ensure!(
        !schedule
            .binding
            .as_ref()
            .is_some_and(|binding| binding.enabled),
        "disable {key} before file-only installation or recovery"
    );
    if schedule.binding.is_some() {
        schedule.suspend(&client, key)?;
    }
    Ok(())
}
