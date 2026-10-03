use anyhow::Result;
use mantic::{
    Config, ConfigItem, ExpenseRule, ExpenseSource, ForecastInput, ForecastResult, MAX_OCCURRENCES,
    RepeatUnit, forecast, forecast_with_adjustments, format_amount, parse_amount, parse_date,
};
use time::{Date, Duration};

fn date(value: &str) -> Date {
    match parse_date(value) {
        Ok(date) => date,
        Err(error) => panic!("invalid test date {value}: {error:#}"),
    }
}

fn once(id: i64, amount_cents: i64, first_due: &str) -> ConfigItem {
    ConfigItem {
        id,
        config_id: 1,
        name: format!("item-{id}"),
        amount_cents,
        first_due: date(first_due),
        repeat_unit: None,
        repeat_every: None,
        end_date: None,
    }
}

fn recurring(
    id: i64,
    amount_cents: i64,
    first_due: &str,
    unit: RepeatUnit,
    every: u32,
) -> ConfigItem {
    ConfigItem {
        repeat_unit: Some(unit),
        repeat_every: Some(every),
        ..once(id, amount_cents, first_due)
    }
}

fn configuration(items: Vec<ConfigItem>) -> Config {
    Config {
        id: 1,
        name: "household".to_owned(),
        items,
    }
}

fn run(config: &Config, amount: i64, from: &str, until: &str) -> ForecastResult {
    match forecast(
        config,
        ForecastInput {
            starting_amount_cents: amount,
            from: date(from),
            until: date(until),
        },
    ) {
        Ok(result) => result,
        Err(error) => panic!("unexpected forecast failure for {from} through {until}: {error:#}"),
    }
}

fn occurrence_dates(result: &ForecastResult) -> Vec<Date> {
    result.occurrences.iter().map(|entry| entry.date).collect()
}

#[test]
fn money_parser_preserves_cents_and_the_full_signed_range() -> Result<()> {
    for (input, expected) in [
        ("0", 0),
        ("12", 1_200),
        ("12.3", 1_230),
        ("12.34", 1_234),
        ("-0.01", -1),
        ("-12.34", -1_234),
        ("92233720368547758.07", i64::MAX),
        ("-92233720368547758.08", i64::MIN),
    ] {
        assert_eq!(parse_amount(input)?, expected, "{input}");
    }
    Ok(())
}

#[test]
fn money_parser_rejects_fractional_cents_overflow_and_nonnumbers() {
    for input in [
        "",
        "-",
        "1.234",
        "1e3",
        "1.2.3",
        "NaN",
        "92233720368547758.08",
        "-92233720368547758.09",
    ] {
        assert!(parse_amount(input).is_err(), "{input}");
    }
}

#[test]
fn displayed_amounts_round_trip_without_losing_cents_or_overflowing() -> Result<()> {
    for cents in [i64::MIN, -100, -1, 0, 1, 123, i64::MAX] {
        assert_eq!(parse_amount(&format_amount(cents))?, cents);
    }
    assert_eq!(format_amount(i64::MIN), "-92233720368547758.08");
    assert_eq!(format_amount(-1), "-0.01");
    Ok(())
}

#[test]
fn dates_follow_the_calendar_and_require_the_documented_format() {
    assert!(parse_date("2000-02-29").is_ok());
    assert!(parse_date("2024-02-29").is_ok());
    for input in [
        "1900-02-29",
        "2023-02-29",
        "2024-04-31",
        "2024-13-01",
        "2024-2-01",
        "2024-01",
        "tomorrow",
    ] {
        assert!(parse_date(input).is_err(), "{input}");
    }
}

#[test]
fn one_off_expenses_include_both_boundaries_and_exclude_other_dates() {
    let config = configuration(vec![
        once(1, 700, "2024-01-01"),
        once(2, 1_000, "2024-01-02"),
        once(3, 2_000, "2024-01-05"),
        once(4, 900, "2024-01-06"),
    ]);
    let result = run(&config, 10_000, "2024-01-02", "2024-01-05");
    assert_eq!(
        occurrence_dates(&result),
        vec![date("2024-01-02"), date("2024-01-05")]
    );
    assert_eq!(result.total_expenses_cents, 3_000);
    assert_eq!(result.remainder_cents, 7_000);
    assert_eq!(result.first_shortfall, None);

    let one_day = run(&config, 10_000, "2024-01-02", "2024-01-02");
    assert_eq!(one_day.occurrences.len(), 1);
    assert_eq!(one_day.remainder_cents, 9_000);
}

