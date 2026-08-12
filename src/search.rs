use std::collections::HashMap;

use rusqlite::{Connection, params};

use crate::error::AppError;
use crate::index;
use crate::model::{
    BreadcrumbItem, MatchReason, ResultExplanation, SearchExplanation, SearchOutput, SearchResult,
};
use crate::render::{HIGHLIGHT_END, HIGHLIGHT_START};
use crate::tree;

const MAX_QUERY_BYTES: usize = 4096;
const MAX_QUERY_TERMS: usize = 32;
const MAX_LIMIT: usize = 100;
const MAX_CANDIDATES: usize = 500;

/// Search scope and presentation controls.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub within: Option<i64>,
    pub limit: usize,
    pub explain: bool,
}

#[derive(Debug, Clone)]
struct QueryTerm {
    text: String,
    phrase: bool,
}

#[derive(Debug)]
struct ParsedQuery {
    terms: Vec<QueryTerm>,
    exact_key: String,
    exact_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetrievalPass {
    And,
    Or,
    Prefix,
}

impl RetrievalPass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
            Self::Prefix => "prefix",
        }
    }

    const fn reason(self) -> MatchReason {
        match self {
            Self::Prefix => MatchReason::Prefix,
            Self::And | Self::Or => MatchReason::Lexical,
        }
    }
}

#[derive(Debug)]
struct Candidate {
    node_id: i64,
    unit_id: Option<i64>,
    text: String,
    breadcrumb: Vec<BreadcrumbItem>,
    snippet: Option<String>,
    reasons: Vec<MatchReason>,
    exact_class: u8,
    lexical_rank: Option<usize>,
    raw_bm25: Option<f64>,
    retrieval_pass: Option<RetrievalPass>,
    score: f64,
}

/// Search the current derived index and return deterministic node-level results.
pub fn search(
    connection: &Connection,
    query: &str,
    options: Options,
) -> Result<SearchOutput, AppError> {
    validate_options(options)?;
    index::require_current(connection)?;
    if let Some(scope) = options.within {
        tree::get_node(connection, scope)?;
    }
    let parsed = parse_query(query)?;
    let mut candidates = exact_candidates(connection, &parsed, options.within)?;
    let exact_count = candidates.len();
    let candidate_limit = options.limit.saturating_mul(10).clamp(50, MAX_CANDIDATES);

    collect_fts(
        connection,
        &fts_expression(&parsed.terms, " AND "),
        RetrievalPass::And,
        options.within,
        candidate_limit,
        &mut candidates,
    )?;
    let after_and = candidates.len();

    let or_fallback = candidates.len() < options.limit && parsed.terms.len() > 1;
    if or_fallback {
        collect_fts(
            connection,
            &fts_expression(&parsed.terms, " OR "),
            RetrievalPass::Or,
            options.within,
            candidate_limit,
            &mut candidates,
        )?;
    }
    let after_or = candidates.len();

    let prefix = prefix_expression(&parsed.terms);
    let prefix_fallback = candidates.len() < options.limit && prefix.is_some();
    if let Some(expression) = prefix.filter(|_| prefix_fallback) {
        collect_fts(
            connection,
            &expression,
            RetrievalPass::Prefix,
            options.within,
            candidate_limit,
            &mut candidates,
        )?;
    }
    let after_prefix = candidates.len();

    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .exact_class
            .cmp(&left.exact_class)
            .then_with(|| right.score.total_cmp(&left.score))
            .then_with(|| left.node_id.cmp(&right.node_id))
    });
    candidates.truncate(options.limit);

    let results = candidates
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| to_result(candidate, index + 1, options))
        .collect::<Vec<_>>();
    let explanation = options.explain.then_some(SearchExplanation {
        exact_candidates: exact_count,
        after_and_candidates: after_and,
        or_fallback_used: or_fallback,
        after_or_candidates: after_or,
        prefix_fallback_used: prefix_fallback,
        after_prefix_candidates: after_prefix,
        groups_after_collapse: results.len(),
        returned_results: results.len(),
    });
    Ok(SearchOutput {
        query: query.to_owned(),
        results,
        explanation,
    })
}

fn validate_options(options: Options) -> Result<(), AppError> {
    if options.limit == 0 || options.limit > MAX_LIMIT {
        return Err(AppError::invalid(
            "invalid_limit",
            format!("search limit must be between 1 and {MAX_LIMIT}"),
        ));
    }
    Ok(())
}

