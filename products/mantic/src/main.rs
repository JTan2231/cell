use std::fmt::Write as _;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{ArgMatches, Args, FromArgMatches, Parser, Subcommand, error::ErrorKind};
use mantic::store::{ItemChanges, ItemInput, Store};
use mantic::{
    Config, ConfigItem, ExpenseRule, ExpenseSource, ForecastInput, ForecastResult, RepeatUnit,
    database_path, forecast_with_adjustments, format_amount, parse_amount, parse_date,
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
    Forecast(ForecastArgs),
}

#[derive(Args, Debug)]
struct RawForecastArgs {
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
    /// Exclude a saved expense ID from this run; repeat to exclude more.
    #[arg(long, value_name = "ITEM_ID")]
    exclude: Vec<i64>,
    /// Start a temporary expense; following scheduling flags apply to this inclusion.
    #[arg(long, num_args = 2, value_names = ["NAME", "AMOUNT"], allow_hyphen_values = true)]
    include: Vec<String>,
    /// First due date for the preceding inclusion; defaults to the forecast start.
    #[arg(long, value_parser = parse_date, value_name = "DATE")]
    first: Vec<Date>,
    /// Recurrence for the preceding inclusion; omit for a one-off expense.
    #[arg(long, value_enum, value_name = "UNIT")]
    every: Vec<RepeatUnit>,
    /// Recurrence interval for the preceding inclusion; defaults to one.
    #[arg(long, value_name = "N")]
    interval: Vec<u32>,
    /// Inclusive cutoff for the preceding inclusion.
    #[arg(long, value_parser = parse_date, value_name = "DATE")]
    end: Vec<Date>,
}

#[derive(Debug)]
struct ForecastArgs {
    config: String,
    amount: i64,
    from: Option<Date>,
    until: Date,
    details: bool,
    exclude: Vec<i64>,
    include: Vec<IncludedArgs>,
}

#[derive(Debug)]
struct IncludedArgs {
    name: String,
    amount: i64,
    first: Option<Date>,
    every: Option<RepeatUnit>,
    interval: Option<u32>,
    end: Option<Date>,
}

enum InclusionOption {
    Include(String, i64),
    First(Date),
    Every(RepeatUnit),
    Interval(u32),
    End(Date),
}

impl Args for ForecastArgs {
    fn augment_args(command: clap::Command) -> clap::Command {
        RawForecastArgs::augment_args(command)
    }

    fn augment_args_for_update(command: clap::Command) -> clap::Command {
        RawForecastArgs::augment_args_for_update(command)
    }
}

impl FromArgMatches for ForecastArgs {
    fn from_arg_matches(matches: &ArgMatches) -> std::result::Result<Self, clap::Error> {
        let raw = RawForecastArgs::from_arg_matches(matches)?;
        let mut options = Vec::new();
        if let Some(indices) = matches.indices_of("include") {
            for (index, values) in indices.step_by(2).zip(raw.include.chunks_exact(2)) {
                let amount = parse_amount(&values[1]).map_err(|error| {
                    clap::Error::raw(
                        ErrorKind::InvalidValue,
                        format!("--include amount: {error}"),
                    )
                })?;
                options.push((index, InclusionOption::Include(values[0].clone(), amount)));
            }
        }
        append_inclusion_options(
            matches,
            "first",
            &raw.first,
            &mut options,
            InclusionOption::First,
        );
        append_inclusion_options(
            matches,
            "every",
            &raw.every,
            &mut options,
            InclusionOption::Every,
        );
        append_inclusion_options(
            matches,
            "interval",
            &raw.interval,
            &mut options,
            InclusionOption::Interval,
        );
        append_inclusion_options(matches, "end", &raw.end, &mut options, InclusionOption::End);
        let include = group_inclusions(options)?;
        Ok(Self {
            config: raw.config,
            amount: raw.amount,
            from: raw.from,
            until: raw.until,
            details: raw.details,
            exclude: raw.exclude,
            include,
        })
    }

