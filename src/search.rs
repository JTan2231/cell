use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;

use rusqlite::{Connection, params};

use crate::error::AppError;
use crate::index;
use crate::model::{
    BodyRange, BreadcrumbItem, Detail, MatchReason, NodeKind, RelatedHit, ResultExplanation,
    SearchExplanation, SearchKind, SearchOutput, SearchResult,
};
use crate::render::{HIGHLIGHT_END, HIGHLIGHT_START};
use crate::tree;

const MAX_QUERY_BYTES: usize = 4096;
const MAX_QUERY_TERMS: usize = 32;
const MAX_LIMIT: usize = 100;
const MIN_CANDIDATES: usize = 50;
const MAX_CANDIDATES: usize = 500;
const SUPPORT_DECAY: f64 = 0.7;
const SUPPORT_WEIGHT: f64 = 0.35;
const DETAIL_CLOSE_RATIO: f64 = 0.15;
const BRANCH_QUOTA: usize = 2;

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub within: Option<i64>,
    pub kind: SearchKind,
    pub detail: Detail,
    pub limit: usize,
    pub explain: bool,
}

#[derive(Clone, Debug)]
struct QueryTerm {
    text: String,
    phrase: bool,
}

#[derive(Clone, Debug)]
struct ParsedQuery {
    terms: Vec<QueryTerm>,
    exact_key: String,
    exact_id: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RetrievalPass {
    And,
    Or,
    Prefix,
}

impl RetrievalPass {
    const fn weight(self) -> f64 {
        match self {
            Self::And => 1.0,
            Self::Or => 0.65,
            Self::Prefix => 0.45,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or_fallback",
            Self::Prefix => "prefix_fallback",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum DiversityDecision {
    ExactExempt,
    WithinQuota,
    Backfill,
}

impl DiversityDecision {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ExactExempt => "exact_match_exempt",
            Self::WithinQuota => "within_branch_quota",
            Self::Backfill => "branch_quota_backfill",
        }
    }
}

#[derive(Clone, Debug)]
struct UnitHit {
    unit_id: i64,
    unit_no: i64,
    snippet: Option<String>,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
    raw_bm25: f64,
    lexical_rank: usize,
    score: f64,
    pass: RetrievalPass,
    body_match: bool,
}

#[derive(Clone, Debug)]
struct Candidate {
    node_id: i64,
    kind: NodeKind,
    title: String,
    normalized_path: String,
    breadcrumb: Vec<BreadcrumbItem>,
    hits: Vec<UnitHit>,
    exact_class: u8,
    exact_reason: Option<MatchReason>,
    direct_score: f64,
    support_score: f64,
    support_only: bool,
    support_source: Option<i64>,
    match_reasons: Vec<MatchReason>,
    related: Vec<Candidate>,
    diversity_decision: Option<DiversityDecision>,
}

impl Candidate {
    fn new(node_id: i64, kind: NodeKind, title: String, normalized_path: String) -> Self {
        Self {
            node_id,
            kind,
            title,
            normalized_path,
            breadcrumb: Vec::new(),
            hits: Vec::new(),
            exact_class: 0,
            exact_reason: None,
            direct_score: 0.0,
            support_score: 0.0,
            support_only: false,
            support_source: None,
            match_reasons: Vec::new(),
            related: Vec::new(),
            diversity_decision: None,
        }
    }

    fn final_score(&self) -> f64 {
        self.direct_score + self.support_score.mul_add(SUPPORT_WEIGHT, 0.0)
    }

    fn best_hit(&self) -> Option<&UnitHit> {
        self.hits.first()
    }
}

#[derive(Debug)]
struct RawUnitHit {
    unit_id: i64,
    node_id: i64,
    unit_no: i64,
    kind: String,
    title: String,
    normalized_path: String,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
    raw_bm25: f64,
    snippet: String,
    highlighted_title: String,
}

impl RawUnitHit {
    fn body_match(&self) -> bool {
        self.snippet.contains(HIGHLIGHT_START)
    }

    fn title_match(&self) -> bool {
        self.highlighted_title.contains(HIGHLIGHT_START)
    }

