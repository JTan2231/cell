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
            "conatus-install {}\n\ninstall --binary ABS --bundle ABS [--home ABS] [--expected-current absent|releases/HASH]\ninspect [--home ABS]\nverify --binary ABS --bundle ABS [--home ABS]\nverify-release ABS\nrecover --release ABS [--home ABS] [--expected-current absent|releases/HASH]\nschedule-definition --state-dir ABS --output ABS [--home ABS]\n\nProgram installation does not initialize runtime state or activate schedules.",
            env!("CARGO_PKG_VERSION")
        );
        return ExitCode::SUCCESS;
    }
    cell_install::simple::main(&specification(), env!("CARGO_PKG_VERSION"))
}