    fn update_from_arg_matches(
        &mut self,
        matches: &ArgMatches,
    ) -> std::result::Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

fn group_inclusions(
    mut options: Vec<(usize, InclusionOption)>,
) -> std::result::Result<Vec<IncludedArgs>, clap::Error> {
    options.sort_by_key(|(index, _)| *index);
    let mut include: Vec<IncludedArgs> = Vec::new();
    for (_, option) in options {
        if let InclusionOption::Include(name, amount) = option {
            include.push(IncludedArgs {
                name,
                amount,
                first: None,
                every: None,
                interval: None,
                end: None,
            });
            continue;
        }
        let item = include.last_mut().ok_or_else(|| {
            clap::Error::raw(
                ErrorKind::MissingRequiredArgument,
                "expense scheduling flags require a preceding --include NAME AMOUNT",
            )
        })?;
        match option {
            InclusionOption::First(value) => set_inclusion_option(&mut item.first, value, "first")?,
            InclusionOption::Every(value) => set_inclusion_option(&mut item.every, value, "every")?,
            InclusionOption::Interval(value) => {
                set_inclusion_option(&mut item.interval, value, "interval")?;
            }
            InclusionOption::End(value) => set_inclusion_option(&mut item.end, value, "end")?,
            InclusionOption::Include(..) => unreachable!(),
        }
    }
    for item in &include {
        if item.interval.is_some() && item.every.is_none() {
            return Err(clap::Error::raw(
                ErrorKind::MissingRequiredArgument,
                "--interval requires --every in the same inclusion",
            ));
        }
    }
    Ok(include)
}

fn append_inclusion_options<T: Copy>(
    matches: &ArgMatches,
    name: &str,
    values: &[T],
    options: &mut Vec<(usize, InclusionOption)>,
    convert: impl Fn(T) -> InclusionOption,
) {
    if let Some(indices) = matches.indices_of(name) {
        options.extend(
            indices
                .zip(values)
                .map(|(index, value)| (index, convert(*value))),
        );
    }
}

fn set_inclusion_option<T>(
    slot: &mut Option<T>,
    value: T,
    name: &str,
) -> std::result::Result<(), clap::Error> {
    if slot.is_some() {
        return Err(clap::Error::raw(
            ErrorKind::ArgumentConflict,
            format!("--{name} may appear only once per inclusion"),
        ));
    }
    *slot = Some(value);
    Ok(())
}

impl Cli {
    fn output_schema(&self) -> u32 {
        if matches!(self.command, Command::Forecast(_)) {
            2
        } else {
            1
        }
    }
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
                    json!({"schema_version":cli.output_schema(),"ok":false,"error":{"code":"mantic_failed","message":format!("{error:#}")}})
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
        serde_json::to_string(
            &json!({"schema_version":cli.output_schema(),"ok":true,"result":result}),
        )?
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
        Command::Forecast(_)
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
        Command::Forecast(args) => {
            let config = store.get_config(&args.config)?;
            let result = calculate_forecast(&config, args)?;
            let text = forecast_text(&result, args.details)?;
            Ok((serde_json::to_value(result)?, text))
        }
    }
}

fn calculate_forecast(config: &Config, args: &ForecastArgs) -> Result<ForecastResult> {
    let from = if let Some(date) = args.from {
        date
    } else {
        OffsetDateTime::now_local()
            .context("cannot determine local date; supply --from")?
            .date()
    };
    let included = args
        .include
        .iter()
        .map(|item| {
            let mut rule = ExpenseRule {
                name: item.name.clone(),
                amount_cents: item.amount,
                first_due: item.first.unwrap_or(from),
                repeat_unit: item.every,
                repeat_every: item.every.map(|_| item.interval.unwrap_or(1)),
                end_date: item.end,
            };
            rule.validate()?;
            rule.name = rule.name.trim().to_owned();
            Ok(rule)
        })
        .collect::<Result<Vec<_>>>()?;
    forecast_with_adjustments(
        config,
        ForecastInput {
            starting_amount_cents: args.amount,
            from,
            until: args.until,
        },
        &args.exclude,
        &included,
    )
}

fn forecast_text(result: &ForecastResult, details: bool) -> Result<String> {
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
    for item in &result.excluded_items {
        write!(text, "\nExcluded: {}", item_line(item))?;
    }
    for item in &result.expenses {
        if let ExpenseSource::Included { ordinal } = item.source {
            write!(
                text,
                "\nIncluded {ordinal}: {}\t{}",
                item.rule.name,
                rule_line(&item.rule)
            )?;
        }
    }
    if details {
        for event in &result.occurrences {
            let source = match event.source {
                ExpenseSource::Saved { item_id } => item_id.to_string(),
                ExpenseSource::Included { ordinal } => format!("included {ordinal}"),
            };
            write!(
                text,
                "\n{}\t{} ({source})\t{}\tremaining {}",
                event.date,
                event.item_name,
                format_amount(event.amount_cents),
                format_amount(event.remainder_cents)
            )?;
        }
    }
    Ok(text)
}