fn parse_query(query: &str) -> Result<ParsedQuery, AppError> {
    if query.trim().is_empty() {
        return Err(AppError::invalid(
            "empty_query",
            "search query cannot be empty",
        ));
    }
    if query.len() > MAX_QUERY_BYTES {
        return Err(AppError::invalid(
            "query_too_large",
            format!("search query cannot exceed {MAX_QUERY_BYTES} UTF-8 bytes"),
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
                } else {
                    push_term(&mut terms, &mut buffer, false);
                }
                quoted = !quoted;
            }
            character if character.is_whitespace() && !quoted => {
                push_term(&mut terms, &mut buffer, false);
            }
            character => buffer.push(character),
        }
    }
    if quoted {
        return Err(AppError::invalid(
            "invalid_query",
            "search query contains an unterminated quoted phrase",
        ));
    }
    push_term(&mut terms, &mut buffer, false);
    if terms.is_empty() {
        return Err(AppError::invalid(
            "empty_query",
            "search query contains no searchable text",
        ));
    }
    if terms.len() > MAX_QUERY_TERMS {
        return Err(AppError::invalid(
            "query_too_large",
            format!("search query cannot contain more than {MAX_QUERY_TERMS} terms"),
        ));
    }
    Ok(ParsedQuery {
        terms,
        exact_key: index::normalize_key(query),
        exact_id: query.trim().parse::<i64>().ok().filter(|id| *id > 0),
    })
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
    let suffix = format!("{}*", quoted_fts(&last.text));
    if preceding.is_empty() {
        Some(suffix)
    } else {
        Some(format!(
            "{} AND {suffix}",
            fts_expression(preceding, " AND ")
        ))
    }
}