    fn own_text_matched(&self) -> bool {
        self.body_match() || self.title_match()
    }
}

/// Search the current derived index and return grouped node-level results.
pub fn search(
    connection: &Connection,
    query: &str,
    options: Options,
) -> Result<SearchOutput, AppError> {
    validate_options(options)?;
    let snapshot = connection.unchecked_transaction()?;
    let output = search_snapshot(&snapshot, query, options)?;
    snapshot.commit()?;
    Ok(output)
}

fn search_snapshot(
    connection: &Connection,
    query: &str,
    options: Options,
) -> Result<SearchOutput, AppError> {
    index::require_current(connection)?;
    if let Some(scope) = options.within {
        tree::get_node(connection, scope)?;
    }
    let parsed = parse_query(query)?;
    let candidate_limit = options
        .limit
        .saturating_mul(10)
        .clamp(MIN_CANDIDATES, MAX_CANDIDATES);
    let fallback_target = options
        .limit
        .saturating_mul(2)
        .min(candidate_limit)
        .max(options.limit);

    let mut candidates = exact_candidates(connection, &parsed, options)?;
    let exact_candidates = candidates.len();
    let and_query = fts_expression(&parsed.terms, " AND ");
    collect_fts(
        connection,
        &and_query,
        RetrievalPass::And,
        candidate_limit,
        options,
        &mut candidates,
    )?;
    let after_and_candidates = candidates.len();

    let or_fallback_used = candidates.len() < fallback_target && parsed.terms.len() > 1;
    if or_fallback_used {
        let or_query = fts_expression(&parsed.terms, " OR ");
        collect_fts(
            connection,
            &or_query,
            RetrievalPass::Or,
            candidate_limit,
            options,
            &mut candidates,
        )?;
    }
    let after_or_candidates = candidates.len();

    let prefix_query = (candidates.len() < fallback_target)
        .then(|| prefix_expression(&parsed.terms))
        .flatten();
    let prefix_fallback_used = prefix_query.is_some();
    if let Some(prefix_query) = prefix_query {
        collect_fts(
            connection,
            &prefix_query,
            RetrievalPass::Prefix,
            candidate_limit,
            options,
            &mut candidates,
        )?;
    }
    let after_prefix_candidates = candidates.len();

    prepare_candidates(connection, &parsed, &mut candidates)?;
    propagate_support(connection, options, &mut candidates)?;
    let mut grouped = collapse_chains(candidates.into_values().collect(), options.detail);
    sort_global_candidates(&mut grouped);
    let groups_after_collapse = grouped.len();
    let diverse = diversify(grouped, options.within, options.limit);
    let returned_results = diverse.len();
    let results = diverse
        .into_iter()
        .enumerate()
        .map(|(rank, candidate)| to_result(candidate, rank + 1, options.within, options.explain))
        .collect();
    Ok(SearchOutput {
        query: query.to_owned(),
        results,
        explanation: options.explain.then_some(SearchExplanation {
            exact_candidates,
            after_and_candidates,
            or_fallback_used,
            after_or_candidates,
            prefix_fallback_used,
            after_prefix_candidates,
            groups_after_collapse,
            returned_results,
        }),
    })
}

fn validate_options(options: Options) -> Result<(), AppError> {
    if options.limit == 0 || options.limit > MAX_LIMIT {
        return Err(AppError::invalid(
            "invalid_limit",
            format!("--limit must be between 1 and {MAX_LIMIT}"),
        ));
    }
    Ok(())
}

fn parse_query(query: &str) -> Result<ParsedQuery, AppError> {
    if query.len() > MAX_QUERY_BYTES {
        return Err(AppError::invalid(
            "query_too_long",
            format!("search queries are limited to {MAX_QUERY_BYTES} UTF-8 bytes"),
        ));
    }
    let mut terms = Vec::new();
    let mut buffer = String::new();
    let mut quoted = false;
    for character in query.chars() {
        match character {
            '"' => {
                if quoted {
                    push_term(&mut terms, &mut buffer, true);
                    quoted = false;
                } else {
                    push_term(&mut terms, &mut buffer, false);
                    quoted = true;
                }
            }
            whitespace if whitespace.is_whitespace() && !quoted => {
                push_term(&mut terms, &mut buffer, false);
            }
            other => buffer.push(other),
        }
    }
    if quoted {
        return Err(AppError::invalid(
            "unmatched_quote",
            "search query contains an unmatched double quote",
        ));
    }
    push_term(&mut terms, &mut buffer, false);
    terms.retain(|term| term.text.chars().any(char::is_alphanumeric));
    if terms.is_empty() {
        return Err(AppError::invalid(
            "empty_search_query",
            "search query must contain a letter or number",
        ));
    }
    let token_count = terms
        .iter()
        .map(|term| approximate_token_count(&term.text))
        .sum::<usize>();
    if token_count > MAX_QUERY_TERMS {
        return Err(AppError::invalid(
            "too_many_query_terms",
            format!("search queries are limited to {MAX_QUERY_TERMS} terms and phrases"),
        ));
    }
    let exact_text = query.replace('"', "");
    let exact_key = index::normalize_key(&exact_text);
    let exact_id = query
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|identifier| *identifier > 0);
    Ok(ParsedQuery {
        terms,
        exact_key,
        exact_id,
    })
}

fn approximate_token_count(text: &str) -> usize {
    let mut count = 0_usize;
    let mut in_token = false;
    for character in text.chars() {
        if character.is_alphanumeric() {
            if !in_token {
                count = count.saturating_add(1);
                in_token = true;
            }
        } else {
            in_token = false;
        }
    }
    count
}

fn push_term(terms: &mut Vec<QueryTerm>, buffer: &mut String, phrase: bool) {
    let text = buffer.trim();
    if !text.is_empty() {
        terms.push(QueryTerm {
            text: text.to_owned(),
            phrase,
        });
    }
    buffer.clear();
}

fn quoted_fts(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

fn fts_expression(terms: &[QueryTerm], separator: &str) -> String {
    terms
        .iter()
        .map(|term| quoted_fts(&term.text))
        .collect::<Vec<_>>()
        .join(separator)
}

fn prefix_expression(terms: &[QueryTerm]) -> Option<String> {
    let (last, preceding) = terms.split_last()?;
    if last.phrase || last.text.chars().count() < 3 || !last.text.chars().all(char::is_alphanumeric)
    {
        return None;
    }
    let prefix = format!("title : {}*", quoted_fts(&last.text));
    if preceding.is_empty() {
        Some(prefix)
    } else {
        Some(format!(
            "{} AND {prefix}",
            fts_expression(preceding, " AND ")
        ))
    }
}

fn exact_candidates(
    connection: &Connection,
    parsed: &ParsedQuery,
    options: Options,
) -> Result<HashMap<i64, Candidate>, AppError> {
    let exact_id = parsed.exact_id.unwrap_or(-1);
    let kind = options.kind.node_kind().map(NodeKind::as_str);
    let mut candidates = HashMap::new();
    if let Some(scope) = options.within {
        let mut statement = connection.prepare(
            "WITH exact_nodes(id) AS (
                 SELECT node_id FROM search_units WHERE normalized_title = ?4
                 UNION
                 SELECT node_id FROM search_units WHERE normalized_path = ?4
                 UNION
                 SELECT ?3 WHERE ?3 > 0
             )
             SELECT n.id, n.kind, n.title, MIN(su.normalized_title),
                    MIN(su.normalized_path)
             FROM exact_nodes
             JOIN nodes AS n ON n.id = exact_nodes.id
             JOIN search_units AS su ON su.node_id = n.id
             WHERE (?2 IS NULL OR n.kind = ?2)
               AND EXISTS (
                   WITH RECURSIVE ancestors(id, parent_id) AS (
                       SELECT id, parent_id FROM nodes WHERE id = n.id
                       UNION ALL
                       SELECT parent.id, parent.parent_id
                       FROM nodes AS parent
                       JOIN ancestors AS child ON child.parent_id = parent.id
                   )
                   SELECT 1 FROM ancestors WHERE id = ?1
               )
             GROUP BY n.id, n.kind, n.title
             ORDER BY n.id",
        )?;
        let rows =
            statement.query_map(params![scope, kind, exact_id, parsed.exact_key], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
        for row in rows {
            let (node_id, node_kind, title, normalized_title, normalized_path) = row?;
            insert_exact_candidate(
                &mut candidates,
                parsed,
                exact_id,
                node_id,
                &node_kind,
                title,
                &normalized_title,
                normalized_path,
            )?;
        }
    } else {
        let mut statement = connection.prepare(
            "WITH exact_nodes(id) AS (
                 SELECT node_id FROM search_units WHERE normalized_title = ?3
                 UNION
                 SELECT node_id FROM search_units WHERE normalized_path = ?3
                 UNION
                 SELECT ?2 WHERE ?2 > 0
             )
             SELECT n.id, n.kind, n.title, MIN(su.normalized_title),
                    MIN(su.normalized_path)
             FROM exact_nodes
             JOIN nodes AS n ON n.id = exact_nodes.id
             JOIN search_units AS su ON su.node_id = n.id
             WHERE (?1 IS NULL OR n.kind = ?1)
             GROUP BY n.id, n.kind, n.title
             ORDER BY n.id",
        )?;
        let rows = statement.query_map(params![kind, exact_id, parsed.exact_key], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (node_id, node_kind, title, normalized_title, normalized_path) = row?;
            insert_exact_candidate(
                &mut candidates,
                parsed,
                exact_id,
                node_id,
                &node_kind,
                title,
                &normalized_title,
                normalized_path,
            )?;
        }
    }
    Ok(candidates)
}

#[allow(clippy::too_many_arguments)]
fn insert_exact_candidate(
    candidates: &mut HashMap<i64, Candidate>,
    parsed: &ParsedQuery,
    exact_id: i64,
    node_id: i64,
    node_kind: &str,
    title: String,
    normalized_title: &str,
    normalized_path: String,
) -> Result<(), AppError> {
    let kind = NodeKind::from_str(node_kind).map_err(|_| {
        AppError::database(
            "invalid_node_kind",
            format!("node {node_id} has invalid kind {node_kind:?}"),
        )
    })?;
    let (exact_class, exact_reason) = if node_id == exact_id {
        (3, MatchReason::ExactId)
    } else if normalized_title == parsed.exact_key {
        (2, MatchReason::ExactTitle)
    } else if normalized_path == parsed.exact_key {
        (3, MatchReason::ExactPath)
    } else {
        return Ok(());
    };
    let mut candidate = Candidate::new(node_id, kind, title, normalized_path);
    candidate.exact_class = exact_class;
    candidate.exact_reason = Some(exact_reason);
    candidate.direct_score = 1.0;
    candidate.match_reasons.push(exact_reason);
    candidates.insert(node_id, candidate);
    Ok(())
}

fn collect_fts(
    connection: &Connection,
    expression: &str,
    pass: RetrievalPass,
    candidate_limit: usize,
    options: Options,
    candidates: &mut HashMap<i64, Candidate>,
) -> Result<(), AppError> {
    let limit = i64::try_from(candidate_limit)
        .map_err(|_| AppError::invalid("invalid_limit", "candidate limit cannot be represented"))?;
    let kind = options.kind.node_kind().map(NodeKind::as_str);
    let marker_start = HIGHLIGHT_START.to_string();
    let marker_end = HIGHLIGHT_END.to_string();
    let hits = if let Some(scope) = options.within {
        let mut statement = connection.prepare(
            "SELECT su.id, su.node_id, su.unit_no, n.kind, su.title,
                    su.normalized_path, su.start_byte, su.end_byte,
                    bm25(search_fts, 8.0, 2.0, 1.0),
                    snippet(search_fts, 2, ?5, ?6, '…', 24),
                    highlight(search_fts, 0, ?5, ?6)
             FROM search_fts
             JOIN search_units AS su ON su.id = search_fts.rowid
             JOIN nodes AS n ON n.id = su.node_id
             WHERE search_fts MATCH ?2 AND (?3 IS NULL OR n.kind = ?3)
               AND EXISTS (
                   WITH RECURSIVE ancestors(id, parent_id) AS (
                       SELECT id, parent_id FROM nodes WHERE id = su.node_id
                       UNION ALL
                       SELECT parent.id, parent.parent_id
                       FROM nodes AS parent
                       JOIN ancestors AS child ON child.parent_id = parent.id
                   )
                   SELECT 1 FROM ancestors WHERE id = ?1
               )
             ORDER BY bm25(search_fts, 8.0, 2.0, 1.0), su.node_id,
                      su.unit_no, su.id
             LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![scope, expression, kind, limit, marker_start, marker_end],
            raw_unit_hit,
        )?;
        rows.collect::<Result<Vec<_>, _>>()?
    } else {
        let mut statement = connection.prepare(
            "SELECT su.id, su.node_id, su.unit_no, n.kind, su.title,
                    su.normalized_path, su.start_byte, su.end_byte,
                    bm25(search_fts, 8.0, 2.0, 1.0),
                    snippet(search_fts, 2, ?4, ?5, '…', 24),
                    highlight(search_fts, 0, ?4, ?5)
             FROM search_fts
             JOIN search_units AS su ON su.id = search_fts.rowid
             JOIN nodes AS n ON n.id = su.node_id
             WHERE search_fts MATCH ?1 AND (?2 IS NULL OR n.kind = ?2)
             ORDER BY bm25(search_fts, 8.0, 2.0, 1.0), su.node_id,
                      su.unit_no, su.id
             LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![expression, kind, limit, marker_start, marker_end],
            raw_unit_hit,
        )?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let mut node_ranks = HashMap::new();
    for raw in hits {
        if !raw.own_text_matched() {
            continue;
        }
        let next_rank = node_ranks.len();
        let lexical_rank = *node_ranks.entry(raw.node_id).or_insert(next_rank);
        merge_unit_hit(candidates, raw, pass, lexical_rank)?;
    }
    Ok(())
}

fn raw_unit_hit(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawUnitHit> {
    Ok(RawUnitHit {
        unit_id: row.get(0)?,
        node_id: row.get(1)?,
        unit_no: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        normalized_path: row.get(5)?,
        start_byte: row.get(6)?,
        end_byte: row.get(7)?,
        raw_bm25: row.get(8)?,
        snippet: row.get(9)?,
        highlighted_title: row.get(10)?,
    })
}

fn merge_unit_hit(
    candidates: &mut HashMap<i64, Candidate>,
    raw: RawUnitHit,
    pass: RetrievalPass,
    lexical_rank: usize,
) -> Result<(), AppError> {
    let kind = NodeKind::from_str(&raw.kind).map_err(|_| {
        AppError::database(
            "invalid_node_kind",
            format!("node {} has invalid kind {:?}", raw.node_id, raw.kind),
        )
    })?;
    let lexical_rank_for_score = u32::try_from(lexical_rank)
        .map_err(|_| AppError::database("candidate_rank_overflow", "too many search candidates"))?;
    let rank = f64::from(lexical_rank_for_score) + 1.0;
    let score = pass.weight() / rank.mul_add(0.15, 1.0);
    let body_match = raw.body_match();
    let title_match = raw.title_match();
    if !(body_match || title_match) {
        return Ok(());
    }
    let snippet = body_match.then_some(raw.snippet);
    let hit = UnitHit {
        unit_id: raw.unit_id,
        unit_no: raw.unit_no,
        snippet,
        start_byte: raw.start_byte,
        end_byte: raw.end_byte,
        raw_bm25: raw.raw_bm25,
        lexical_rank,
        score,
        pass,
        body_match,
    };
    let candidate = candidates
        .entry(raw.node_id)
        .or_insert_with(|| Candidate::new(raw.node_id, kind, raw.title, raw.normalized_path));
    if let Some(existing) = candidate
        .hits
        .iter_mut()
        .find(|existing| existing.unit_id == hit.unit_id)
    {
        if hit.score > existing.score {
            *existing = hit;
        }
    } else {
        candidate.hits.push(hit);
    }
    Ok(())
}

fn prepare_candidates(
    connection: &Connection,
    parsed: &ParsedQuery,
    candidates: &mut HashMap<i64, Candidate>,
) -> Result<(), AppError> {
    for candidate in candidates.values_mut() {
        candidate.hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.raw_bm25.total_cmp(&right.raw_bm25))
                .then_with(|| left.unit_no.cmp(&right.unit_no))
                .then_with(|| left.unit_id.cmp(&right.unit_id))
        });
        let lexical = rolled_up_score(&candidate.hits);
        if candidate.exact_class > 0 {
            candidate.direct_score = lexical.mul_add(0.2, 1.0);
        } else {
            candidate.direct_score = lexical;
        }
        if !candidate.hits.is_empty() {
            push_reason(&mut candidate.match_reasons, MatchReason::Lexical);
        }
        if candidate
            .hits
            .iter()
            .any(|hit| hit.pass == RetrievalPass::Prefix)
        {
            push_reason(&mut candidate.match_reasons, MatchReason::Prefix);
        }
        if parsed.terms.iter().any(|term| term.phrase)
            && candidate
                .hits
                .iter()
                .any(|hit| hit.pass == RetrievalPass::And)
        {
            push_reason(&mut candidate.match_reasons, MatchReason::Phrase);
        }
        if let Some(reason) = candidate.exact_reason {
            push_reason(&mut candidate.match_reasons, reason);
        }
        candidate.breadcrumb = load_breadcrumb(connection, candidate.node_id)?;
        if candidate.normalized_path.is_empty() {
            candidate.normalized_path = index::normalize_key(
                &candidate
                    .breadcrumb
                    .iter()
                    .map(|item| item.title.as_str())
                    .collect::<Vec<_>>()
                    .join(index::BREADCRUMB_SEPARATOR),
            );
        }
    }
    Ok(())
}

