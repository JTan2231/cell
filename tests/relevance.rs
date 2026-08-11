use std::error::Error;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::Command;

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::Value;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

#[derive(Debug, Deserialize)]
struct Judgment {
    query: String,
    scope_node_id: Option<i64>,
    kind: String,
    relevant_node_ids: Vec<i64>,
    preferred_primary_id: i64,
    case: String,
    #[serde(default)]
    required_branch_ids: Vec<i64>,
}

#[derive(Debug, Default)]
struct Metrics {
    found_at_5: usize,
    found_at_10: usize,
    expected: usize,
    reciprocal_rank_sum: f64,
    preferred_primary_found: usize,
    scoped_found_at_10: usize,
    scoped_expected: usize,
    required_branches_found: usize,
    required_branches_expected: usize,
}

fn command(library: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_annals"));
    command.arg("--library").arg(library).arg("--json");
    command
}

fn run_json<I, S>(library: &Path, arguments: I) -> TestResult<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = command(library).args(arguments).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "annals failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let envelope = serde_json::from_slice::<Value>(&output.stdout)?;
    Ok(envelope["data"].clone())
}

fn add(library: &Path, parent: i64, kind: &str, title: &str, body: &str) -> TestResult {
    run_json(
        library,
        [
            "node",
            "add",
            "--parent",
            &parent.to_string(),
            "--kind",
            kind,
            "--title",
            title,
            "--body",
            body,
        ],
    )?;
    Ok(())
}

