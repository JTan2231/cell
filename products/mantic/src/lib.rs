//! Saved expense configurations and ephemeral cash forecasts.

pub mod installation;
pub mod store;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use time::{Date, Duration, Month};

pub const MAX_OCCURRENCES: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum RepeatUnit {
    Day,
    Week,
    Month,
    Year,
}

impl RepeatUnit {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigItem {
    pub id: i64,
    pub config_id: i64,
    pub name: String,
    pub amount_cents: i64,
    pub first_due: Date,
    pub repeat_unit: Option<RepeatUnit>,
    pub repeat_every: Option<u32>,
    pub end_date: Option<Date>,
}

impl ConfigItem {
    /// Check the complete expense rule.
    ///
    /// # Errors
    /// Returns an error for invalid identity, money, dates, or recurrence.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.id > 0 && self.config_id > 0,
            "item IDs must be positive"
        );
        validate_name(&self.name)?;
        ensure!(self.amount_cents > 0, "expense amount must be positive");
        validate_date(self.first_due)?;
        match (self.repeat_unit, self.repeat_every) {
            (None, None) | (Some(_), Some(1..)) => {}
            _ => bail!("recurrence needs a unit and a positive interval"),
        }
        if let Some(end) = self.end_date {
            validate_date(end)?;
            ensure!(end >= self.first_due, "end date precedes first due date");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub id: i64,
    pub name: String,
    pub items: Vec<ConfigItem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForecastInput {
    pub starting_amount_cents: i64,
    pub from: Date,
    pub until: Date,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occurrence {
    pub date: Date,
    pub item_id: i64,
    pub item_name: String,
    pub amount_cents: i64,
    pub remainder_cents: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForecastResult {
    pub config_id: i64,
    pub config_name: String,
    pub starting_amount_cents: i64,
    pub from: Date,
    pub until: Date,
    pub total_expenses_cents: i64,
    pub remainder_cents: i64,
    pub first_shortfall: Option<Date>,
    pub occurrences: Vec<Occurrence>,
}

#[must_use]
pub fn database_path(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Mantic/mantic.db")
}

/// Parse an exact calendar date in the supported range.
///
/// # Errors
/// Returns an error unless the input is a valid `YYYY-MM-DD` date in years 1–9999.
pub fn parse_date(input: &str) -> Result<Date> {
    let bytes = input.as_bytes();
    ensure!(
        bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes
                .iter()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit()),
        "date must use YYYY-MM-DD"
    );
    let format = time::format_description::parse_borrowed::<2>("[year]-[month]-[day]")?;
    let date = Date::parse(input, &format).context("invalid calendar date")?;
    validate_date(date)?;
    Ok(date)
}

/// Parse a signed decimal amount without floating-point rounding.
///
/// # Errors
/// Returns an error for invalid syntax, more than two decimal places, or overflow.
pub fn parse_amount(input: &str) -> Result<i64> {
    let (negative, unsigned) = if let Some(value) = input.strip_prefix('-') {
        (true, value)
    } else {
        (false, input.strip_prefix('+').unwrap_or(input))
    };
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    ensure!(
        !whole.is_empty()
            && whole.bytes().all(|c| c.is_ascii_digit())
            && fraction.len() <= 2
            && fraction.bytes().all(|c| c.is_ascii_digit())
            && (!unsigned.contains('.') || !fraction.is_empty()),
        "amount must be a decimal number with at most two decimal places"
    );
    let major = whole.parse::<i128>().context("amount is too large")?;
    let minor = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<i128>()?
    };
    let minor = if fraction.len() == 1 {
        minor * 10
    } else {
        minor
    };
    let cents = major
        .checked_mul(100)
        .and_then(|v| v.checked_add(minor))
        .context("amount is too large")?;
    i64::try_from(if negative { -cents } else { cents })
        .context("amount is outside the supported cents range")
}

#[must_use]
pub fn format_amount(cents: i64) -> String {
    let value = i128::from(cents).abs();
    format!(
        "{}{}.{:02}",
        if cents < 0 { "-" } else { "" },
        value / 100,
        value % 100
    )
}

pub(crate) fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.trim().is_empty() && !name.chars().any(char::is_control),
        "name must be nonempty and contain no control characters"
    );
    Ok(())
}

fn validate_date(date: Date) -> Result<()> {
    ensure!(
        (1..=9999).contains(&date.year()),
        "date must be between 0001-01-01 and 9999-12-31"
    );
    Ok(())
}

