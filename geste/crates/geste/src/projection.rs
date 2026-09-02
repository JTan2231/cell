use std::collections::HashSet;
use std::fmt::Write as _;

use crate::model::{
    Graph, GraphEdge, GraphNode, GraphSource, Report, RevisionView, SettlementStatus,
};

const SOURCE_BOUNDARY: &str = "Source anchors are manual assertions. Geste v0.1 validates their shape but does not resolve upstream existence, mutable state, or interpretation.";
const INTERPRETATION_LABEL: &str = "Except for explicitly structured settlements, episode prose is Geste-authored interpretation rather than upstream authority.";

#[must_use]
pub fn report(view: RevisionView) -> Report {
    let warnings = coverage_warnings(&view);
    Report {
        kind: "episode_report",
        episode: view,
        interpretation_label: INTERPRETATION_LABEL,
        source_boundary: SOURCE_BOUNDARY,
        warnings,
    }
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn render_report_markdown(report: &Report) -> String {
    let view = &report.episode;
    let capture = &view.capture;
    let mut output = String::new();
    let _ = writeln!(
        output,
        "# {} — revision {}",
        safe(&capture.title),
        view.revision
    );
    let _ = writeln!(output);
    let _ = writeln!(output, "- Episode: {}", safe(&view.episode));
    let _ = writeln!(output, "- Recorded at: {}", safe(&view.recorded_at));
    let _ = writeln!(output, "- Basis cutoff: {}", safe(&capture.basis_cutoff_at));
    let _ = writeln!(output, "- Recorded by: {}", safe(&capture.recorded_by));
    let _ = writeln!(
        output,
        "- Submitted SHA-256: {}",
        safe(&view.submitted_sha256)
    );
    let _ = writeln!(output, "- Outcome: {}", capture.outcome.status.as_str());
    let _ = writeln!(output);
    let _ = writeln!(output, "> {}", report.interpretation_label);
    let _ = writeln!(output, "> {}", report.source_boundary);

    section(&mut output, "Shape", &capture.shape);
    section(&mut output, "Situation", &capture.situation);
    section(&mut output, "Response", &capture.response);
    section(&mut output, "Outcome", &capture.outcome.summary);
    section(&mut output, "Applicability", &capture.applicability);
    list_section(&mut output, "Actions", &capture.actions);
    list_section(&mut output, "Lessons", &capture.lessons);

    let _ = writeln!(output, "## Settlements");
    let _ = writeln!(output);
    if capture.settlements.is_empty() {
        let _ = writeln!(output, "None.");
    } else {
        for settlement in &capture.settlements {
            match settlement.status {
                SettlementStatus::Verified => {
                    let _ = writeln!(
                        output,
                        "- {} [verified; cited Decisions lifecycle authority]: {}",
                        safe(&settlement.id),
                        safe(&settlement.statement)
                    );
                }
                SettlementStatus::Unverified => {
                    let _ = writeln!(
                        output,
                        "- {} [unverified; not enacted; {}]: {}",
                        safe(&settlement.id),
                        safe(settlement.gap.as_deref().unwrap_or("missing gap")),
                        safe(&settlement.statement)
                    );
                }
            }
        }
    }
    let _ = writeln!(output);

    list_section(&mut output, "Coverage gaps", &capture.gaps);
    list_section(&mut output, "Tags", &capture.tags);

    let _ = writeln!(output, "## Source basis");
    let _ = writeln!(output);
    for source in &capture.sources {
        let frozen = match (&source.revision, &source.digest) {
            (Some(revision), Some(digest)) => {
                format!("revision {}; digest {}", safe(revision), safe(digest))
            }
            (Some(revision), None) => format!("revision {}", safe(revision)),
            (None, Some(digest)) => format!("digest {}", safe(digest)),
            (None, None) => "locator-only".to_owned(),
        };
        let _ = writeln!(
            output,
            "- {} — {}/{} {}; role {}; observed {}; {}. {}",
            safe(&source.id),
            safe(&source.system),
            safe(&source.kind),
            safe(&source.reference),
            source.role.as_str(),
            safe(&source.observed_at),
            frozen,
            safe(&source.label)
        );
        if !source.supports.is_empty() {
            let _ = writeln!(output, "  Supports: {}", source.supports.join(", "));
        }
    }
    let _ = writeln!(output);

    let _ = writeln!(output, "## Related episodes");
    let _ = writeln!(output);
    if capture.related_episodes.is_empty() {
        let _ = writeln!(output, "None.");
    } else {
        for link in &capture.related_episodes {
            let _ = writeln!(
                output,
                "- {} revision {} — {}",
                safe(&link.episode),
                link.revision,
                link.relation.as_str()
            );
        }
    }
    if !report.warnings.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "## Coverage warnings");
        let _ = writeln!(output);
        for warning in &report.warnings {
            let _ = writeln!(output, "- {}", safe(warning));
        }
    }
    output.trim_end().to_owned()
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn graph(view: &RevisionView) -> Graph {
    let root = format!("episode:{}@{}", view.episode, view.revision);
    let capture = &view.capture;
    let mut nodes = vec![GraphNode {
        id: root.clone(),
        kind: "episode",
        origin: "geste_episode",
        label: capture.title.clone(),
        status: Some(capture.outcome.status.as_str().to_owned()),
        source: None,
    }];
    let mut edges = Vec::new();

    for (target, label, value) in [
        ("shape", "shape", capture.shape.as_str()),
        ("situation", "situation", capture.situation.as_str()),
        ("response", "response", capture.response.as_str()),
        ("outcome", "outcome", capture.outcome.summary.as_str()),
        (
            "applicability",
            "applicability",
            capture.applicability.as_str(),
        ),
    ] {
        add_claim(&mut nodes, &mut edges, &root, target, label, value);
    }
    for (index, action) in capture.actions.iter().enumerate() {
        let target = format!("action:{}", index + 1);
        add_claim(&mut nodes, &mut edges, &root, &target, "action", action);
    }
    for (index, lesson) in capture.lessons.iter().enumerate() {
        let target = format!("lesson:{}", index + 1);
        add_claim(&mut nodes, &mut edges, &root, &target, "lesson", lesson);
    }
    for settlement in &capture.settlements {
        let target = format!("settlement:{}", settlement.id);
        let origin = match settlement.status {
            SettlementStatus::Verified => "geste_structured_verified_settlement",
            SettlementStatus::Unverified => "geste_authored_unverified_settlement",
        };
        let claim_id = claim_node_id(&target);
        nodes.push(GraphNode {
            id: claim_id.clone(),
            kind: "settlement",
            origin,
            label: settlement.statement.clone(),
            status: Some(settlement.status.as_str().to_owned()),
            source: None,
        });
        edges.push(GraphEdge {
            from: root.clone(),
            to: claim_id.clone(),
            kind: "structural",
            label: Some("settlement".to_owned()),
        });
        if settlement.status == SettlementStatus::Unverified
            && let Some(gap) = settlement.gap.as_deref()
        {
            edges.push(GraphEdge {
                from: claim_id,
                to: claim_node_id(gap),
                kind: "structural",
                label: Some("unverified_gap".to_owned()),
            });
        }
    }
    for (index, gap) in capture.gaps.iter().enumerate() {
        let target = format!("gap:{}", index + 1);
        add_claim(&mut nodes, &mut edges, &root, &target, "gap", gap);
    }

    for source in &capture.sources {
        let source_id = format!("source:{}", source.id);
        nodes.push(GraphNode {
            id: source_id.clone(),
            kind: "source",
            origin: "manual_upstream_anchor",
            label: format!(
                "{}/{} {} — {}",
                source.system, source.kind, source.reference, source.label
            ),
            status: Some(source.role.as_str().to_owned()),
            source: Some(GraphSource {
                system: source.system.clone(),
                kind: source.kind.clone(),
                reference: source.reference.clone(),
                revision: source.revision.clone(),
                digest: source.digest.clone(),
                observed_at: source.observed_at.clone(),
                role: source.role,
            }),
        });
        edges.push(GraphEdge {
            from: root.clone(),
            to: source_id.clone(),
            kind: "structural",
            label: Some("source_basis".to_owned()),
        });
        for target in &source.supports {
            edges.push(GraphEdge {
                from: source_id.clone(),
                to: claim_node_id(target),
                kind: "support",
                label: Some(source.role.as_str().to_owned()),
            });
        }
    }

    let mut related_nodes = HashSet::new();
    for link in &capture.related_episodes {
        let related_id = format!("related:{}@{}", link.episode, link.revision);
        if related_nodes.insert(related_id.clone()) {
            nodes.push(GraphNode {
                id: related_id.clone(),
                kind: "related_episode",
                origin: "geste_frozen_reference",
                label: format!("{} revision {}", link.episode, link.revision),
                status: None,
                source: None,
            });
        }
        edges.push(GraphEdge {
            from: root.clone(),
            to: related_id,
            kind: "episode_relation",
            label: Some(link.relation.as_str().to_owned()),
        });
    }

    Graph {
        kind: "episode_graph",
        episode: view.episode.clone(),
        revision: view.revision,
        interpretation_label: INTERPRETATION_LABEL,
        source_boundary: SOURCE_BOUNDARY,
        nodes,
        edges,
        warnings: coverage_warnings(view),
    }
}