fn seed(library: &Path) -> TestResult {
    run_json(library, ["init"])?;
    run_json(
        library,
        [
            "tree",
            "create",
            "--title",
            "Knowledge",
            "--body",
            "architecture overview",
        ],
    )?;
    add(
        library,
        1,
        "topic",
        "Transactions",
        "atomic isolation durability sharedbranch",
    )?;
    add(
        library,
        2,
        "source",
        "Write Skew Paper",
        "Serializable snapshot isolation permits the write skew anomaly.",
    )?;
    add(
        library,
        1,
        "topic",
        "Distributed Systems",
        "consensus and replication sharedbranch",
    )?;
    add(
        library,
        4,
        "source",
        "Quorum Notes",
        "Network partitions require a consensus quorum.",
    )?;
    add(
        library,
        1,
        "source",
        "C++ ABI",
        "Itanium C++ ABI identifiers and calling conventions.",
    )?;
    add(
        library,
        1,
        "topic",
        "Databases",
        "database indexing overview sharedbranch",
    )?;
    add(
        library,
        7,
        "topic",
        "Indexes",
        "B-tree lookup structures and page layouts.",
    )?;
    add(
        library,
        1,
        "topic",
        "Languages",
        "compiler and runtime overview",
    )?;
    add(
        library,
        9,
        "topic",
        "Indexes",
        "symbol indexing and reference data",
    )?;
    add(
        library,
        9,
        "source",
        "Rust Parser Notes",
        "Use serde_json::from_str for checked fixture parsing.",
    )?;
    let long_body = (0..2_500)
        .map(|word| {
            if word == 1_150 {
                "overlapneedle"
            } else {
                "filler"
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    add(library, 7, "source", "Long Passage Notes", &long_body)?;
    Ok(())
}

fn judgments() -> TestResult<Vec<Judgment>> {
    include_str!("fixtures/relevance.jsonl")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

fn visible_ids(results: &[Value]) -> Vec<i64> {
    let mut ids = Vec::new();
    for result in results {
        if let Some(node_id) = result["node_id"].as_i64() {
            ids.push(node_id);
        }
        if let Some(related) = result["related_hits"].as_array() {
            ids.extend(related.iter().filter_map(|hit| hit["node_id"].as_i64()));
        }
    }
    ids
}

fn result_group_is_relevant(result: &Value, relevant_node_ids: &[i64]) -> bool {
    result["node_id"]
        .as_i64()
        .is_some_and(|node_id| relevant_node_ids.contains(&node_id))
        || result["related_hits"].as_array().is_some_and(|related| {
            related.iter().any(|hit| {
                hit["node_id"]
                    .as_i64()
                    .is_some_and(|node_id| relevant_node_ids.contains(&node_id))
            })
        })
}

fn represented_branches(results: &[Value], scope_node_id: i64) -> BTreeSet<i64> {
    results
        .iter()
        .filter_map(|result| result["breadcrumb"].as_array())
        .filter_map(|breadcrumb| {
            let scope_index = breadcrumb
                .iter()
                .position(|item| item["node_id"].as_i64() == Some(scope_node_id))?;
            breadcrumb
                .get(scope_index + 1)
                .and_then(|item| item["node_id"].as_i64())
                .or(Some(scope_node_id))
        })
        .collect()
}

fn assert_documented_classes(judgments: &[Judgment]) {
    let cases = judgments
        .iter()
        .map(|judgment| judgment.case.as_str())
        .collect::<BTreeSet<_>>();
    for required in [
        "duplicate exact titles",
        "normalized full path",
        "scoped duplicate title",
        "missing-term OR fallback",
        "tokenizer identifier edge",
        "several relevant branches",
        "overlapping long passages",
        "ancestor and descendant direct matches",
        "incomplete final title token",
        "combined scope and kind filters",
    ] {
        assert!(
            cases.contains(required),
            "relevance fixture omitted documented class {required:?}"
        );
    }
}

fn evaluate_judgment(library: &Path, judgment: &Judgment, metrics: &mut Metrics) -> TestResult {
    let mut arguments = vec![
        "search".to_owned(),
        judgment.query.clone(),
        "--kind".to_owned(),
        judgment.kind.clone(),
        "--limit".to_owned(),
        "10".to_owned(),
    ];
    if let Some(scope) = judgment.scope_node_id {
        arguments.push("--within".to_owned());
        arguments.push(scope.to_string());
    }
    let data = run_json(library, &arguments)?;
    let results = data["results"]
        .as_array()
        .ok_or_else(|| io::Error::other("search response omitted results"))?;
    let visible_at_5 = visible_ids(&results[..results.len().min(5)]);
    let visible_at_10 = visible_ids(results);
    let expected_for_query = judgment.relevant_node_ids.len();
    let found_at_5 = judgment
        .relevant_node_ids
        .iter()
        .filter(|node_id| visible_at_5.contains(node_id))
        .count();
    let found_at_10 = judgment
        .relevant_node_ids
        .iter()
        .filter(|node_id| visible_at_10.contains(node_id))
        .count();
    metrics.expected += expected_for_query;
    metrics.found_at_5 += found_at_5;
    metrics.found_at_10 += found_at_10;

    if let Some(rank) = results
        .iter()
        .position(|result| result_group_is_relevant(result, &judgment.relevant_node_ids))
    {
        let one_based_rank = u32::try_from(rank.saturating_add(1))?;
        metrics.reciprocal_rank_sum += 1.0 / f64::from(one_based_rank);
    }
    if results
        .iter()
        .any(|result| result["node_id"].as_i64() == Some(judgment.preferred_primary_id))
    {
        metrics.preferred_primary_found += 1;
    }
    if let Some(scope_node_id) = judgment.scope_node_id {
        metrics.scoped_expected += expected_for_query;
        metrics.scoped_found_at_10 += found_at_10;
        let represented = represented_branches(results, scope_node_id);
        metrics.required_branches_expected += judgment.required_branch_ids.len();
        metrics.required_branches_found += judgment
            .required_branch_ids
            .iter()
            .filter(|branch_id| represented.contains(branch_id))
            .count();
    }

    if matches!(
        judgment.case.as_str(),
        "exact topic title" | "distinctive source phrase"
    ) {
        assert!(
            results
                .iter()
                .take(3)
                .any(|result| result["node_id"] == judgment.preferred_primary_id),
            "preferred result missing from top three for {judgment:?}"
        );
    }
    Ok(())
}

fn assert_and_report_metrics(metrics: &Metrics, judgment_count: usize) -> TestResult {
    assert!(metrics.expected > 0);
    assert!(metrics.scoped_expected > 0);
    assert!(metrics.required_branches_expected > 0);
    assert!(
        metrics.found_at_10.saturating_mul(100) >= metrics.expected.saturating_mul(90),
        "Recall@10 fell below the existing 0.90 release gate"
    );
    assert!(
        metrics.found_at_5.saturating_mul(100) >= metrics.expected.saturating_mul(80),
        "Recall@5 fell below 0.80"
    );
    assert!(
        metrics.scoped_found_at_10.saturating_mul(100)
            >= metrics.scoped_expected.saturating_mul(90),
        "scoped Recall@10 fell below 0.90"
    );
    assert_eq!(
        metrics.preferred_primary_found, judgment_count,
        "a preferred node was absent from the primary result groups"
    );
    assert_eq!(
        metrics.required_branches_found, metrics.required_branches_expected,
        "a required branch was absent from the first ten primary groups"
    );

    let expected = f64::from(u32::try_from(metrics.expected)?);
    let scoped_expected = f64::from(u32::try_from(metrics.scoped_expected)?);
    let judgment_count_float = f64::from(u32::try_from(judgment_count)?);
    let required_branches = f64::from(u32::try_from(metrics.required_branches_expected)?);
    let recall_at_5 = f64::from(u32::try_from(metrics.found_at_5)?) / expected;
    let recall_at_10 = f64::from(u32::try_from(metrics.found_at_10)?) / expected;
    let scoped_recall = f64::from(u32::try_from(metrics.scoped_found_at_10)?) / scoped_expected;
    let mean_reciprocal_rank = metrics.reciprocal_rank_sum / judgment_count_float;
    let branch_presence =
        f64::from(u32::try_from(metrics.required_branches_found)?) / required_branches;
    assert!(
        mean_reciprocal_rank >= 0.75,
        "MRR fell below 0.75: {mean_reciprocal_rank:.3}"
    );
    eprintln!(
        "relevance metrics: Recall@5={recall_at_5:.3}, Recall@10={recall_at_10:.3}, \
         MRR={mean_reciprocal_rank:.3}, preferred primaries={}/{}, \
         scoped Recall@10={scoped_recall:.3}, required branch presence={branch_presence:.3}",
        metrics.preferred_primary_found, judgment_count
    );
    Ok(())
}

#[test]
fn checked_in_relevance_gate_passes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let library = directory.path().join("relevance.db");
    seed(&library)?;
    let judgments = judgments()?;
    assert_documented_classes(&judgments);
    let mut metrics = Metrics::default();
    for judgment in &judgments {
        evaluate_judgment(&library, judgment, &mut metrics)?;
    }
    assert_and_report_metrics(&metrics, judgments.len())
}