fn rolled_up_score(hits: &[UnitHit]) -> f64 {
    let Some(best) = hits.first() else {
        return 0.0;
    };
    let second = hits
        .iter()
        .skip(1)
        .find(|hit| hit.body_match && (!best.body_match || !ranges_overlap(best, hit)))
        .map_or(0.0, |hit| (hit.score * 0.15).min(0.08));
    best.score + second
}

fn ranges_overlap(left: &UnitHit, right: &UnitHit) -> bool {
    match (
        left.start_byte,
        left.end_byte,
        right.start_byte,
        right.end_byte,
    ) {
        (Some(left_start), Some(left_end), Some(right_start), Some(right_end)) => {
            left_start < right_end && right_start < left_end
        }
        _ => left.unit_id == right.unit_id,
    }
}

fn push_reason(reasons: &mut Vec<MatchReason>, reason: MatchReason) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

fn load_breadcrumb(connection: &Connection, node_id: i64) -> Result<Vec<BreadcrumbItem>, AppError> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE ancestors(id, parent_id, title, distance) AS (
             SELECT id, parent_id, title, 0 FROM nodes WHERE id = ?1
             UNION ALL
             SELECT n.id, n.parent_id, n.title, ancestors.distance + 1
             FROM nodes AS n JOIN ancestors ON ancestors.parent_id = n.id
         )
         SELECT id, title FROM ancestors ORDER BY distance DESC",
    )?;
    let rows = statement.query_map([node_id], |row| {
        Ok(BreadcrumbItem {
            id: row.get(0)?,
            title: row.get(1)?,
        })
    })?;
    let breadcrumb = rows.collect::<Result<Vec<_>, _>>()?;
    if breadcrumb.is_empty() {
        return Err(AppError::not_found(
            "node_not_found",
            format!("node {node_id} was not found"),
        ));
    }
    Ok(breadcrumb)
}