#[must_use]
pub fn render_graph_human(graph: &Graph) -> String {
    let mut output = format!("{} revision {}\n", safe(&graph.episode), graph.revision);
    let _ = writeln!(
        output,
        "Interpretation: {}\nSource boundary: {}",
        graph.interpretation_label, graph.source_boundary
    );
    output.push_str("Nodes:\n");
    for node in &graph.nodes {
        let _ = writeln!(
            output,
            "  {} [{}; {}] {}",
            safe(&node.id),
            node.kind,
            node.origin,
            safe(&node.label)
        );
        if let Some(source) = &node.source {
            let revision = source
                .revision
                .as_deref()
                .map_or_else(|| "null".to_owned(), safe);
            let digest = source
                .digest
                .as_deref()
                .map_or_else(|| "null".to_owned(), safe);
            let _ = writeln!(
                output,
                "    source={}/{} reference={} revision={} digest={} observed_at={} role={}",
                safe(&source.system),
                safe(&source.kind),
                safe(&source.reference),
                revision,
                digest,
                safe(&source.observed_at),
                source.role.as_str()
            );
        }
    }
    output.push_str("Edges:\n");
    for edge in &graph.edges {
        let label = edge
            .label
            .as_deref()
            .map_or_else(String::new, |value| format!(" ({})", safe(value)));
        let _ = writeln!(
            output,
            "  {} -{}{}-> {}",
            safe(&edge.from),
            edge.kind,
            label,
            safe(&edge.to)
        );
    }
    if !graph.warnings.is_empty() {
        output.push_str("Warnings:\n");
        for warning in &graph.warnings {
            let _ = writeln!(output, "  {}", safe(warning));
        }
    }
    output.trim_end().to_owned()
}

