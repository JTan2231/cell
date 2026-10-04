use clap::Parser;
use conatus::installation::{ScheduleDefinitionArgs, schedule_definition, specification};
use serde_json::json;
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|value| value == "schedule-definition")
    {
        let args = ScheduleDefinitionArgs::parse_from(arguments);
        return match schedule_definition(args) {
            Ok(data) => {
                println!("{}", json!({"ok": true, "data": data}));
                ExitCode::SUCCESS
            }
            Err(error) => {
                println!(
                    "{}",
                    json!({"ok": false, "error": {"detail": error.to_string()}})
                );
                ExitCode::FAILURE
            }
        };
    }
    if arguments.is_empty() || arguments == ["--help"] || arguments == ["-h"] {
        println!(
            "conatus-install {}\n\ndeploy < REQUEST.json\ninstall --binary ABS --bundle ABS [--home ABS] [--expected-current absent|releases/ID]\ninspect [--home ABS]\nrecover --release ABS [--home ABS] [--expected-current absent|releases/ID]\nschedule-definition --state-dir ABS --output ABS [--daily-email] [--home ABS]\n\nDirect install selects program files only. Deploy also updates owned configuration and schedules.",
            env!("CARGO_PKG_VERSION")
        );
        return ExitCode::SUCCESS;
    }
    if let Err(error) = require_inactive_schedules(&arguments) {
        println!(
            "{}",
            serde_json::json!({"ok":false,"error":{"detail":error.to_string()}})
        );
        return std::process::ExitCode::FAILURE;
    }
    cell_install::simple::main_with_deployment(
        &specification(),
        env!("CARGO_PKG_VERSION"),
        conatus::installation::deploy,
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
    for key in ["conatus/update", "conatus/daily-email"] {
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
    }
    Ok(())
}