fn propagate_support(
    connection: &Connection,
    options: Options,
    candidates: &mut HashMap<i64, Candidate>,
) -> Result<(), AppError> {
    let direct = candidates
        .values()
        .filter(|candidate| candidate.direct_score > 0.0)
        .map(|candidate| {
            (
                candidate.node_id,
                candidate.direct_score,
                candidate.breadcrumb.clone(),
            )
        })
        .collect::<Vec<_>>();

    for (descendant_id, descendant_score, breadcrumb) in direct {
        let scope_start = options
            .within
            .and_then(|scope| breadcrumb.iter().position(|item| item.id == scope))
            .unwrap_or(0);
        for (ancestor_index, item) in breadcrumb
            .iter()
            .enumerate()
            .skip(scope_start)
            .take(breadcrumb.len().saturating_sub(scope_start + 1))
        {
            let distance = breadcrumb.len().saturating_sub(ancestor_index + 1);
            let distance_i32 = i32::try_from(distance).unwrap_or(i32::MAX);
            let support = descendant_score * SUPPORT_DECAY.powi(distance_i32);
            let node = tree::get_node(connection, item.id)?;
            if options
                .kind
                .node_kind()
                .is_some_and(|kind| kind != node.kind)
            {
                continue;
            }
            let ancestor_breadcrumb = breadcrumb[..=ancestor_index].to_vec();
            let normalized_path = index::normalize_key(
                &ancestor_breadcrumb
                    .iter()
                    .map(|part| part.title.as_str())
                    .collect::<Vec<_>>()
                    .join(index::BREADCRUMB_SEPARATOR),
            );
            let candidate = candidates.entry(item.id).or_insert_with(|| {
                let mut candidate = Candidate::new(item.id, node.kind, node.title, normalized_path);
                candidate.breadcrumb = ancestor_breadcrumb;
                candidate.support_only = true;
                candidate
            });
            let support_order = support.total_cmp(&candidate.support_score);
            let deterministic_tie = support_order == Ordering::Equal
                && candidate
                    .support_source
                    .is_none_or(|source_id| descendant_id < source_id);
            if support_order == Ordering::Greater || deterministic_tie {
                candidate.support_score = support;
                candidate.support_source = Some(descendant_id);
            }
            push_reason(&mut candidate.match_reasons, MatchReason::DescendantSupport);
        }
    }
    Ok(())
}

fn sort_global_candidates(candidates: &mut [Candidate]) {
    candidates.sort_by(compare_global_candidates);
}

fn compare_global_candidates(left: &Candidate, right: &Candidate) -> Ordering {
    right
        .exact_class
        .cmp(&left.exact_class)
        .then_with(|| right.final_score().total_cmp(&left.final_score()))
        .then_with(|| right.direct_score.total_cmp(&left.direct_score))
        .then_with(|| deterministic_candidate_order(left, right))
}

fn compare_chain_candidates(left: &Candidate, right: &Candidate, detail: Detail) -> Ordering {
    right
        .exact_class
        .cmp(&left.exact_class)
        .then_with(|| compare_direct_relevance(left, right, detail))
        .then_with(|| right.final_score().total_cmp(&left.final_score()))
        .then_with(|| deterministic_candidate_order(left, right))
}

fn deterministic_candidate_order(left: &Candidate, right: &Candidate) -> Ordering {
    best_bm25(left)
        .total_cmp(&best_bm25(right))
        .then_with(|| left.normalized_path.cmp(&right.normalized_path))
        .then_with(|| left.node_id.cmp(&right.node_id))
}

fn compare_direct_relevance(left: &Candidate, right: &Candidate, detail: Detail) -> Ordering {
    let strongest = left.direct_score.max(right.direct_score);
    let close = (left.direct_score - right.direct_score).abs()
        <= strongest.mul_add(DETAIL_CLOSE_RATIO, f64::EPSILON);
    if close {
        let detail_order = detail_priority(right, detail).cmp(&detail_priority(left, detail));
        if detail_order != Ordering::Equal {
            return detail_order;
        }
    }
    right.direct_score.total_cmp(&left.direct_score)
}