fn rule_line(rule: &ExpenseRule) -> String {
    let recurrence = match (rule.repeat_unit, rule.repeat_every) {
        (Some(unit), Some(every)) => format!("every {every} {}", unit.as_str()),
        _ => "once".into(),
    };
    let end = rule
        .end_date
        .map_or_else(String::new, |date| format!(" through {date}"));
    format!(
        "{}\tfrom {}\t{recurrence}{end}",
        format_amount(rule.amount_cents),
        rule.first_due
    )
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
    format!(
        "{}\t{}\t{}",
        item.id,
        item.name,
        rule_line(&ExpenseRule::from(item))
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
        let Command::Forecast(args) = cli.command else {
            anyhow::bail!("wrong command");
        };
        assert_eq!(args.amount, -1);
        assert_eq!(args.from, Some(parse_date("2026-10-02")?));
        assert_eq!(args.until, parse_date("2026-11-02")?);
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

    #[test]
    fn ordinary_inclusion_blocks_keep_their_own_schedules() -> Result<()> {
        let cli = Cli::try_parse_from([
            "mantic",
            "forecast",
            "default",
            "5000",
            "--include",
            "Extra monthly",
            "500",
            "--every=month",
            "--include",
            "Repair",
            "200",
            "--first",
            "2026-11-01",
            "--exclude",
            "17",
            "--end=2026-11-01",
            "--include",
            "Quarterly",
            "30.50",
            "--every",
            "month",
            "--interval=3",
            "--from",
            "2026-10-02",
            "--until",
            "2026-12-31",
            "--json",
        ])?;
        assert!(cli.json);
        assert_eq!(cli.output_schema(), 2);
        let Command::Forecast(args) = cli.command else {
            anyhow::bail!("wrong command");
        };
        assert_eq!(args.exclude, vec![17]);
        assert_eq!(args.include.len(), 3);
        assert_eq!(args.include[0].name, "Extra monthly");
        assert_eq!(args.include[0].amount, 50_000);
        assert_eq!(args.include[0].every, Some(RepeatUnit::Month));
        assert_eq!(args.include[0].first, None);
        assert_eq!(args.include[1].every, None);
        assert_eq!(args.include[1].first, Some(parse_date("2026-11-01")?));
        assert_eq!(args.include[1].end, args.include[1].first);
        assert_eq!(args.include[2].amount, 3_050);
        assert_eq!(args.include[2].interval, Some(3));
        Ok(())
    }

    #[test]
    fn inclusion_modifiers_require_a_block_and_cannot_borrow_another_recurrence() {
        let base = [
            "mantic",
            "forecast",
            "default",
            "5000",
            "--until",
            "2026-12-31",
        ];
        let invalid = [
            vec!["--first", "2026-10-02", "--include", "Extra", "500"],
            vec!["--every", "month"],
            vec!["--interval", "1"],
            vec!["--end", "2026-12-31"],
            vec![
                "--include",
                "Monthly",
                "500",
                "--every",
                "month",
                "--include",
                "Once",
                "200",
                "--interval",
                "2",
            ],
            vec![
                "--include",
                "Once",
                "200",
                "--interval",
                "2",
                "--include",
                "Monthly",
                "500",
                "--every",
                "month",
            ],
            vec![
                "--include",
                "Extra",
                "500",
                "--every",
                "month",
                "--every",
                "week",
            ],
            vec![
                "--include",
                "Extra",
                "500",
                "--first",
                "2026-10-02",
                "--first",
                "2026-11-02",
            ],
            vec![
                "--include",
                "Extra",
                "500",
                "--every",
                "month",
                "--interval",
                "1",
                "--interval",
                "2",
            ],
            vec![
                "--include",
                "Extra",
                "500",
                "--end",
                "2026-12-31",
                "--end",
                "2026-11-30",
            ],
            vec!["--include", "Extra"],
            vec!["--include", "Extra", "1.001"],
        ];
        for suffix in invalid {
            let Err(error) = Cli::try_parse_from(base.into_iter().chain(suffix)) else {
                panic!("invalid inclusion accepted");
            };
            assert_eq!(error.exit_code(), 2);
        }
    }

    #[test]
    fn mixed_adjustments_calculate_and_explain_the_effective_expenses() -> Result<()> {
        let cli = Cli::try_parse_from([
            "mantic",
            "forecast",
            "default",
            "5000",
            "--until",
            "2026-12-31",
            "--exclude",
            "17",
            "--exclude",
            "17",
            "--include",
            " Extra monthly ",
            "500",
            "--every",
            "month",
            "--include",
            "Repair",
            "200",
            "--first",
            "2026-11-01",
            "--from",
            "2026-10-02",
        ])?;
        let Command::Forecast(args) = cli.command else {
            anyhow::bail!("wrong command");
        };
        let config = Config {
            id: 1,
            name: "default".to_owned(),
            items: vec![ConfigItem {
                id: 17,
                config_id: 1,
                name: "Robinhood".to_owned(),
                amount_cents: 6_000,
                first_due: parse_date("2026-01-01")?,
                repeat_unit: Some(RepeatUnit::Day),
                repeat_every: Some(1),
                end_date: None,
            }],
        };
        let original = config.clone();
        let result = calculate_forecast(&config, &args)?;
        assert_eq!(result.total_expenses_cents, 170_000);
        assert_eq!(result.remainder_cents, 330_000);
        assert_eq!(result.excluded_items, config.items);
        assert_eq!(result.expenses[0].rule.name, "Extra monthly");
        assert_eq!(result.expenses[0].rule.first_due, parse_date("2026-10-02")?);
        assert_eq!(result.expenses[0].rule.repeat_every, Some(1));
        assert_eq!(
            result
                .occurrences
                .iter()
                .map(|event| event.date)
                .collect::<Vec<_>>(),
            vec![
                parse_date("2026-10-02")?,
                parse_date("2026-11-01")?,
                parse_date("2026-11-02")?,
                parse_date("2026-12-02")?,
            ]
        );
        let text = forecast_text(&result, true)?;
        assert!(text.contains("Excluded: 17\tRobinhood\t60.00"));
        assert!(text.contains("Included 1: Extra monthly\t500.00\tfrom 2026-10-02\tevery 1 month"));
        assert!(text.contains("Repair (included 2)"));
        let value = serde_json::to_value(&result)?;
        assert_eq!(
            value["occurrences"][0]["source"],
            json!({"kind":"included","ordinal":1})
        );
        assert!(value["occurrences"][0].get("item_id").is_none());
        assert_eq!(config, original);
        Ok(())
    }

    #[test]
    fn temporary_expenses_share_validation_and_remain_visible_without_occurrences() -> Result<()> {
        let config = Config {
            id: 1,
            name: "default".to_owned(),
            items: vec![],
        };
        let base = [
            "mantic",
            "forecast",
            "default",
            "5000",
            "--from",
            "2026-10-02",
            "--until",
            "2026-12-31",
        ];
        for suffix in [
            vec!["--include", "Extra", "0"],
            vec!["--include", "Extra", "-1"],
            vec!["--include", " ", "1"],
            vec!["--include", "\tExtra", "1"],
            vec!["--include", "Extra\n", "1"],
            vec![
                "--include",
                "Extra",
                "1",
                "--every",
                "month",
                "--interval",
                "0",
            ],
            vec!["--include", "Extra", "1", "--end", "2026-10-01"],
        ] {
            let cli = Cli::try_parse_from(base.into_iter().chain(suffix))?;
            let Command::Forecast(args) = cli.command else {
                anyhow::bail!("wrong command");
            };
            assert!(calculate_forecast(&config, &args).is_err());
        }
        let cli = Cli::try_parse_from(base.into_iter().chain([
            "--include",
            "Future",
            "500",
            "--first",
            "2027-01-01",
            "--every",
            "month",
        ]))?;
        let Command::Forecast(args) = cli.command else {
            anyhow::bail!("wrong command");
        };
        let result = calculate_forecast(&config, &args)?;
        assert!(result.occurrences.is_empty());
        assert_eq!(result.expenses.len(), 1);
        assert!(
            forecast_text(&result, false)?.contains("Included 1: Future\t500.00\tfrom 2027-01-01")
        );
        assert_eq!(
            Cli::try_parse_from(["mantic", "config", "list"])?.output_schema(),
            1
        );
        Ok(())
    }
}
