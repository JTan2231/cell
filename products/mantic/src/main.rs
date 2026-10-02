use std::fmt::Write as _;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use mantic::store::{ItemChanges, ItemInput, Store};
use mantic::{
    ConfigItem, ForecastInput, RepeatUnit, database_path, forecast, format_amount, parse_amount,
    parse_date,
};
use serde_json::{Value, json};
use time::{Date, OffsetDateTime};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Forecast remaining money from saved expense configurations"
)]
struct Cli {
    /// Select the single configuration database (absolute path).
    #[arg(long, global = true, value_parser = absolute_path)]
    database: Option<PathBuf>,
    /// Print structured output with exact integer-cent amounts.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Initialize empty configuration storage; preserve supported existing data.
    Init,
    /// Manage named collections of expense rules.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Manage the expense rules owned by a configuration.
    Item {
        #[command(subcommand)]
        command: ItemCommand,
    },
    /// Calculate without saving a balance, occurrence, or run.
    Forecast {
        config: String,
        #[arg(value_parser = parse_amount, allow_hyphen_values = true)]
        amount: i64,
        /// Starting day; defaults to the current local calendar date.
        #[arg(long, value_parser = parse_date)]
        from: Option<Date>,
        /// Inclusive final day.
        #[arg(long, value_parser = parse_date)]
        until: Date,
        /// Include each occurrence and running balance in text output.
        #[arg(long)]
        details: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    Create { name: String },
    List,
    Show { name: String },
    Rename { name: String, replacement: String },
    Delete { name: String },
}

#[derive(Subcommand, Debug)]
enum ItemCommand {
    Add {
        config: String,
        name: String,
        #[arg(value_parser = parse_amount, allow_hyphen_values = true)]
        amount: i64,
        #[arg(long, value_parser = parse_date)]
        first: Date,
        /// Omit for a one-off expense.
        #[arg(long, value_enum)]
        every: Option<RepeatUnit>,
        /// Number of recurrence units; defaults to one.
        #[arg(long, requires = "every")]
        interval: Option<u32>,
        #[arg(long, value_parser = parse_date)]
        end: Option<Date>,
    },
    List {
        config: String,
    },
    Update(UpdateArgs),
    Delete {
        id: i64,
    },
}

#[derive(Args, Debug)]
struct UpdateArgs {
    id: i64,
    #[arg(long)]
    name: Option<String>,
    #[arg(long, value_parser = parse_amount, allow_hyphen_values = true)]
    amount: Option<i64>,
    #[arg(long, value_parser = parse_date)]
    first: Option<Date>,
    #[arg(long, value_enum, conflicts_with = "once")]
    every: Option<RepeatUnit>,
    #[arg(long, conflicts_with = "once")]
    interval: Option<u32>,
    #[arg(long, value_parser = parse_date, conflicts_with = "clear_end")]
    end: Option<Date>,
    #[arg(long)]
    clear_end: bool,
    #[arg(long)]
    once: bool,
}

fn absolute_path(value: &str) -> std::result::Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err("database path must be absolute".into())
    }
}