const fn detail_priority(candidate: &Candidate, detail: Detail) -> u8 {
    match (detail, candidate.kind) {
        (Detail::Overview, NodeKind::Topic) | (Detail::Source, NodeKind::Source) => 1,
        _ => 0,
    }
}

fn best_bm25(candidate: &Candidate) -> f64 {
    candidate
        .best_hit()
        .map_or(f64::INFINITY, |hit| hit.raw_bm25)
}

fn collapse_chains(mut candidates: Vec<Candidate>, detail: Detail) -> Vec<Candidate> {
    candidates.sort_by(|left, right| compare_chain_candidates(left, right, detail));
    let mut primaries: Vec<Candidate> = Vec::new();
    for candidate in candidates {
        let related_primary = primaries
            .iter()
            .position(|primary| can_collapse(primary, &candidate));
        if let Some(index) = related_primary {
            if !candidate.support_only {
                primaries[index].related.push(candidate);
                primaries[index]
                    .related
                    .sort_by(|left, right| compare_chain_candidates(left, right, detail));
            }
        } else if !candidate.support_only || candidate.support_score > 0.0 {
            primaries.push(candidate);
        }
    }
    primaries
}

fn can_collapse(primary: &Candidate, candidate: &Candidate) -> bool {
    let primary_ids = breadcrumb_ids(primary);
    let candidate_ids = breadcrumb_ids(candidate);
    if is_prefix(&primary_ids, &candidate_ids) {
        let primary_depth = primary_ids.len();
        let candidate_branch = candidate_ids.get(primary_depth).copied();
        let existing_branch = primary
            .related
            .iter()
            .find_map(|related| breadcrumb_ids(related).get(primary_depth).copied());
        existing_branch.is_none() || existing_branch == candidate_branch
    } else {
        is_prefix(&candidate_ids, &primary_ids)
    }
}

fn breadcrumb_ids(candidate: &Candidate) -> Vec<i64> {
    candidate.breadcrumb.iter().map(|item| item.id).collect()
}

fn is_prefix(left: &[i64], right: &[i64]) -> bool {
    left.len() <= right.len() && left.iter().zip(right).all(|(left, right)| left == right)
}

fn diversify(candidates: Vec<Candidate>, scope: Option<i64>, limit: usize) -> Vec<Candidate> {
    let mut accepted = Vec::new();
    let mut skipped = Vec::new();
    let mut branch_counts: HashMap<i64, usize> = HashMap::new();
    for mut candidate in candidates {
        let key = branch_key(&candidate, scope);
        if candidate.exact_class > 0 {
            candidate.diversity_decision = Some(DiversityDecision::ExactExempt);
            accepted.push(candidate);
            continue;
        }
        let count = branch_counts.entry(key).or_default();
        if *count < BRANCH_QUOTA {
            *count += 1;
            candidate.diversity_decision = Some(DiversityDecision::WithinQuota);
            accepted.push(candidate);
        } else {
            skipped.push(candidate);
        }
    }
    if accepted.len() < limit {
        let remaining = limit - accepted.len();
        accepted.extend(skipped.into_iter().take(remaining).map(|mut candidate| {
            candidate.diversity_decision = Some(DiversityDecision::Backfill);
            candidate
        }));
    }
    accepted.truncate(limit);
    accepted
}

fn branch_key(candidate: &Candidate, scope: Option<i64>) -> i64 {
    match scope {
        Some(scope) => candidate
            .breadcrumb
            .iter()
            .position(|item| item.id == scope)
            .and_then(|index| candidate.breadcrumb.get(index + 1))
            .map_or(scope, |item| item.id),
        None => candidate
            .breadcrumb
            .first()
            .map_or(candidate.node_id, |item| item.id),
    }
}

fn to_result(candidate: Candidate, rank: usize, scope: Option<i64>, explain: bool) -> SearchResult {
    let (body_range, snippet) = hit_display(candidate.best_hit());
    let chain_group_node_id = candidate.node_id;
    let grouping_reason = if candidate.support_only {
        "supporting_ancestor"
    } else if candidate.related.is_empty() {
        "independent_result"
    } else {
        "chain_representative"
    };
    let explanation = explain.then(|| {
        explain_candidate(
            &candidate,
            chain_group_node_id,
            grouping_reason,
            branch_key(&candidate, scope),
            candidate
                .diversity_decision
                .map_or("not_applicable", DiversityDecision::as_str),
            Some(rank),
        )
    });
    let related_hits = candidate
        .related
        .into_iter()
        .map(|related| to_related_hit(related, chain_group_node_id, scope, explain))
        .collect();
    SearchResult {
        rank,
        node_id: candidate.node_id,
        kind: candidate.kind,
        title: candidate.title,
        breadcrumb: candidate.breadcrumb,
        body_range,
        snippet,
        match_reasons: candidate.match_reasons,
        related_hits,
        explanation,
    }
}

fn to_related_hit(
    candidate: Candidate,
    chain_group_node_id: i64,
    scope: Option<i64>,
    explain: bool,
) -> RelatedHit {
    let (body_range, snippet) = hit_display(candidate.best_hit());
    let explanation = explain.then(|| {
        explain_candidate(
            &candidate,
            chain_group_node_id,
            "ancestor_descendant_chain",
            branch_key(&candidate, scope),
            "related_hit",
            None,
        )
    });
    RelatedHit {
        node_id: candidate.node_id,
        kind: candidate.kind,
        title: candidate.title,
        breadcrumb: candidate.breadcrumb,
        body_range,
        snippet,
        match_reasons: candidate.match_reasons,
        explanation,
    }
}

fn explain_candidate(
    candidate: &Candidate,
    chain_group_node_id: i64,
    grouping_reason: &str,
    branch_key: i64,
    diversity_reason: &str,
    final_position: Option<usize>,
) -> ResultExplanation {
    let best_hit = candidate.best_hit();
    ResultExplanation {
        primary_unit_id: best_hit.map(|hit| hit.unit_id),
        raw_bm25: best_hit.map(|hit| hit.raw_bm25),
        lexical_rank: best_hit.map(|hit| hit.lexical_rank.saturating_add(1)),
        retrieval_pass: best_hit.map(|hit| hit.pass.as_str().to_owned()),
        exact_class: exact_class(candidate).to_owned(),
        direct_score: candidate.direct_score,
        support_score: candidate.support_score,
        support_source_node_id: candidate.support_source,
        chain_group_node_id,
        grouping_reason: grouping_reason.to_owned(),
        branch_key,
        diversity_reason: diversity_reason.to_owned(),
        final_position,
    }
}