fn add_claim(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    root: &str,
    target: &str,
    label: &str,
    value: &str,
) {
    let id = claim_node_id(target);
    nodes.push(GraphNode {
        id: id.clone(),
        kind: "claim",
        origin: "geste_authored_interpretation",
        label: value.to_owned(),
        status: None,
        source: None,
    });
    edges.push(GraphEdge {
        from: root.to_owned(),
        to: id,
        kind: "structural",
        label: Some(label.to_owned()),
    });
}

fn claim_node_id(target: &str) -> String {
    format!("claim:{target}")
}

fn coverage_warnings(view: &RevisionView) -> Vec<String> {
    view.capture
        .sources
        .iter()
        .filter(|source| source.revision.is_none() && source.digest.is_none())
        .map(|source| {
            format!(
                "Source {} ({}/{}/{}) is locator-only and cannot verify mutable upstream state.",
                source.id, source.system, source.kind, source.reference
            )
        })
        .collect()
}

fn section(output: &mut String, title: &str, value: &str) {
    let _ = writeln!(output);
    let _ = writeln!(output, "## {title}");
    let _ = writeln!(output);
    let _ = writeln!(output, "{}", safe(value));
}

fn list_section(output: &mut String, title: &str, values: &[String]) {
    let _ = writeln!(output);
    let _ = writeln!(output, "## {title}");
    let _ = writeln!(output);
    if values.is_empty() {
        let _ = writeln!(output, "None.");
    } else {
        for value in values {
            let _ = writeln!(output, "- {}", safe(value));
        }
    }
}

fn safe(value: &str) -> String {
    let mut rendered = String::with_capacity(value.len());
    for character in value.chars() {
        if character == '\n' {
            rendered.push(character);
        } else if character.is_control() {
            let _ = write!(rendered, "\\u{{{:x}}}", u32::from(character));
        } else {
            rendered.push(character);
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use crate::model::{
        Capture, Outcome, OutcomeStatus, RevisionView, Settlement, SettlementStatus, SourceAnchor,
        SourceRole,
    };

    use super::{graph, render_report_markdown, report};

    #[test]
    fn report_and_graph_keep_authorship_and_locator_warning_visible() {
        let view = RevisionView {
            episode: "e1".to_owned(),
            revision: 1,
            submitted_sha256: "a".repeat(64),
            recorded_at: "2026-09-02T00:00:00Z".to_owned(),
            capture: Capture {
                schema_version: 1,
                title: "Case".to_owned(),
                shape: "Shape".to_owned(),
                basis_cutoff_at: "2026-09-02T00:00:00Z".to_owned(),
                recorded_by: "codex".to_owned(),
                situation: "Situation".to_owned(),
                response: "Response".to_owned(),
                outcome: Outcome {
                    status: OutcomeStatus::Solved,
                    summary: "Solved".to_owned(),
                },
                applicability: "When useful".to_owned(),
                actions: vec![],
                lessons: vec![],
                settlements: vec![Settlement {
                    id: "choice".to_owned(),
                    statement: "A choice".to_owned(),
                    status: SettlementStatus::Unverified,
                    gap: Some("gap:1".to_owned()),
                }],
                tags: vec![],
                gaps: vec!["Decision unavailable".to_owned()],
                sources: vec![SourceAnchor {
                    id: "thread".to_owned(),
                    system: "conversations".to_owned(),
                    kind: "thread".to_owned(),
                    reference: "t1".to_owned(),
                    revision: None,
                    digest: None,
                    observed_at: "2026-09-02T00:00:00Z".to_owned(),
                    role: SourceRole::Context,
                    label: "Request".to_owned(),
                    supports: vec!["shape".to_owned()],
                }],
                related_episodes: vec![],
            },
        };
        let report = report(view.clone());
        let markdown = render_report_markdown(&report);
        assert!(markdown.contains("Geste-authored interpretation"));
        assert!(markdown.contains("unverified; not enacted; gap:1"));
        assert!(markdown.contains("locator-only"));
        let graph = graph(&view);
        assert!(graph.nodes.iter().any(|node| {
            node.id == "claim:settlement:choice"
                && node.origin == "geste_authored_unverified_settlement"
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.from == "source:thread" && edge.to == "claim:shape" && edge.kind == "support"
        }));
    }
}
