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
            "clew-install {}\n\ninstall --binary ABS --bundle ABS [--home ABS] [--expected-current absent|releases/HASH]\ninspect [--home ABS]\nverify --binary ABS --bundle ABS [--home ABS]\nverify-release ABS\nrecover --release ABS [--home ABS] [--expected-current absent|releases/HASH]\nschedule-definition --state-dir ABS --output ABS [--home ABS]\n\nProgram installation does not initialize state or activate schedules.",
            env!("CARGO_PKG_VERSION")
        );
        return std::process::ExitCode::SUCCESS;
    }
    cell_install::simple::main_with_lifecycle(
        &clew::installation::specification(),
        env!("CARGO_PKG_VERSION"),
        clew::installation::lifecycle,
    )
}
