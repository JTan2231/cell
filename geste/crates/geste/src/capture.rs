use std::collections::{BTreeSet, HashSet};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unicode_normalization::UnicodeNormalization as _;

use crate::error::{AppError, AppResult};
use crate::model::{Capture, SettlementStatus, SourceRole};

pub const MAX_INPUT_BYTES: usize = 256 * 1024;
pub const MAX_INPUT_BYTES_U64: u64 = 256 * 1024;
const MAX_COLLECTION: usize = 128;
const MAX_TAGS: usize = 64;
const MAX_TAG_SCALARS: usize = 64;
const MAX_COLLECTION_TEXT_SCALARS: usize = 2_000;

pub(crate) fn parse_capture(bytes: &[u8]) -> AppResult<Capture> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(AppError::usage(
            "input_too_large",
            "capture input exceeds the 256 KiB limit",
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| AppError::usage("invalid_utf8", "capture input must be valid UTF-8"))?;
    let capture: Capture = serde_json::from_str(text).map_err(|_| {
        AppError::usage(
            "invalid_capture_json",
            "capture must be strict JSON matching schema version 1",
        )
    })?;
    validate_capture(&capture)?;
    Ok(capture)
}

#[allow(clippy::too_many_lines)]
pub(crate) fn validate_capture(capture: &Capture) -> AppResult<()> {
    if capture.schema_version != 1 {
        return Err(AppError::usage(
            "unsupported_capture_schema",
            format!(
                "capture schema {} is unsupported; expected schema 1",
                capture.schema_version
            ),
        ));
    }

    bounded_text("title", &capture.title, 200)?;
    bounded_text("shape", &capture.shape, 1_000)?;
    bounded_text("recorded_by", &capture.recorded_by, 200)?;
    bounded_text("situation", &capture.situation, 4_000)?;
    bounded_text("response", &capture.response, 4_000)?;
    bounded_text("outcome.summary", &capture.outcome.summary, 4_000)?;
    bounded_text("applicability", &capture.applicability, 4_000)?;
    let cutoff = parse_time("basis_cutoff_at", &capture.basis_cutoff_at)?;

    bounded_collection("actions", capture.actions.len(), MAX_COLLECTION)?;
    bounded_collection("lessons", capture.lessons.len(), MAX_COLLECTION)?;
    bounded_collection("settlements", capture.settlements.len(), MAX_COLLECTION)?;
    bounded_collection("tags", capture.tags.len(), MAX_TAGS)?;
    bounded_collection("gaps", capture.gaps.len(), MAX_COLLECTION)?;
    bounded_collection("sources", capture.sources.len(), MAX_COLLECTION)?;
    bounded_collection(
        "related_episodes",
        capture.related_episodes.len(),
        MAX_COLLECTION,
    )?;
    if capture.sources.is_empty() {
        return Err(AppError::usage(
            "source_required",
            "capture must contain at least one source anchor",
        ));
    }

    for (index, value) in capture.actions.iter().enumerate() {
        bounded_text(
            &format!("actions[{}]", index + 1),
            value,
            MAX_COLLECTION_TEXT_SCALARS,
        )?;
    }
    for (index, value) in capture.lessons.iter().enumerate() {
        bounded_text(
            &format!("lessons[{}]", index + 1),
            value,
            MAX_COLLECTION_TEXT_SCALARS,
        )?;
    }
    for (index, value) in capture.gaps.iter().enumerate() {
        bounded_text(
            &format!("gaps[{}]", index + 1),
            value,
            MAX_COLLECTION_TEXT_SCALARS,
        )?;
    }

    let mut normalized_tags = HashSet::new();
    for (index, tag) in capture.tags.iter().enumerate() {
        bounded_text(&format!("tags[{}]", index + 1), tag, MAX_TAG_SCALARS)?;
        if !normalized_tags.insert(normalize(tag)) {
            return Err(AppError::usage(
                "duplicate_tag",
                format!("tags[{}] duplicates another normalized tag", index + 1),
            ));
        }
    }

    let mut settlement_ids = HashSet::new();
    for (index, settlement) in capture.settlements.iter().enumerate() {
        bounded_text(
            &format!("settlements[{}].id", index + 1),
            &settlement.id,
            MAX_COLLECTION_TEXT_SCALARS,
        )?;
        bounded_text(
            &format!("settlements[{}].statement", index + 1),
            &settlement.statement,
            MAX_COLLECTION_TEXT_SCALARS,
        )?;
        if !settlement_ids.insert(settlement.id.as_str()) {
            return Err(AppError::usage(
                "duplicate_settlement_id",
                format!("settlement id {:?} is duplicated", settlement.id),
            ));
        }
        match settlement.status {
            SettlementStatus::Verified => {
                if settlement.gap.is_some() {
                    return Err(AppError::usage(
                        "verified_settlement_gap_forbidden",
                        format!(
                            "verified settlement {:?} must set gap to null",
                            settlement.id
                        ),
                    ));
                }
            }
            SettlementStatus::Unverified => {
                let gap = settlement.gap.as_deref().ok_or_else(|| {
                    AppError::usage(
                        "unverified_settlement_gap_required",
                        format!(
                            "unverified settlement {:?} must name an existing gap:N",
                            settlement.id
                        ),
                    )
                })?;
                validate_gap_target(gap, capture.gaps.len())?;
            }
        }
    }

    let valid_targets = support_targets(capture);
    let mut source_ids = HashSet::new();
    for (index, source) in capture.sources.iter().enumerate() {
        for (field, value) in [
            ("id", source.id.as_str()),
            ("system", source.system.as_str()),
            ("kind", source.kind.as_str()),
            ("reference", source.reference.as_str()),
            ("label", source.label.as_str()),
        ] {
            bounded_text(
                &format!("sources[{}].{field}", index + 1),
                value,
                MAX_COLLECTION_TEXT_SCALARS,
            )?;
        }
        if !valid_system(&source.system) {
            return Err(AppError::usage(
                "invalid_source_system",
                format!(
                    "sources[{}].system must be a lowercase product namespace or other",
                    index + 1
                ),
            ));
        }
        if !source_ids.insert(source.id.as_str()) {
            return Err(AppError::usage(
                "duplicate_source_id",
                format!("source id {:?} is duplicated", source.id),
            ));
        }
        if let Some(revision) = source.revision.as_deref() {
            bounded_text(
                &format!("sources[{}].revision", index + 1),
                revision,
                MAX_COLLECTION_TEXT_SCALARS,
            )?;
        }
        if let Some(digest) = source.digest.as_deref()
            && (digest.len() != 64 || !is_lower_hex(digest))
        {
            return Err(AppError::usage(
                "invalid_source_digest",
                format!(
                    "sources[{}].digest must be 64 lowercase hexadecimal SHA-256 characters",
                    index + 1
                ),
            ));
        }
        if source.system == "git"
            && source.digest.is_none()
            && let Some(revision) = source.revision.as_deref()
            && !((revision.len() == 40 || revision.len() == 64) && is_lower_hex(revision))
        {
            return Err(AppError::usage(
                "invalid_git_revision",
                format!(
                    "sources[{}].revision must be a full 40- or 64-character lowercase hexadecimal Git object ID when digest is null",
                    index + 1
                ),
            ));
        }
        let observed = parse_time(
            &format!("sources[{}].observed_at", index + 1),
            &source.observed_at,
        )?;
        if observed > cutoff {
            return Err(AppError::usage(
                "source_after_cutoff",
                format!(
                    "sources[{}].observed_at is later than basis_cutoff_at",
                    index + 1
                ),
            ));
        }
        bounded_collection(
            &format!("sources[{}].supports", index + 1),
            source.supports.len(),
            MAX_COLLECTION,
        )?;
        let mut seen_supports = HashSet::new();
        for target in &source.supports {
            bounded_text(
                &format!("sources[{}].supports", index + 1),
                target,
                MAX_COLLECTION_TEXT_SCALARS,
            )?;
            if !valid_targets.contains(target) {
                return Err(AppError::usage(
                    "invalid_support_target",
                    format!(
                        "source {:?} names unknown support target {target:?}",
                        source.id
                    ),
                ));
            }
            if !seen_supports.insert(target) {
                return Err(AppError::usage(
                    "duplicate_support_target",
                    format!("source {:?} repeats support target {target:?}", source.id),
                ));
            }
        }
    }

    for settlement in &capture.settlements {
        if settlement.status == SettlementStatus::Verified {
            let target = format!("settlement:{}", settlement.id);
            let grounded = capture.sources.iter().any(|source| {
                source.system == "decisions"
                    && source.kind == "lifecycle_event"
                    && source.role == SourceRole::Authority
                    && source.supports.iter().any(|support| support == &target)
            });
            if !grounded {
                return Err(AppError::usage(
                    "verified_settlement_not_grounded",
                    format!(
                        "verified settlement {:?} requires a decisions/lifecycle_event authority source that supports {target:?}",
                        settlement.id
                    ),
                ));
            }
        }
    }

    let mut related = HashSet::new();
    for (index, link) in capture.related_episodes.iter().enumerate() {
        let related_id = parse_episode_id(&link.episode)?;
        if link.revision == 0 {
            return Err(AppError::usage(
                "invalid_related_revision",
                format!(
                    "related_episodes[{}].revision must be at least 1",
                    index + 1
                ),
            ));
        }
        let key = (related_id, link.revision, link.relation.as_str());
        if !related.insert(key) {
            return Err(AppError::usage(
                "duplicate_related_episode",
                format!("related_episodes[{}] duplicates an earlier link", index + 1),
            ));
        }
    }
    Ok(())
}