fn main() -> ExitCode {
    let cli = chancery_usage::cli::parse::<Cli>("mantic", "");
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if cli.json {
                eprintln!(
                    "{}",
                    json!({"schema_version":1,"ok":false,"error":{"code":"mantic_failed","message":format!("{error:#}")}})
                );
            } else {
                eprintln!("mantic: {error:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    let (result, text) = execute(cli)?;
    let output = if cli.json {
        serde_json::to_string(&json!({"schema_version":1,"ok":true,"result":result}))?
    } else {
        text
    };
    writeln!(std::io::stdout().lock(), "{output}").context("cannot write command output")
}

fn execute(cli: &Cli) -> Result<(Value, String)> {
    let path = if let Some(path) = &cli.database {
        path.clone()
    } else {
        database_path(&PathBuf::from(
            std::env::var_os("HOME").context("HOME is unavailable; supply --database")?,
        ))
    };
    if matches!(cli.command, Command::Init) {
        Store::initialize(&path)?;
        return Ok((
            json!({"initialized":true,"database":path,"schema_version":1}),
            format!("Initialized {}", path.display()),
        ));
    }
    let read_only = matches!(
        cli.command,
        Command::Forecast { .. }
            | Command::Config {
                command: ConfigCommand::List | ConfigCommand::Show { .. }
            }
            | Command::Item {
                command: ItemCommand::List { .. }
            }
    );
    let mut store = Store::open(&path, read_only)?;
    match &cli.command {
        Command::Init => unreachable!(),
        Command::Config { command } => execute_config(&mut store, command),
        Command::Item { command } => execute_item(&mut store, command),
        Command::Forecast {
            config,
            amount,
            from,
            until,
            details,
        } => {
            let from = if let Some(date) = from {
                *date
            } else {
                OffsetDateTime::now_local()
                    .context("cannot determine local date; supply --from")?
                    .date()
            };
            let config = store.get_config(config)?;
            let result = forecast(
                &config,
                ForecastInput {
                    starting_amount_cents: *amount,
                    from,
                    until: *until,
                },
            )?;
            let mut text = format!(
                "Config: {}\nPeriod: {} through {} (inclusive)\nStarting amount: {}\nExpected expenses: {}\nRemaining amount: {}",
                result.config_name,
                result.from,
                result.until,
                format_amount(result.starting_amount_cents),
                format_amount(result.total_expenses_cents),
                format_amount(result.remainder_cents)
            );
            if let Some(date) = result.first_shortfall {
                write!(text, "\nFirst shortfall: {date}")?;
            }
            if *details {
                for event in &result.occurrences {
                    write!(
                        text,
                        "\n{}\t{} ({})\t{}\tremaining {}",
                        event.date,
                        event.item_name,
                        event.item_id,
                        format_amount(event.amount_cents),
                        format_amount(event.remainder_cents)
                    )?;
                }
            }
            Ok((serde_json::to_value(result)?, text))
        }
    }
}

fn execute_config(store: &mut Store, command: &ConfigCommand) -> Result<(Value, String)> {
    match command {
        ConfigCommand::Create { name } => {
            let config = store.create_config(name)?;
            Ok((
                serde_json::to_value(&config)?,
                format!("Created config {} ({})", config.name, config.id),
            ))
        }
        ConfigCommand::List => {
            let configs = store.list_configs()?;
            let text = if configs.is_empty() {
                "No configs".into()
            } else {
                configs
                    .iter()
                    .map(|c| format!("{}\t{}", c.id, c.name))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            Ok((json!({"configs":configs}), text))
        }
        ConfigCommand::Show { name } => {
            let config = store.get_config(name)?;
            let text = format!(
                "Config {} ({})\n{}",
                config.name,
                config.id,
                item_lines(&config.items)
            );
            Ok((serde_json::to_value(config)?, text))
        }
        ConfigCommand::Rename { name, replacement } => {
            let config = store.rename_config(name, replacement)?;
            Ok((
                serde_json::to_value(&config)?,
                format!("Renamed config to {} ({})", config.name, config.id),
            ))
        }
        ConfigCommand::Delete { name } => {
            store.delete_config(name)?;
            Ok((
                json!({"deleted":true,"config_name":name.trim()}),
                format!("Deleted config {} and its items", name.trim()),
            ))
        }
    }
}

fn execute_item(store: &mut Store, command: &ItemCommand) -> Result<(Value, String)> {
    match command {
        ItemCommand::Add {
            config,
            name,
            amount,
            first,
            every,
            interval,
            end,
        } => {
            let item = store.add_item(
                config,
                &ItemInput {
                    name: name.clone(),
                    amount_cents: *amount,
                    first_due: *first,
                    repeat_unit: *every,
                    repeat_every: every.map(|_| interval.unwrap_or(1)),
                    end_date: *end,
                },
            )?;
            Ok((
                serde_json::to_value(&item)?,
                format!("Added {}", item_line(&item)),
            ))
        }
        ItemCommand::List { config } => {
            let config = store.get_config(config)?;
            let text = item_lines(&config.items);
            Ok((
                json!({"config_id":config.id,"config_name":config.name,"items":config.items}),
                text,
            ))
        }
        ItemCommand::Update(changes) => {
            let item = store.update_item(
                changes.id,
                ItemChanges {
                    name: changes.name.clone(),
                    amount_cents: changes.amount,
                    first_due: changes.first,
                    repeat_unit: changes.every,
                    repeat_every: changes.interval,
                    end_date: changes.end,
                    clear_end: changes.clear_end,
                    once: changes.once,
                },
            )?;
            Ok((
                serde_json::to_value(&item)?,
                format!("Updated {}", item_line(&item)),
            ))
        }
        ItemCommand::Delete { id } => {
            store.delete_item(*id)?;
            Ok((
                json!({"deleted":true,"item_id":id}),
                format!("Deleted item {id}"),
            ))
        }
    }
}

fn item_lines(items: &[ConfigItem]) -> String {
    if items.is_empty() {
        "No items".into()
    } else {
        items.iter().map(item_line).collect::<Vec<_>>().join("\n")
    }
}

fn item_line(item: &ConfigItem) -> String {
    let recurrence = match (item.repeat_unit, item.repeat_every) {
        (Some(unit), Some(every)) => format!("every {every} {}", unit.as_str()),
        _ => "once".into(),
    };
    let end = item
        .end_date
        .map_or_else(String::new, |date| format!(" through {date}"));
    format!(
        "{}\t{}\t{}\tfrom {}\t{recurrence}{end}",
        item.id,
        item.name,
        format_amount(item.amount_cents),
        item.first_due
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_definition_and_usage_inventory_are_complete() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
        let ids = chancery_usage::cli::command_ids(&Cli::command(), "");
        assert_eq!(ids.len(), 11);
        assert!(ids.contains(&"forecast".to_owned()));
        assert!(ids.contains(&"config.rename".to_owned()));
        assert!(ids.contains(&"item.update".to_owned()));
    }

    #[test]
    fn forecast_accepts_negative_amount_and_explicit_calendar_dates() -> Result<()> {
        let cli = Cli::try_parse_from([
            "mantic",
            "forecast",
            "home",
            "-0.01",
            "--from",
            "2026-10-02",
            "--until",
            "2026-11-02",
            "--json",
        ])?;
        assert!(cli.json);
        let Command::Forecast {
            amount,
            from,
            until,
            ..
        } = cli.command
        else {
            anyhow::bail!("wrong command");
        };
        assert_eq!(amount, -1);
        assert_eq!(from, Some(parse_date("2026-10-02")?));
        assert_eq!(until, parse_date("2026-11-02")?);
        Ok(())
    }

    #[test]
    fn invalid_dates_precision_and_relative_database_paths_are_syntax_errors() {
        assert!(
            Cli::try_parse_from([
                "mantic",
                "forecast",
                "home",
                "1.001",
                "--until",
                "2026-11-02"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from(["mantic", "forecast", "home", "1", "--until", "2026-02-30"])
                .is_err()
        );
        assert!(Cli::try_parse_from(["mantic", "--database", "home.db", "init"]).is_err());
        assert!(Cli::try_parse_from(["mantic", "forecast", "home", "1"]).is_err());
    }

    #[test]
    fn recurrence_updates_and_end_date_flags_have_clear_conflicts() {
        assert!(
            Cli::try_parse_from([
                "mantic", "item", "update", "1", "--once", "--every", "month"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "mantic",
                "item",
                "update",
                "1",
                "--end",
                "2026-11-02",
                "--clear-end"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "mantic",
                "item",
                "add",
                "home",
                "rent",
                "10",
                "--first",
                "2026-10-02",
                "--interval",
                "2"
            ])
            .is_err()
        );
        assert!(Cli::try_parse_from(["mantic", "item", "update", "1", "--interval", "2"]).is_ok());
    }
}