#[test]
fn monthly_clamping_keeps_the_original_day_after_a_short_month() {
    let config = configuration(vec![recurring(1, 100, "2024-01-31", RepeatUnit::Month, 1)]);
    let result = run(&config, 1_000, "2024-01-01", "2024-04-30");
    assert_eq!(
        occurrence_dates(&result),
        vec![
            date("2024-01-31"),
            date("2024-02-29"),
            date("2024-03-31"),
            date("2024-04-30"),
        ]
    );
    assert_eq!(result.total_expenses_cents, 400);
    assert_eq!(result.remainder_cents, 600);
}

#[test]
fn yearly_clamping_returns_to_leap_day_when_it_is_available() {
    let config = configuration(vec![recurring(1, 100, "2020-02-29", RepeatUnit::Year, 1)]);
    let result = run(&config, 1_000, "2020-01-01", "2024-02-29");
    assert_eq!(
        occurrence_dates(&result),
        vec![
            date("2020-02-29"),
            date("2021-02-28"),
            date("2022-02-28"),
            date("2023-02-28"),
            date("2024-02-29"),
        ]
    );

    let century = configuration(vec![recurring(1, 100, "2000-02-29", RepeatUnit::Year, 100)]);
    let result = run(&century, 1_000, "2100-01-01", "2400-02-29");
    assert_eq!(
        occurrence_dates(&result),
        vec![
            date("2100-02-28"),
            date("2200-02-28"),
            date("2300-02-28"),
            date("2400-02-29"),
        ]
    );
}

#[test]
fn every_n_recurrences_stay_aligned_with_the_anchor() {
    let cases = [
        (
            RepeatUnit::Day,
            3,
            "2024-03-01",
            "2024-03-02",
            "2024-03-10",
            vec!["2024-03-04", "2024-03-07", "2024-03-10"],
        ),
        (
            RepeatUnit::Week,
            2,
            "2024-02-26",
            "2024-03-01",
            "2024-04-01",
            vec!["2024-03-11", "2024-03-25"],
        ),
        (
            RepeatUnit::Month,
            2,
            "2024-01-31",
            "2024-02-01",
            "2024-07-31",
            vec!["2024-03-31", "2024-05-31", "2024-07-31"],
        ),
        (
            RepeatUnit::Year,
            2,
            "2020-02-29",
            "2021-01-01",
            "2024-03-01",
            vec!["2022-02-28", "2024-02-29"],
        ),
    ];
    for (unit, every, anchor, from, until, expected) in cases {
        let config = configuration(vec![recurring(1, 100, anchor, unit, every)]);
        let result = run(&config, 1_000, from, until);
        assert_eq!(
            occurrence_dates(&result),
            expected.into_iter().map(date).collect::<Vec<_>>()
        );
    }
}

#[test]
fn an_end_date_is_an_inclusive_cutoff_and_need_not_be_a_due_date() {
    let mut config = configuration(vec![recurring(1, 100, "2024-01-15", RepeatUnit::Month, 1)]);
    config.items[0].end_date = Some(date("2024-03-15"));
    let inclusive = run(&config, 1_000, "2024-01-01", "2024-04-30");
    assert_eq!(
        occurrence_dates(&inclusive),
        vec![date("2024-01-15"), date("2024-02-15"), date("2024-03-15"),]
    );

    config.items[0].end_date = Some(date("2024-03-14"));
    let cutoff = run(&config, 1_000, "2024-01-01", "2024-04-30");
    assert_eq!(
        occurrence_dates(&cutoff),
        vec![date("2024-01-15"), date("2024-02-15")]
    );
    let expired = run(&config, 1_000, "2024-04-01", "2024-04-30");
    assert!(expired.occurrences.is_empty());
    assert_eq!(expired.remainder_cents, 1_000);
}

#[test]
fn ancient_anchors_skip_history_and_stop_at_the_maximum_date() {
    let daily = configuration(vec![recurring(1, 100, "0001-01-01", RepeatUnit::Day, 1)]);
    let result = run(&daily, 1_000, "9999-12-30", "9999-12-31");
    assert_eq!(
        occurrence_dates(&result),
        vec![date("9999-12-30"), date("9999-12-31")]
    );
    assert_eq!(result.total_expenses_cents, 200);

    let monthly = configuration(vec![recurring(1, 100, "0001-01-31", RepeatUnit::Month, 1)]);
    let result = run(&monthly, 1_000, "9999-10-01", "9999-12-31");
    assert_eq!(
        occurrence_dates(&result),
        vec![date("9999-10-31"), date("9999-11-30"), date("9999-12-31"),]
    );
}