fn bounded_collection(field: &str, actual: usize, maximum: usize) -> AppResult<()> {
    if actual > maximum {
        return Err(AppError::usage(
            "collection_too_large",
            format!("{field} has {actual} members; maximum is {maximum}"),
        ));
    }
    Ok(())
}

fn bounded_text(field: &str, value: &str, maximum: usize) -> AppResult<()> {
    if value.trim().is_empty() {
        return Err(AppError::usage(
            "blank_field",
            format!("{field} must not be blank"),
        ));
    }
    let length = value.chars().count();
    if length > maximum {
        return Err(AppError::usage(
            "field_too_long",
            format!("{field} has {length} Unicode scalar values; maximum is {maximum}"),
        ));
    }
    Ok(())
}

fn parse_time(field: &str, value: &str) -> AppResult<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| {
        AppError::usage(
            "invalid_timestamp",
            format!("{field} must be an RFC 3339 timestamp"),
        )
    })
}

fn validate_gap_target(target: &str, gap_count: usize) -> AppResult<usize> {
    let Some(number) = target.strip_prefix("gap:") else {
        return Err(AppError::usage(
            "invalid_gap_target",
            format!("{target:?} must name an existing gap:N"),
        ));
    };
    let ordinal = number.parse::<usize>().ok().filter(|value| *value >= 1);
    let Some(ordinal) = ordinal.filter(|value| *value <= gap_count) else {
        return Err(AppError::usage(
            "invalid_gap_target",
            format!("{target:?} does not name an existing gap"),
        ));
    };
    if number.starts_with('0') {
        return Err(AppError::usage(
            "invalid_gap_target",
            format!("{target:?} must use the canonical gap:N form"),
        ));
    }
    Ok(ordinal)
}

