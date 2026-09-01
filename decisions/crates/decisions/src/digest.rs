use std::fmt::Write as _;

use crate::model::StoredCandidate;

pub(crate) fn render(
    report_date: &str,
    coverage_cutoff_at: Option<i64>,
    candidates: &[StoredCandidate],
) -> (String, String) {
    let decisions = candidates
        .iter()
        .filter(|candidate| {
            candidate.review_state != "dismissed"
                && (candidate.confidence == "high" || candidate.review_state == "confirmed")
        })
        .collect::<Vec<_>>();
    let possible = candidates
        .iter()
        .filter(|candidate| {
            candidate.confidence == "medium" && candidate.review_state == "unreviewed"
        })
        .collect::<Vec<_>>();

    let subject = if decisions.is_empty() && possible.is_empty() {
        format!("Codex decisions for {report_date}: all clear")
    } else {
        format!(
            "Codex decisions for {report_date}: {} {}, {} possible",
            decisions.len(),
            if decisions.len() == 1 {
                "decision"
            } else {
                "decisions"
            },
            possible.len()
        )
    };

    let mut body = format!("Codex decisions — {report_date}\n\n");
    if let Some(cutoff) = coverage_cutoff_at {
        let _ = writeln!(
            body,
            "Coverage includes eligible root-task turns completed by Unix second {cutoff}."
        );
        body.push('\n');
    }
    if decisions.is_empty() && possible.is_empty() {
        body.push_str(
            "All clear. Completed enacted-decision coverage found no attributable operative decisions.\n",
        );
        return (subject, body);
    }

    if !decisions.is_empty() {
        body.push_str("Decisions\n");
        for candidate in decisions {
            let _ = writeln!(
                body,
                "- {} [{}; {}]",
                candidate.statement, candidate.disposition, candidate.id
            );
        }
        body.push('\n');
    }

    if !possible.is_empty() {
        body.push_str("Possible decisions to review\n");
        for candidate in possible {
            let _ = writeln!(
                body,
                "- {} [{}; {}]",
                candidate.statement, candidate.disposition, candidate.id
            );
        }
        body.push_str(
            "\nReview with `decisions review confirm DECISION_ID` or `decisions review dismiss DECISION_ID`.\n",
        );
    }
    body
        .push_str("\nA decision is an attributable transition from practical openness to operative settlement.\n");
    (subject, body)
}

#[cfg(test)]
mod tests {
    use crate::model::StoredCandidate;

    use super::render;

    fn candidate(confidence: &str, review_state: &str) -> StoredCandidate {
        StoredCandidate {
            id: "d_1".to_owned(),
            run_id: "run_1".to_owned(),
            decided_at: 1,
            timestamp_precision: "item".to_owned(),
            statement: "Use the simple design.".to_owned(),
            disposition: "adopt".to_owned(),
            confidence: confidence.to_owned(),
            rationale: None,
            supersedes_id: None,
            authority_start: 0,
            authority_end: 6,
            review_state: review_state.to_owned(),
            sources: Vec::new(),
        }
    }

    #[test]
    fn all_clear_is_explicit() {
        let (subject, body) = render("2026-08-31", Some(10), &[]);
        assert!(subject.ends_with("all clear"));
        assert!(body.contains("Completed enacted-decision coverage"));
        assert!(body.contains("Unix second 10"));
    }

    #[test]
    fn separates_high_and_medium_candidates() {
        let (subject, body) = render(
            "2026-08-31",
            Some(10),
            &[
                candidate("high", "unreviewed"),
                candidate("medium", "unreviewed"),
            ],
        );
        assert!(subject.contains("1 decision, 1 possible"));
        assert!(body.contains("Decisions\n"));
        assert!(body.contains("Possible decisions to review\n"));
    }
}