#[test]
fn large_intervals_do_not_overflow_when_only_the_anchor_is_in_range() {
    for unit in [
        RepeatUnit::Day,
        RepeatUnit::Week,
        RepeatUnit::Month,
        RepeatUnit::Year,
    ] {
        let config = configuration(vec![recurring(1, 100, "2024-01-01", unit, u32::MAX)]);
        let result = run(&config, 1_000, "2024-01-01", "2024-01-02");
        assert_eq!(occurrence_dates(&result), vec![date("2024-01-01")]);
        assert_eq!(result.remainder_cents, 900);
    }
}

#[test]
fn same_day_expenses_sort_by_id_and_explain_running_remainders() {
    let config = configuration(vec![
        once(9, 30, "2024-01-03"),
        once(2, 50, "2024-01-03"),
        once(5, 80, "2024-01-03"),
        once(10, 1, "2024-01-02"),
    ]);
    let result = run(&config, 101, "2024-01-01", "2024-01-03");
    let entries = result
        .occurrences
        .iter()
        .map(|entry| {
            (
                entry.source,
                entry.item_name.as_str(),
                entry.amount_cents,
                entry.remainder_cents,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        entries,
        vec![
            (ExpenseSource::Saved { item_id: 10 }, "item-10", 1, 100),
            (ExpenseSource::Saved { item_id: 2 }, "item-2", 50, 50),
            (ExpenseSource::Saved { item_id: 5 }, "item-5", 80, -30),
            (ExpenseSource::Saved { item_id: 9 }, "item-9", 30, -60),
        ]
    );
    assert_eq!(result.total_expenses_cents, 161);
    assert_eq!(result.remainder_cents, -60);
    assert_eq!(result.first_shortfall, Some(date("2024-01-03")));
}

#[test]
fn shortfalls_include_a_negative_start_but_do_not_include_zero() {
    let empty = configuration(vec![]);
    let negative = run(&empty, -1, "2024-01-01", "2024-01-31");
    assert_eq!(negative.first_shortfall, Some(date("2024-01-01")));
    assert_eq!(negative.remainder_cents, -1);

    let zero = run(&empty, 0, "2024-01-01", "2024-01-31");
    assert_eq!(zero.first_shortfall, None);

    let config = configuration(vec![once(1, 100, "2024-01-02"), once(2, 1, "2024-01-03")]);
    let exactly_zero = run(&config, 100, "2024-01-01", "2024-01-02");
    assert_eq!(exactly_zero.remainder_cents, 0);
    assert_eq!(exactly_zero.first_shortfall, None);
    let later_negative = run(&config, 100, "2024-01-01", "2024-01-03");
    assert_eq!(later_negative.first_shortfall, Some(date("2024-01-03")));
}

#[test]
fn repeated_forecasts_do_not_consume_money_or_advance_the_schedule() {
    let config = configuration(vec![recurring(1, 125, "2024-01-31", RepeatUnit::Month, 1)]);
    let saved = config.clone();
    let original = run(&config, 1_000, "2024-01-01", "2024-03-31");
    let shorter = run(&config, 1_000, "2024-02-01", "2024-02-29");
    let repeated = run(&config, 1_000, "2024-01-01", "2024-03-31");
    assert_eq!(original.total_expenses_cents, 375);
    assert_eq!(shorter.total_expenses_cents, 125);
    assert_eq!(repeated.remainder_cents, 625);
    assert_eq!(original, repeated);
    assert_eq!(config, saved);
}

#[test]
fn forecast_json_uses_calendar_dates_and_exact_integer_cents() -> Result<()> {
    let config = configuration(vec![recurring(1, 100, "2024-01-31", RepeatUnit::Month, 1)]);
    let result = run(&config, i64::MAX, "2024-02-01", "2024-02-29");
    let value = serde_json::to_value(result)?;
    assert_eq!(value["from"], "2024-02-01");
    assert_eq!(value["until"], "2024-02-29");
    assert_eq!(value["starting_amount_cents"].as_i64(), Some(i64::MAX));
    assert_eq!(value["total_expenses_cents"].as_i64(), Some(100));
    assert_eq!(value["remainder_cents"].as_i64(), Some(i64::MAX - 100));
    assert_eq!(value["occurrences"][0]["date"], "2024-02-29");
    assert_eq!(value["occurrences"][0]["source"]["kind"], "saved");
    assert_eq!(value["occurrences"][0]["source"]["item_id"], 1);
    assert!(value["occurrences"][0].get("item_id").is_none());
    assert_eq!(value["expenses"][0]["source"]["kind"], "saved");
    assert_eq!(value["expenses"][0]["first_due"], "2024-01-31");
    assert_eq!(value["expenses"][0]["amount_cents"], 100);
    assert_eq!(value["excluded_items"], serde_json::json!([]));
    assert!(value["first_shortfall"].is_null());

    let result = run(&config, 99, "2024-02-01", "2024-02-29");
    let value = serde_json::to_value(result)?;
    assert_eq!(value["first_shortfall"], "2024-02-29");
    assert_eq!(value["remainder_cents"].as_i64(), Some(-1));
    Ok(())
}

#[test]
fn the_occurrence_limit_returns_complete_results_or_an_error() -> Result<()> {
    let config = configuration(vec![recurring(1, 1, "2024-01-01", RepeatUnit::Day, 1)]);
    let limit = i64::try_from(MAX_OCCURRENCES)?;
    let input = ForecastInput {
        starting_amount_cents: limit,
        from: date("2024-01-01"),
        until: date("2024-01-01") + Duration::days(limit - 1),
    };
    let result = forecast(&config, input)?;
    assert_eq!(result.occurrences.len(), MAX_OCCURRENCES);
    assert_eq!(result.total_expenses_cents, limit);
    assert_eq!(result.remainder_cents, 0);
    assert!(
        forecast(
            &config,
            ForecastInput {
                until: input.until + Duration::days(1),
                ..input
            }
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn invalid_windows_and_rules_are_rejected_at_the_engine_boundary() {
    let input = || ForecastInput {
        starting_amount_cents: 1_000,
        from: date("2024-01-01"),
        until: date("2024-01-31"),
    };
    assert!(
        forecast(
            &configuration(vec![]),
            ForecastInput {
                from: date("2024-02-01"),
                ..input()
            }
        )
        .is_err()
    );

    assert!(
        forecast(
            &configuration(vec![once(1, 100, "2024-01-01"), once(1, 100, "2024-01-02"),]),
            input(),
        )
        .is_err()
    );

    let mut invalid_items = vec![
        once(1, 0, "2024-01-01"),
        once(1, -1, "2024-01-01"),
        recurring(1, 100, "2024-01-01", RepeatUnit::Day, 0),
    ];
    let mut missing_unit = once(1, 100, "2024-01-01");
    missing_unit.repeat_every = Some(1);
    invalid_items.push(missing_unit);
    let mut missing_interval = once(1, 100, "2024-01-01");
    missing_interval.repeat_unit = Some(RepeatUnit::Day);
    invalid_items.push(missing_interval);
    let mut invalid_cutoff = once(1, 100, "2024-01-01");
    invalid_cutoff.end_date = Some(date("2023-12-31"));
    invalid_items.push(invalid_cutoff);
    let mut other_config = once(1, 100, "2024-01-01");
    other_config.config_id = 2;
    invalid_items.push(other_config);
    for item in invalid_items {
        assert!(forecast(&configuration(vec![item]), input()).is_err());
    }
}

#[test]
fn monetary_overflow_returns_an_error_instead_of_wrapping() -> Result<()> {
    let input = |amount| ForecastInput {
        starting_amount_cents: amount,
        from: date("2024-01-01"),
        until: date("2024-01-31"),
    };
    let overflowing_total = configuration(vec![
        once(1, i64::MAX, "2024-01-01"),
        once(2, 1, "2024-01-01"),
    ]);
    assert!(forecast(&overflowing_total, input(i64::MAX)).is_err());
    let underflowing_remainder = configuration(vec![once(1, 1, "2024-01-01")]);
    assert!(forecast(&underflowing_remainder, input(i64::MIN)).is_err());

    let empty = configuration(vec![]);
    assert_eq!(forecast(&empty, input(i64::MAX))?.remainder_cents, i64::MAX);
    assert_eq!(forecast(&empty, input(i64::MIN))?.remainder_cents, i64::MIN);
    Ok(())
}

#[test]
fn adjustments_compose_an_ephemeral_bundle_and_keep_saved_anchors() -> Result<()> {
    let config = configuration(vec![
        recurring(17, 6_000, "2026-01-01", RepeatUnit::Day, 1),
        recurring(23, 1_000, "2026-01-31", RepeatUnit::Month, 1),
    ]);
    let saved = config.clone();
    let extra = ExpenseRule {
        name: "Extra monthly".to_owned(),
        ..ExpenseRule::from(&recurring(1, 50_000, "2026-10-02", RepeatUnit::Month, 1))
    };
    let input = ForecastInput {
        starting_amount_cents: 500_000,
        from: date("2026-10-02"),
        until: date("2026-12-31"),
    };
    let result =
        forecast_with_adjustments(&config, input, &[17, 17], std::slice::from_ref(&extra))?;
    assert_eq!(result.excluded_items, vec![config.items[0].clone()]);
    assert_eq!(result.expenses.len(), 2);
    assert_eq!(
        result.expenses[0].source,
        ExpenseSource::Saved { item_id: 23 }
    );
    assert_eq!(result.expenses[0].rule.first_due, date("2026-01-31"));
    assert_eq!(
        result.expenses[1].source,
        ExpenseSource::Included { ordinal: 1 }
    );
    assert_eq!(result.expenses[1].rule, extra);
    assert_eq!(
        occurrence_dates(&result),
        vec![
            date("2026-10-02"),
            date("2026-10-31"),
            date("2026-11-02"),
            date("2026-11-30"),
            date("2026-12-02"),
            date("2026-12-31"),
        ]
    );
    assert_eq!(result.total_expenses_cents, 153_000);
    assert_eq!(result.remainder_cents, 347_000);
    assert_eq!(
        forecast_with_adjustments(&config, input, &[17], &[extra])?,
        result
    );
    assert_eq!(config, saved);
    assert_eq!(forecast(&config, input)?.occurrences.len(), 94);
    Ok(())
}

#[test]
fn identical_additions_remain_distinct_and_follow_saved_ids_on_each_date() -> Result<()> {
    let config = configuration(vec![
        once(9, 30, "2024-01-03"),
        once(2, 50, "2024-01-03"),
        once(10, 1, "2024-01-02"),
    ]);
    let included = ExpenseRule::from(&once(2, 50, "2024-01-03"));
    let result = forecast_with_adjustments(
        &config,
        ForecastInput {
            starting_amount_cents: 101,
            from: date("2024-01-01"),
            until: date("2024-01-03"),
        },
        &[],
        &[included.clone(), included],
    )?;
    assert_eq!(
        result
            .occurrences
            .iter()
            .map(|entry| (entry.source, entry.remainder_cents))
            .collect::<Vec<_>>(),
        vec![
            (ExpenseSource::Saved { item_id: 10 }, 100),
            (ExpenseSource::Saved { item_id: 2 }, 50),
            (ExpenseSource::Saved { item_id: 9 }, 20),
            (ExpenseSource::Included { ordinal: 1 }, -30),
            (ExpenseSource::Included { ordinal: 2 }, -80),
        ]
    );
    assert_eq!(result.total_expenses_cents, 181);
    assert_eq!(result.first_shortfall, Some(date("2024-01-03")));
    Ok(())
}

#[test]
fn composition_explains_rules_with_no_occurrences() -> Result<()> {
    let config = configuration(vec![once(1, 100, "2023-01-01"), once(2, 200, "2025-01-01")]);
    let included = ExpenseRule::from(&once(3, 300, "2025-02-01"));
    let result = forecast_with_adjustments(
        &config,
        ForecastInput {
            starting_amount_cents: 1_000,
            from: date("2024-01-01"),
            until: date("2024-01-31"),
        },
        &[2],
        &[included],
    )?;
    assert!(result.occurrences.is_empty());
    assert_eq!(result.expenses.len(), 2);
    assert_eq!(result.excluded_items, vec![config.items[1].clone()]);
    let json = serde_json::to_value(result)?;
    assert_eq!(
        json["expenses"][1]["source"],
        serde_json::json!({"kind":"included","ordinal":1})
    );
    assert_eq!(json["expenses"][1]["first_due"], "2025-02-01");
    assert_eq!(json["excluded_items"][0]["id"], 2);
    assert_eq!(json["excluded_items"][0]["amount_cents"], 200);
    Ok(())
}

#[test]
fn exclusions_must_belong_to_the_selected_snapshot_and_cannot_hide_invalid_items() {
    let config = configuration(vec![once(1, 100, "2024-01-01")]);
    let input = ForecastInput {
        starting_amount_cents: 1_000,
        from: date("2024-01-01"),
        until: date("2024-01-31"),
    };
    let foreign = Config {
        id: 2,
        name: "other".to_owned(),
        items: vec![ConfigItem {
            id: 2,
            config_id: 2,
            ..once(1, 100, "2024-01-01")
        }],
    };
    for excluded in [0, -1, 99, foreign.items[0].id] {
        let result = forecast_with_adjustments(&config, input, &[excluded], &[]);
        assert!(result.is_err(), "excluded ID {excluded}");
    }
    let invalid_configs = [
        configuration(vec![ConfigItem {
            config_id: 2,
            ..config.items[0].clone()
        }]),
        configuration(vec![ConfigItem {
            amount_cents: 0,
            ..config.items[0].clone()
        }]),
        configuration(vec![config.items[0].clone(), config.items[0].clone()]),
    ];
    for invalid in invalid_configs {
        assert!(forecast_with_adjustments(&invalid, input, &[1], &[]).is_err());
    }
}

#[test]
fn included_rules_use_the_same_validation_as_saved_rules() {
    let valid = ExpenseRule::from(&once(1, 100, "2024-01-01"));
    let invalid_rules = [
        ExpenseRule {
            name: " ".to_owned(),
            ..valid.clone()
        },
        ExpenseRule {
            name: "bad\nname".to_owned(),
            ..valid.clone()
        },
        ExpenseRule {
            amount_cents: 0,
            ..valid.clone()
        },
        ExpenseRule {
            amount_cents: -1,
            ..valid.clone()
        },
        ExpenseRule {
            repeat_unit: Some(RepeatUnit::Day),
            ..valid.clone()
        },
        ExpenseRule {
            repeat_every: Some(1),
            ..valid.clone()
        },
        ExpenseRule {
            repeat_unit: Some(RepeatUnit::Day),
            repeat_every: Some(0),
            ..valid.clone()
        },
        ExpenseRule {
            end_date: Some(date("2023-12-31")),
            ..valid
        },
    ];
    let input = ForecastInput {
        starting_amount_cents: 1_000,
        from: date("2024-01-01"),
        until: date("2024-01-31"),
    };
    for rule in invalid_rules {
        assert!(forecast_with_adjustments(&configuration(vec![]), input, &[], &[rule]).is_err());
    }
}

#[test]
fn the_occurrence_limit_applies_to_the_effective_bundle() -> Result<()> {
    let config = configuration(vec![
        recurring(1, 1, "2024-01-01", RepeatUnit::Day, 1),
        recurring(2, 1, "2024-01-01", RepeatUnit::Day, 1),
    ]);
    let limit = i64::try_from(MAX_OCCURRENCES)?;
    let input = ForecastInput {
        starting_amount_cents: limit,
        from: date("2024-01-01"),
        until: date("2024-01-01") + Duration::days(limit - 1),
    };
    let result = forecast_with_adjustments(&config, input, &[1], &[])?;
    assert_eq!(result.occurrences.len(), MAX_OCCURRENCES);
    assert_eq!(result.remainder_cents, 0);
    assert!(forecast(&config, input).is_err());
    let extra = ExpenseRule::from(&once(3, 1, "2024-01-01"));
    assert!(forecast_with_adjustments(&config, input, &[1], &[extra]).is_err());
    Ok(())
}

#[test]
fn adjustments_preserve_checked_money_arithmetic() -> Result<()> {
    let config = configuration(vec![once(1, i64::MAX, "2024-01-01")]);
    let extra = ExpenseRule::from(&once(2, 1, "2024-01-01"));
    let input = ForecastInput {
        starting_amount_cents: i64::MAX,
        from: date("2024-01-01"),
        until: date("2024-01-01"),
    };
    assert!(forecast_with_adjustments(&config, input, &[], std::slice::from_ref(&extra)).is_err());
    assert_eq!(
        forecast_with_adjustments(&config, input, &[1], std::slice::from_ref(&extra))?
            .remainder_cents,
        i64::MAX - 1
    );
    assert!(
        forecast_with_adjustments(
            &config,
            ForecastInput {
                starting_amount_cents: i64::MIN,
                ..input
            },
            &[1],
            &[extra]
        )
        .is_err()
    );
    Ok(())
}