/// Calculate a complete forecast without changing its configuration.
///
/// # Errors
/// Returns an error for invalid inputs, arithmetic overflow, or over 100,000 occurrences.
pub fn forecast(config: &Config, input: ForecastInput) -> Result<ForecastResult> {
    ensure!(config.id > 0, "config ID must be positive");
    validate_name(&config.name)?;
    validate_date(input.from)?;
    validate_date(input.until)?;
    ensure!(
        input.from <= input.until,
        "forecast end precedes start date"
    );
    let mut ids = HashSet::new();
    let mut occurrences = Vec::new();
    for item in &config.items {
        item.validate()?;
        ensure!(
            item.config_id == config.id && ids.insert(item.id),
            "invalid config item membership or duplicate ID"
        );
        let cutoff = item
            .end_date
            .map_or(input.until, |end| end.min(input.until));
        let mut index = first_index(item, input.from);
        while let Some(date) = occurrence_date(item, index)? {
            if date > cutoff {
                break;
            }
            if date >= input.from {
                ensure!(
                    occurrences.len() < MAX_OCCURRENCES,
                    "forecast exceeds {MAX_OCCURRENCES} occurrences; choose a shorter period"
                );
                occurrences.push(Occurrence {
                    date,
                    item_id: item.id,
                    item_name: item.name.clone(),
                    amount_cents: item.amount_cents,
                    remainder_cents: 0,
                });
            }
            index += 1;
        }
    }
    occurrences.sort_by_key(|event| (event.date, event.item_id));
    let mut remainder = input.starting_amount_cents;
    let mut total = 0_i64;
    let mut first_shortfall = (remainder < 0).then_some(input.from);
    for event in &mut occurrences {
        total = total
            .checked_add(event.amount_cents)
            .context("expense total exceeds the supported cents range")?;
        remainder = remainder
            .checked_sub(event.amount_cents)
            .context("remaining amount exceeds the supported cents range")?;
        event.remainder_cents = remainder;
        if remainder < 0 && first_shortfall.is_none() {
            first_shortfall = Some(event.date);
        }
    }
    Ok(ForecastResult {
        config_id: config.id,
        config_name: config.name.clone(),
        starting_amount_cents: input.starting_amount_cents,
        from: input.from,
        until: input.until,
        total_expenses_cents: total,
        remainder_cents: remainder,
        first_shortfall,
        occurrences,
    })
}

fn first_index(item: &ConfigItem, from: Date) -> u64 {
    if from <= item.first_due {
        return 0;
    }
    let (Some(unit), Some(every)) = (item.repeat_unit, item.repeat_every) else {
        return 0;
    };
    let distance = match unit {
        RepeatUnit::Day => (from - item.first_due).whole_days(),
        RepeatUnit::Week => (from - item.first_due).whole_days() / 7,
        RepeatUnit::Month => month_index(from) - month_index(item.first_due),
        RepeatUnit::Year => i64::from(from.year() - item.first_due.year()),
    };
    // The positive date distance fits u64; validation precedes this calculation.
    u64::try_from(distance).unwrap_or_default() / u64::from(every)
}

fn month_index(date: Date) -> i64 {
    i64::from(date.year()) * 12 + i64::from(u8::from(date.month())) - 1
}

fn occurrence_date(item: &ConfigItem, index: u64) -> Result<Option<Date>> {
    let (Some(unit), Some(every)) = (item.repeat_unit, item.repeat_every) else {
        return Ok((index == 0).then_some(item.first_due));
    };
    let step = index
        .checked_mul(u64::from(every))
        .context("recurrence interval overflow")?;
    let step = i64::try_from(step).context("recurrence interval overflow")?;
    match unit {
        RepeatUnit::Day | RepeatUnit::Week => {
            let days = step
                .checked_mul(if unit == RepeatUnit::Week { 7 } else { 1 })
                .context("recurrence interval overflow")?;
            // Reject dates beyond the supported year before constructing a duration.
            let max = Date::from_calendar_date(9999, Month::December, 31)?;
            if days > (max - item.first_due).whole_days() {
                return Ok(None);
            }
            Ok(item.first_due.checked_add(Duration::days(days)))
        }
        RepeatUnit::Month | RepeatUnit::Year => {
            let months = step
                .checked_mul(if unit == RepeatUnit::Year { 12 } else { 1 })
                .context("recurrence interval overflow")?;
            let target = month_index(item.first_due)
                .checked_add(months)
                .context("recurrence date overflow")?;
            if target / 12 > 9999 {
                return Ok(None);
            }
            let year = i32::try_from(target / 12)?;
            let month = Month::try_from(u8::try_from(target % 12 + 1)?)?;
            let day = item.first_due.day().min(month.length(year));
            Ok(Some(Date::from_calendar_date(year, month, day)?))
        }
    }
}