fn exact_candidates(
    connection: &Connection,
    query: &ParsedQuery,
    scope: Option<i64>,
) -> Result<HashMap<i64, Candidate>, AppError> {
    let exact_id = query.exact_id.unwrap_or(-1);
    let mut statement = connection.prepare(
        "SELECT id, node_id, text, normalized_text, breadcrumb, normalized_path \
         FROM search_units \
         WHERE node_id = ?1 OR normalized_text = ?2 OR normalized_path = ?2 \
         ORDER BY node_id",
    )?;
    let rows = statement.query_map(params![exact_id, query.exact_key], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    let mut candidates = HashMap::new();
    for row in rows {
        let (unit_id, node_id, text, normalized_text, _path, normalized_path) = row?;
        if !in_scope(connection, node_id, scope)? {
            continue;
        }
        let (class, reason) = if node_id == exact_id {
            (3, MatchReason::ExactId)
        } else if normalized_path == query.exact_key {
            (3, MatchReason::ExactPath)
        } else if normalized_text == query.exact_key {
            (2, MatchReason::ExactText)
        } else {
            continue;
        };
        candidates.insert(
            node_id,
            Candidate {
                node_id,
                unit_id: Some(unit_id),
                text,
                breadcrumb: load_breadcrumb(connection, node_id)?,
                snippet: None,
                reasons: vec![reason],
                exact_class: class,
                lexical_rank: None,
                raw_bm25: None,
                retrieval_pass: None,
                score: 100.0 + f64::from(class),
            },
        );
    }
    Ok(candidates)
}

#[derive(Debug)]
struct RawHit {
    unit_id: i64,
    node_id: i64,
    text: String,
    raw_bm25: f64,
    snippet: String,
}

fn collect_fts(
    connection: &Connection,
    expression: &str,
    pass: RetrievalPass,
    scope: Option<i64>,
    limit: usize,
    candidates: &mut HashMap<i64, Candidate>,
) -> Result<(), AppError> {
    let marker_start = HIGHLIGHT_START.to_string();
    let marker_end = HIGHLIGHT_END.to_string();
    let limit = i64::try_from(limit)
        .map_err(|_| AppError::invalid("invalid_limit", "candidate limit is too large"))?;
    let rows = if let Some(scope) = scope {
        let mut statement = connection.prepare(
            "WITH RECURSIVE subtree(id) AS (\
                 SELECT ?1 UNION ALL \
                 SELECT child.id FROM nodes AS child JOIN subtree ON child.parent_id = subtree.id\
             ) \
             SELECT su.id, su.node_id, su.text, bm25(search_fts, 8.0, 1.0), \
                    snippet(search_fts, 0, ?3, ?4, '…', 24) \
             FROM search_fts JOIN search_units AS su ON su.id = search_fts.rowid \
             WHERE search_fts MATCH ?2 AND su.node_id IN subtree \
             ORDER BY bm25(search_fts, 8.0, 1.0), su.node_id LIMIT ?5",
        )?;
        statement
            .query_map(
                params![scope, expression, marker_start, marker_end, limit],
                raw_hit,
            )?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let mut statement = connection.prepare(
            "SELECT su.id, su.node_id, su.text, bm25(search_fts, 8.0, 1.0), \
                    snippet(search_fts, 0, ?2, ?3, '…', 24) \
             FROM search_fts JOIN search_units AS su ON su.id = search_fts.rowid \
             WHERE search_fts MATCH ?1 \
             ORDER BY bm25(search_fts, 8.0, 1.0), su.node_id LIMIT ?4",
        )?;
        statement
            .query_map(
                params![expression, marker_start, marker_end, limit],
                raw_hit,
            )?
            .collect::<Result<Vec<_>, _>>()?
    };

    for (rank, hit) in rows.into_iter().enumerate() {
        let rank_score = 10.0 / (f64::from(u32::try_from(rank).unwrap_or(u32::MAX)) + 1.0);
        let candidate = candidates.entry(hit.node_id).or_insert(Candidate {
            node_id: hit.node_id,
            unit_id: Some(hit.unit_id),
            text: hit.text,
            breadcrumb: load_breadcrumb(connection, hit.node_id)?,
            snippet: Some(hit.snippet.clone()),
            reasons: Vec::new(),
            exact_class: 0,
            lexical_rank: Some(rank + 1),
            raw_bm25: Some(hit.raw_bm25),
            retrieval_pass: Some(pass),
            score: rank_score,
        });
        push_reason(&mut candidate.reasons, pass.reason());
        if pass == RetrievalPass::And {
            push_reason(&mut candidate.reasons, MatchReason::Phrase);
        }
        if rank_score > candidate.score || candidate.snippet.is_none() {
            candidate.score = candidate.score.max(rank_score);
            candidate.unit_id = Some(hit.unit_id);
            candidate.snippet = Some(hit.snippet);
            candidate.lexical_rank = Some(rank + 1);
            candidate.raw_bm25 = Some(hit.raw_bm25);
            candidate.retrieval_pass = Some(pass);
        }
    }
    Ok(())
}

fn raw_hit(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawHit> {
    Ok(RawHit {
        unit_id: row.get(0)?,
        node_id: row.get(1)?,
        text: row.get(2)?,
        raw_bm25: row.get(3)?,
        snippet: row.get(4)?,
    })
}

fn in_scope(connection: &Connection, node_id: i64, scope: Option<i64>) -> Result<bool, AppError> {
    let Some(scope) = scope else {
        return Ok(true);
    };
    let found = connection.query_row(
        "WITH RECURSIVE ancestors(id, parent_id) AS (\
             SELECT id, parent_id FROM nodes WHERE id = ?1 \
             UNION ALL \
             SELECT parent.id, parent.parent_id \
             FROM nodes AS parent JOIN ancestors AS child ON child.parent_id = parent.id\
         ) SELECT EXISTS(SELECT 1 FROM ancestors WHERE id = ?2)",
        params![node_id, scope],
        |row| row.get::<_, bool>(0),
    )?;
    Ok(found)
}

fn load_breadcrumb(connection: &Connection, node_id: i64) -> Result<Vec<BreadcrumbItem>, AppError> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE ancestors(id, parent_id, text, distance) AS (\
             SELECT id, parent_id, text, 0 FROM nodes WHERE id = ?1 \
             UNION ALL \
             SELECT parent.id, parent.parent_id, parent.text, child.distance + 1 \
             FROM nodes AS parent JOIN ancestors AS child ON child.parent_id = parent.id\
         ) SELECT id, text FROM ancestors ORDER BY distance DESC",
    )?;
    let rows = statement.query_map([node_id], |row| {
        Ok(BreadcrumbItem {
            id: row.get(0)?,
            text: row.get(1)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn push_reason(reasons: &mut Vec<MatchReason>, reason: MatchReason) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

fn to_result(candidate: Candidate, rank: usize, options: Options) -> SearchResult {
    let branch_key = branch_key(&candidate.breadcrumb, options.within, candidate.node_id);
    let exact_class = match candidate.reasons.first() {
        Some(MatchReason::ExactId) => "identifier",
        Some(MatchReason::ExactPath) => "path",
        Some(MatchReason::ExactText) => "text",
        _ => "none",
    };
    let explanation = options.explain.then_some(ResultExplanation {
        primary_unit_id: candidate.unit_id,
        raw_bm25: candidate.raw_bm25,
        lexical_rank: candidate.lexical_rank,
        retrieval_pass: candidate
            .retrieval_pass
            .map(|pass| pass.as_str().to_owned()),
        exact_class: exact_class.to_owned(),
        direct_score: candidate.score,
        support_score: 0.0,
        support_node_id: None,
        chain_group_node_id: candidate.node_id,
        grouping_reason: "independent_result".to_owned(),
        branch_key,
        diversity_reason: "ranked".to_owned(),
        final_position: Some(rank),
    });
    SearchResult {
        rank,
        node_id: candidate.node_id,
        text: candidate.text,
        breadcrumb: candidate.breadcrumb,
        snippet: candidate.snippet,
        match_reasons: candidate.reasons,
        related_hits: Vec::new(),
        explanation,
    }
}

fn branch_key(breadcrumb: &[BreadcrumbItem], scope: Option<i64>, fallback: i64) -> i64 {
    match scope {
        Some(scope) => breadcrumb
            .iter()
            .position(|item| item.id == scope)
            .and_then(|index| breadcrumb.get(index + 1))
            .map_or(scope, |item| item.id),
        None => breadcrumb.first().map_or(fallback, |item| item.id),
    }
}

#[cfg(test)]
mod tests {
    use super::{fts_expression, parse_query, prefix_expression};

    #[test]
    fn parses_phrases_without_exposing_fts_syntax() -> Result<(), crate::error::AppError> {
        let query = parse_query("sqlite \"write skew\"")?;
        assert_eq!(
            fts_expression(&query.terms, " AND "),
            "\"sqlite\" AND \"write skew\""
        );
        assert!(prefix_expression(&query.terms).is_none());
        Ok(())
    }

    #[test]
    fn rejects_unterminated_phrases() {
        let result = parse_query("\"unfinished");
        assert!(result.is_err_and(|error| error.code() == "invalid_query"));
    }
}