const fn exact_class(candidate: &Candidate) -> &'static str {
    match candidate.exact_reason {
        Some(MatchReason::ExactId) => "identifier",
        Some(MatchReason::ExactPath) => "path",
        Some(MatchReason::ExactTitle) => "title",
        _ => "none",
    }
}

fn hit_display(hit: Option<&UnitHit>) -> (Option<BodyRange>, Option<String>) {
    let Some(hit) = hit else {
        return (None, None);
    };
    let body_range = if hit.body_match {
        hit.start_byte.zip(hit.end_byte).and_then(|(start, end)| {
            Some(BodyRange {
                start_byte: usize::try_from(start).ok()?,
                end_byte: usize::try_from(end).ok()?,
            })
        })
    } else {
        None
    };
    (body_range, hit.snippet.clone())
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use std::collections::BTreeSet;
    use std::error::Error;

    use rusqlite::{Connection, Transaction};

    use super::{
        Candidate, HIGHLIGHT_START, Options, RetrievalPass, UnitHit, compare_chain_candidates,
        diversify, fts_expression, parse_query, prefix_expression, quoted_fts, rolled_up_score,
        search, sort_global_candidates,
    };
    use crate::db;
    use crate::error::AppError;
    use crate::index;
    use crate::model::{
        BreadcrumbItem, Detail, MatchReason, NodeKind, SearchKind, SearchOutput, SearchResult,
    };
    use crate::tree::{self, NewNode, SourceFields};

    type TestResult<T = ()> = Result<T, Box<dyn Error>>;

    struct TestLibrary {
        _directory: tempfile::TempDir,
        connection: Connection,
    }

    impl TestLibrary {
        fn new() -> TestResult<Self> {
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("annals.db");
            let connection = db::init(&path)?;
            Ok(Self {
                _directory: directory,
                connection,
            })
        }

        fn populate<T>(
            &mut self,
            build: impl FnOnce(&Transaction<'_>) -> Result<T, AppError>,
        ) -> TestResult<T> {
            let transaction = self.connection.transaction()?;
            let value = build(&transaction)?;
            index::rebuild_all(&transaction)?;
            transaction.commit()?;
            Ok(value)
        }
    }

    fn options() -> Options {
        Options {
            within: None,
            kind: SearchKind::All,
            detail: Detail::Balanced,
            limit: 10,
            explain: false,
        }
    }

    fn child(kind: NodeKind, title: &str, body: &str) -> NewNode {
        NewNode {
            kind,
            title: title.to_owned(),
            body: body.to_owned(),
            position: None,
            source: SourceFields::default(),
        }
    }

    fn all_result_ids(output: &SearchOutput) -> Vec<i64> {
        output
            .results
            .iter()
            .flat_map(|result| {
                std::iter::once(result.node_id)
                    .chain(result.related_hits.iter().map(|related| related.node_id))
            })
            .collect()
    }

    fn first_result(output: &SearchOutput) -> Result<&SearchResult, std::io::Error> {
        output
            .results
            .first()
            .ok_or_else(|| std::io::Error::other("search returned no primary result"))
    }

    #[test]
    fn parses_and_escapes_terms_without_exposing_fts_operators() -> TestResult {
        let parsed = parse_query("alpha OR \"beta gamma\"")?;
        assert_eq!(
            fts_expression(&parsed.terms, " AND "),
            "\"alpha\" AND \"OR\" AND \"beta gamma\""
        );
        assert_eq!(quoted_fts("alpha\" OR beta"), "\"alpha\"\" OR beta\"");

        let prefix = parse_query("sqlite trans")?;
        assert_eq!(
            prefix_expression(&prefix.terms).as_deref(),
            Some("\"sqlite\" AND title : \"trans\"*")
        );

        assert!(parse_query(" --- ").is_err());
        assert!(parse_query("\"unfinished").is_err());
        let oversized_phrase = format!(
            "\"{}\"",
            std::iter::repeat_n("word", 33)
                .collect::<Vec<_>>()
                .join(" ")
        );
        let Err(error) = parse_query(&oversized_phrase) else {
            return Err(std::io::Error::other("oversized phrase was accepted").into());
        };
        assert_eq!(error.code(), "too_many_query_terms");
        let punctuation_split = std::iter::repeat_n("word", 33)
            .collect::<Vec<_>>()
            .join("-");
        let Err(error) = parse_query(&punctuation_split) else {
            return Err(std::io::Error::other("token-heavy identifier was accepted").into());
        };
        assert_eq!(error.code(), "too_many_query_terms");
        Ok(())
    }

    #[test]
    fn exact_identifier_title_and_path_are_distinct_and_deterministic() -> TestResult {
        let mut library = TestLibrary::new()?;
        let (first_root, first_duplicate, second_duplicate) = library.populate(|transaction| {
            let first_root = tree::create_root(transaction, "Databases", "overview")?;
            let first_duplicate = tree::add_node(
                transaction,
                first_root,
                &child(NodeKind::Topic, "Duplicate", "first copy"),
            )?;
            let second_root = tree::create_root(transaction, "Other", "other overview")?;
            let second_duplicate = tree::add_node(
                transaction,
                second_root,
                &child(NodeKind::Topic, "Duplicate", "second copy"),
            )?;
            Ok((first_root, first_duplicate, second_duplicate))
        })?;

        let title = search(&library.connection, "  ＤＵＰＬＩＣＡＴＥ  ", options())?;
        let title_ids = all_result_ids(&title).into_iter().collect::<BTreeSet<_>>();
        assert_eq!(
            title_ids,
            BTreeSet::from([first_duplicate, second_duplicate])
        );
        assert!(title.results.iter().all(|result| {
            result.match_reasons.contains(&MatchReason::ExactTitle)
                || result
                    .related_hits
                    .iter()
                    .all(|related| related.match_reasons.contains(&MatchReason::ExactTitle))
        }));

        let path = search(&library.connection, "databases / duplicate", options())?;
        let path_result = first_result(&path)?;
        assert_eq!(path_result.node_id, first_duplicate);
        assert!(path_result.match_reasons.contains(&MatchReason::ExactPath));

        let root_title = search(&library.connection, "Databases", options())?;
        let root_result = first_result(&root_title)?;
        assert_eq!(root_result.node_id, first_root);
        assert!(root_result.match_reasons.contains(&MatchReason::ExactTitle));
        assert_eq!(all_result_ids(&root_title), [first_root]);

        let identifier = search(&library.connection, &first_duplicate.to_string(), options())?;
        let identifier_result = first_result(&identifier)?;
        assert_eq!(identifier_result.node_id, first_duplicate);
        assert!(
            identifier_result
                .match_reasons
                .contains(&MatchReason::ExactId)
        );

        let repeated = search(&library.connection, "  ＤＵＰＬＩＣＡＴＥ  ", options())?;
        assert_eq!(all_result_ids(&title), all_result_ids(&repeated));
        Ok(())
    }

    #[test]
    fn breadcrumb_only_hits_do_not_flood_descendants_but_mixed_hits_work() -> TestResult {
        let mut library = TestLibrary::new()?;
        let (root, paper, unrelated) = library.populate(|transaction| {
            let root = tree::create_root(transaction, "Databases", "")?;
            let paper = tree::add_node(
                transaction,
                root,
                &child(NodeKind::Source, "Paper", "indexing internals"),
            )?;
            let unrelated = tree::add_node(
                transaction,
                root,
                &child(NodeKind::Source, "Unrelated", "different material"),
            )?;
            Ok((root, paper, unrelated))
        })?;

        let title = search(&library.connection, "Databases", options())?;
        assert_eq!(all_result_ids(&title), [root]);
        assert!(!all_result_ids(&title).contains(&paper));
        assert!(!all_result_ids(&title).contains(&unrelated));

        let mixed = search(&library.connection, "Databases indexing", options())?;
        let mixed_result = first_result(&mixed)?;
        assert_eq!(mixed_result.node_id, paper);
        assert!(mixed_result.match_reasons.contains(&MatchReason::Lexical));
        assert!(
            mixed_result
                .snippet
                .as_deref()
                .is_some_and(|snippet| snippet.contains(HIGHLIGHT_START))
        );

        // FTS punctuation and operators remain quoted data. This must not
        // produce a syntax error or turn `title:`/`*` into user-controlled FTS.
        let escaped = search(&library.connection, "title:indexing*", options())?;
        assert!(escaped.results.is_empty());
        Ok(())
    }

    #[test]
    fn and_or_prefix_scope_and_kind_passes_use_hard_filters() -> TestResult {
        let mut library = TestLibrary::new()?;
        let (root, transactions, local_source, remote_source) =
            library.populate(|transaction| {
                let root = tree::create_root(transaction, "Local", "")?;
                let transactions = tree::add_node(
                    transaction,
                    root,
                    &child(NodeKind::Topic, "Transactions", "alpha material"),
                )?;
                let local_source = tree::add_node(
                    transaction,
                    root,
                    &child(NodeKind::Source, "Local paper", "snapshot anomaly"),
                )?;
                let remote_root = tree::create_root(transaction, "Remote", "")?;
                let remote_source = tree::add_node(
                    transaction,
                    remote_root,
                    &child(NodeKind::Source, "Remote paper", "snapshot anomaly"),
                )?;
                Ok((root, transactions, local_source, remote_source))
            })?;

        let mut scoped = options();
        scoped.within = Some(root);
        scoped.explain = true;
        let or_fallback = search(&library.connection, "alpha absent", scoped)?;
        assert!(all_result_ids(&or_fallback).contains(&transactions));
        let Some(search_explanation) = &or_fallback.explanation else {
            return Err(std::io::Error::other("search explanation was omitted").into());
        };
        assert!(search_explanation.or_fallback_used);
        let transaction_explanation = or_fallback
            .results
            .iter()
            .find(|result| result.node_id == transactions)
            .and_then(|result| result.explanation.as_ref());
        assert!(transaction_explanation.is_some_and(|explanation| {
            explanation.retrieval_pass.as_deref() == Some("or_fallback")
                && explanation.lexical_rank.is_some()
                && explanation.final_position.is_some()
        }));

        let prefix = search(&library.connection, "transact", scoped)?;
        let prefix_result = first_result(&prefix)?;
        assert_eq!(prefix_result.node_id, transactions);
        assert!(prefix_result.match_reasons.contains(&MatchReason::Prefix));

        scoped.kind = SearchKind::Source;
        let local = search(&library.connection, "anomaly", scoped)?;
        assert_eq!(all_result_ids(&local), [local_source]);

        scoped.kind = SearchKind::Topic;
        let no_topics = search(&library.connection, "anomaly", scoped)?;
        assert!(no_topics.results.is_empty());

        let mut global_sources = options();
        global_sources.kind = SearchKind::Source;
        let global = search(&library.connection, "anomaly", global_sources)?;
        let global_ids = all_result_ids(&global).into_iter().collect::<BTreeSet<_>>();
        assert_eq!(global_ids, BTreeSet::from([local_source, remote_source]));
        Ok(())
    }

    #[test]
    fn quoted_phrase_reason_is_only_attached_to_phrase_matches() -> TestResult {
        let mut library = TestLibrary::new()?;
        let exact_phrase = library.populate(|transaction| {
            let root = tree::create_root(transaction, "Root", "")?;
            let exact_phrase = tree::add_node(
                transaction,
                root,
                &child(
                    NodeKind::Source,
                    "Exact",
                    "serializable isolation guarantees",
                ),
            )?;
            let _separated = tree::add_node(
                transaction,
                root,
                &child(
                    NodeKind::Source,
                    "Separated",
                    "serializable robust isolation",
                ),
            )?;
            Ok(exact_phrase)
        })?;

        let phrase = search(&library.connection, "\"serializable isolation\"", options())?;
        assert_eq!(all_result_ids(&phrase), [exact_phrase]);
        assert!(
            first_result(&phrase)?
                .match_reasons
                .contains(&MatchReason::Phrase)
        );
        Ok(())
    }

    #[test]
    fn passage_hits_roll_up_to_one_node_with_canonical_offsets() -> TestResult {
        let mut library = TestLibrary::new()?;
        let body = (0..2_500)
            .map(|position| {
                if matches!(position, 100 | 2_200) {
                    "quasar"
                } else {
                    "filler"
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let source = library.populate(|transaction| {
            let root = tree::create_root(transaction, "Root", "")?;
            tree::add_node(
                transaction,
                root,
                &child(NodeKind::Source, "Long source", &body),
            )
        })?;

        let unit_count = library.connection.query_row(
            "SELECT COUNT(*) FROM search_units WHERE node_id = ?1",
            [source],
            |row| row.get::<_, i64>(0),
        )?;
        assert!(unit_count >= 2);

        let mut source_only = options();
        source_only.kind = SearchKind::Source;
        let output = search(&library.connection, "quasar", source_only)?;
        assert_eq!(output.results.len(), 1);
        let result = first_result(&output)?;
        assert_eq!(result.node_id, source);
        assert!(result.related_hits.is_empty());
        let Some(range) = result.body_range else {
            return Err(std::io::Error::other("passage result lacked byte offsets").into());
        };
        assert!(range.start_byte < range.end_byte);
        assert!(range.end_byte <= body.len());
        assert!(body.is_char_boundary(range.start_byte));
        assert!(body.is_char_boundary(range.end_byte));

        let title_match = search(&library.connection, "Long source", source_only)?;
        let title_result = first_result(&title_match)?;
        assert_eq!(title_result.node_id, source);
        assert_eq!(title_result.body_range, None);
        assert_eq!(title_result.snippet, None);
        Ok(())
    }

    #[test]
    fn passage_rollup_ignores_repeated_title_hits_and_caps_body_evidence() {
        let title_hits = vec![
            unit_hit(1, 0, 1_000, 1.0, false),
            unit_hit(2, 1_000, 2_000, 0.9, false),
        ];
        assert!((rolled_up_score(&title_hits) - 1.0).abs() < 1e-12);

        let body_hits = vec![
            unit_hit(1, 0, 1_000, 1.0, true),
            unit_hit(2, 1_000, 2_000, 0.9, true),
        ];
        assert!((rolled_up_score(&body_hits) - 1.08).abs() < 1e-12);
    }

    fn unit_hit(
        unit_id: i64,
        start_byte: i64,
        end_byte: i64,
        score: f64,
        body_match: bool,
    ) -> UnitHit {
        UnitHit {
            unit_id,
            unit_no: unit_id,
            snippet: None,
            start_byte: Some(start_byte),
            end_byte: Some(end_byte),
            raw_bm25: -score,
            lexical_rank: 0,
            score,
            pass: RetrievalPass::And,
            body_match,
        }
    }

    #[test]
    fn chain_grouping_keeps_cousins_independent_and_support_explainable() -> TestResult {
        let mut library = TestLibrary::new()?;
        let (root, left, right) = library.populate(|transaction| {
            let root = tree::create_root(transaction, "Root", "sharedword overview")?;
            let left = tree::add_node(
                transaction,
                root,
                &child(NodeKind::Topic, "Left", "sharedword left"),
            )?;
            let right = tree::add_node(
                transaction,
                root,
                &child(NodeKind::Topic, "Right", "sharedword right"),
            )?;
            Ok((root, left, right))
        })?;

        let mut scoped = options();
        scoped.within = Some(root);
        let output = search(&library.connection, "sharedword", scoped)?;
        assert_eq!(output.results.len(), 2);
        assert_eq!(
            all_result_ids(&output).into_iter().collect::<BTreeSet<_>>(),
            BTreeSet::from([root, left, right])
        );
        assert_eq!(
            output
                .results
                .iter()
                .map(|result| result.related_hits.len())
                .sum::<usize>(),
            1
        );
        let root_reasons = output
            .results
            .iter()
            .find(|result| result.node_id == root)
            .map(|result| &result.match_reasons);
        assert!(
            root_reasons
                .is_some_and(|reasons| { reasons.contains(&MatchReason::DescendantSupport) })
        );
        Ok(())
    }

    #[test]
    fn detail_mode_selects_the_preferred_primary_when_direct_matches_are_close() -> TestResult {
        let mut library = TestLibrary::new()?;
        let (root, source) = library.populate(|transaction| {
            let root = tree::create_root(transaction, "Root", "modeword")?;
            let source = tree::add_node(
                transaction,
                root,
                &child(NodeKind::Source, "Source", "modeword"),
            )?;
            Ok((root, source))
        })?;

        let mut search_options = options();
        search_options.within = Some(root);
        search_options.detail = Detail::Overview;
        let overview = search(&library.connection, "modeword", search_options)?;
        assert_eq!(first_result(&overview)?.node_id, root);

        search_options.detail = Detail::Source;
        let source_detail = search(&library.connection, "modeword", search_options)?;
        assert_eq!(first_result(&source_detail)?.node_id, source);
        assert_eq!(
            all_result_ids(&source_detail)
                .into_iter()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([root, source])
        );
        Ok(())
    }

    #[test]
    fn global_and_chain_ranking_use_their_documented_score_precedence() {
        let mut stronger_direct = quota_candidate(100, 10, false);
        stronger_direct.direct_score = 0.8;
        let mut stronger_final = quota_candidate(200, 20, false);
        stronger_final.direct_score = 0.75;
        stronger_final.support_score = 0.5;

        assert_eq!(
            compare_chain_candidates(&stronger_direct, &stronger_final, Detail::Balanced),
            Ordering::Less
        );

        let mut global = vec![stronger_direct, stronger_final];
        sort_global_candidates(&mut global);
        assert_eq!(
            global
                .iter()
                .map(|candidate| candidate.node_id)
                .collect::<Vec<_>>(),
            [200, 100]
        );
    }

    #[test]
    fn branch_quota_diversifies_and_order_is_repeatable() -> TestResult {
        let mut library = TestLibrary::new()?;
        let (root, a_leaves, b_leaf) = library.populate(|transaction| {
            let root = tree::create_root(transaction, "Root", "")?;
            let branch_a =
                tree::add_node(transaction, root, &child(NodeKind::Topic, "Branch A", ""))?;
            let branch_b =
                tree::add_node(transaction, root, &child(NodeKind::Topic, "Branch B", ""))?;
            let mut a_leaves = Vec::new();
            for title in ["A one", "A two", "A three"] {
                a_leaves.push(tree::add_node(
                    transaction,
                    branch_a,
                    &child(NodeKind::Topic, title, "needle"),
                )?);
            }
            let b_leaf = tree::add_node(
                transaction,
                branch_b,
                &child(NodeKind::Topic, "B one", "needle"),
            )?;
            Ok((root, a_leaves, b_leaf))
        })?;

        let mut scoped = options();
        scoped.within = Some(root);
        scoped.limit = 3;
        let first = search(&library.connection, "needle", scoped)?;
        let second = search(&library.connection, "needle", scoped)?;
        let first_ids = first
            .results
            .iter()
            .map(|result| result.node_id)
            .collect::<Vec<_>>();
        let second_ids = second
            .results
            .iter()
            .map(|result| result.node_id)
            .collect::<Vec<_>>();
        assert_eq!(first_ids, second_ids);
        assert_eq!(first_ids.len(), 3);
        assert!(first_ids.contains(&b_leaf));
        assert_eq!(
            first_ids
                .iter()
                .filter(|node_id| a_leaves.contains(node_id))
                .count(),
            2
        );
        Ok(())
    }

    #[test]
    fn exact_matches_do_not_consume_branch_quota() {
        let ranked = vec![
            quota_candidate(100, 10, true),
            quota_candidate(101, 10, false),
            quota_candidate(102, 10, false),
            quota_candidate(103, 10, false),
            quota_candidate(200, 20, false),
        ];
        let node_ids = diversify(ranked, Some(1), 4)
            .into_iter()
            .map(|candidate| candidate.node_id)
            .collect::<Vec<_>>();
        assert_eq!(node_ids, [100, 101, 102, 200]);
    }

    fn quota_candidate(node_id: i64, branch_id: i64, exact: bool) -> Candidate {
        let mut candidate = Candidate::new(
            node_id,
            NodeKind::Topic,
            format!("Node {node_id}"),
            format!("root / branch {branch_id} / node {node_id}"),
        );
        candidate.exact_class = if exact { 2 } else { 0 };
        candidate.direct_score = 0.5;
        candidate.breadcrumb = vec![
            BreadcrumbItem {
                id: 1,
                title: "Root".to_owned(),
            },
            BreadcrumbItem {
                id: branch_id,
                title: format!("Branch {branch_id}"),
            },
            BreadcrumbItem {
                id: node_id,
                title: format!("Node {node_id}"),
            },
        ];
        candidate
    }
}