pub(crate) fn gap_ordinal(target: &str) -> AppResult<i64> {
    target
        .strip_prefix("gap:")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= 1)
        .ok_or_else(|| {
            AppError::usage(
                "invalid_gap_target",
                format!("{target:?} must name an existing gap:N"),
            )
        })
}

fn support_targets(capture: &Capture) -> HashSet<String> {
    let mut targets = HashSet::from([
        "shape".to_owned(),
        "situation".to_owned(),
        "response".to_owned(),
        "outcome".to_owned(),
        "applicability".to_owned(),
    ]);
    for index in 1..=capture.actions.len() {
        targets.insert(format!("action:{index}"));
    }
    for index in 1..=capture.lessons.len() {
        targets.insert(format!("lesson:{index}"));
    }
    for settlement in &capture.settlements {
        targets.insert(format!("settlement:{}", settlement.id));
    }
    for index in 1..=capture.gaps.len() {
        targets.insert(format!("gap:{index}"));
    }
    targets
}

fn valid_system(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[must_use]
pub fn normalize(value: &str) -> String {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn query_terms(query: &str) -> AppResult<Vec<String>> {
    let normalized = normalize(query);
    if normalized.is_empty() {
        return Err(AppError::usage(
            "blank_query",
            "search query must contain at least one term",
        ));
    }
    let terms: Vec<String> = normalized
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect();
    if terms.len() > 16 {
        return Err(AppError::usage(
            "query_too_many_terms",
            "search query may contain at most 16 terms",
        ));
    }
    let mut unique = BTreeSet::new();
    for term in &terms {
        if !unique.insert(term) {
            return Err(AppError::usage(
                "duplicate_query_term",
                format!("search query repeats normalized term {term:?}"),
            ));
        }
    }
    Ok(terms)
}

pub(crate) fn parse_episode_id(value: &str) -> AppResult<i64> {
    let Some(number) = value.strip_prefix('e') else {
        return Err(AppError::usage(
            "invalid_episode_id",
            format!("{value:?} is not a canonical episode ID such as e1"),
        ));
    };
    if number.is_empty()
        || number.starts_with('0')
        || !number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(AppError::usage(
            "invalid_episode_id",
            format!("{value:?} is not a canonical episode ID such as e1"),
        ));
    }
    number
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| {
            AppError::usage(
                "invalid_episode_id",
                format!("{value:?} is not a canonical episode ID such as e1"),
            )
        })
}

#[must_use]
pub(crate) fn format_episode_id(value: i64) -> String {
    format!("e{value}")
}

#[cfg(test)]
mod tests {
    use super::{normalize, parse_episode_id, query_terms};

    #[test]
    fn normalization_is_nfkc_lowercase_and_collapsed() {
        assert_eq!(normalize("  Ｃross\tPRODUCT  "), "cross product");
    }

    #[test]
    fn query_rejects_duplicates_after_normalization() {
        let Err(error) = query_terms("Thing ＴＨＩＮＧ") else {
            panic!("duplicate normalized terms must be rejected");
        };
        assert_eq!(error.code(), "duplicate_query_term");
    }

    #[test]
    fn episode_ids_are_canonical() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(parse_episode_id("e12")?, 12);
        let Err(error) = parse_episode_id("e01") else {
            panic!("noncanonical episode ID must be rejected");
        };
        assert_eq!(error.code(), "invalid_episode_id");
        Ok(())
    }
}
