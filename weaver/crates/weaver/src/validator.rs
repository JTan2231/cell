use std::collections::BTreeSet;
use std::fs;

use regex::Regex;

use crate::error::{AppResult, WeaverError};
use crate::project::{Project, STAGES};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Verdict {
    Pass,
    Revise,
    Blocked,
}

impl Verdict {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Revise => "REVISE",
            Self::Blocked => "BLOCKED",
        }
    }
}

pub(crate) fn check(project: &Project) -> AppResult<Verdict> {
    let mut outputs = Vec::with_capacity(STAGES.len());
    for stage in STAGES {
        let path = project.output_path(stage);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            WeaverError::runtime(format!(
                "{} is missing or unreadable: {error}",
                project.output_relative(stage)
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
            return Err(WeaverError::runtime(format!(
                "{} must be a nonempty non-symlink regular file",
                project.output_relative(stage)
            )));
        }
        let stage_root = path.parent().ok_or_else(|| {
            WeaverError::runtime(format!("output has no parent: {}", path.display()))
        })?;
        let stage_metadata = fs::symlink_metadata(stage_root).map_err(|error| {
            WeaverError::runtime(format!("cannot inspect {}: {error}", stage_root.display()))
        })?;
        if stage_metadata.file_type().is_symlink() || !stage_metadata.is_dir() {
            return Err(WeaverError::runtime(format!(
                "{} must be a non-symlink directory",
                stage_root.display()
            )));
        }
        outputs.push(fs::read(&path).map_err(|error| {
            WeaverError::runtime(format!("cannot read {}: {error}", path.display()))
        })?);
    }

    let stories = decode(&outputs[0], &project.output_relative(STAGES[0]))?;
    let themes = decode(&outputs[1], &project.output_relative(STAGES[1]))?;
    let draft = decode(&outputs[2], &project.output_relative(STAGES[2]))?;
    let review = decode(&outputs[3], &project.output_relative(STAGES[3]))?;
    let final_output = decode(&outputs[4], &project.output_relative(STAGES[4]))?;

    let verdict = parse_verdict(review)?;
    for (content, label) in [
        (stories, project.output_relative(STAGES[0])),
        (draft, project.output_relative(STAGES[2])),
        (final_output, project.output_relative(STAGES[4])),
    ] {
        validate_reader_surface(content, &label)?;
    }

    let anchors = validate_story_anchors(stories, &project.output_relative(STAGES[0]))?;
    validate_story_links(themes, &project.output_relative(STAGES[1]), &anchors)?;
    validate_story_links(draft, &project.output_relative(STAGES[2]), &anchors)?;
    let final_link_count =
        validate_story_links(final_output, &project.output_relative(STAGES[4]), &anchors)?;

    match verdict {
        Verdict::Pass => {
            if outputs[2] != outputs[4] {
                return Err(WeaverError::runtime(format!(
                    "{} changed a draft that passed review",
                    project.output_relative(STAGES[4])
                )));
            }
            require_final_links(project, final_link_count)?;
        }
        Verdict::Revise => {
            if outputs[2] == outputs[4] {
                return Err(WeaverError::runtime(format!(
                    "{} did not change a draft that required revision",
                    project.output_relative(STAGES[4])
                )));
            }
            require_final_links(project, final_link_count)?;
        }
        Verdict::Blocked => {
            validate_blocked(project, review, final_output, final_link_count)?;
        }
    }
    Ok(verdict)
}

fn decode<'a>(bytes: &'a [u8], label: &str) -> AppResult<&'a str> {
    std::str::from_utf8(bytes)
        .map_err(|error| WeaverError::runtime(format!("{label} is not UTF-8: {error}")))
}

fn parse_verdict(review: &str) -> AppResult<Verdict> {
    let first = review
        .lines()
        .next()
        .unwrap_or_default()
        .trim_end_matches('\r');
    match first {
        "Verdict: PASS" => Ok(Verdict::Pass),
        "Verdict: REVISE" => Ok(Verdict::Revise),
        "Verdict: BLOCKED" => Ok(Verdict::Blocked),
        _ => Err(WeaverError::runtime(
            "04-review/output.md has no valid first-line verdict",
        )),
    }
}

fn validate_reader_surface(content: &str, label: &str) -> AppResult<()> {
    for (needle, reason) in [
        (
            "career-sourcebook/",
            "exposes a private career-sourcebook path",
        ),
        ("workflow/narrative/", "exposes an internal workflow path"),
        ("basis.md", "exposes an authored-input path"),
        ("brief.md", "exposes an authored-input path"),
        ("<!--", "contains a hidden HTML comment"),
    ] {
        if content.contains(needle) {
            return Err(WeaverError::runtime(format!("{label} {reason}")));
        }
    }
    if content
        .split('\n')
        .any(|line| line.trim_end_matches('\r').ends_with([' ', '\t']))
    {
        return Err(WeaverError::runtime(format!(
            "{label} contains trailing whitespace"
        )));
    }
    Ok(())
}

fn validate_story_anchors(content: &str, label: &str) -> AppResult<BTreeSet<String>> {
    let anchor_pattern = Regex::new(r#"^<a id="([a-z0-9-]+)"></a>$"#)
        .map_err(|error| WeaverError::runtime(format!("invalid anchor validator: {error}")))?;
    let anchor_reference = Regex::new(r"<a\s")
        .map_err(|error| WeaverError::runtime(format!("invalid anchor scanner: {error}")))?;
    let mut anchors = BTreeSet::new();
    let mut last_nonempty = "";
    for raw_line in content.split('\n') {
        let line = raw_line;
        if anchor_reference.is_match(line) {
            let captures = anchor_pattern.captures(line).ok_or_else(|| {
                WeaverError::runtime(format!(
                    "{label} contains a malformed explicit story anchor"
                ))
            })?;
            let id = captures.get(1).ok_or_else(|| {
                WeaverError::runtime(format!("{label} contains an unreadable story anchor"))
            })?;
            if !anchors.insert(id.as_str().to_owned()) {
                return Err(WeaverError::runtime(format!(
                    "{label} contains a duplicate explicit story anchor"
                )));
            }
        }
        if (line.starts_with("## ") || line.starts_with("##\t"))
            && !anchor_pattern.is_match(last_nonempty)
        {
            return Err(WeaverError::runtime(format!(
                "{label} contains a story heading without a preceding explicit anchor"
            )));
        }
        if !line.trim().is_empty() {
            last_nonempty = line;
        }
    }
    Ok(anchors)
}

fn validate_story_links(
    content: &str,
    label: &str,
    anchors: &BTreeSet<String>,
) -> AppResult<usize> {
    let token_pattern = Regex::new(r"[^\]\[()\s]*01-stories[^\]\[()\s]*")
        .map_err(|error| WeaverError::runtime(format!("invalid link scanner: {error}")))?;
    let target_pattern = Regex::new(r"^\.\./01-stories/output\.md#([a-z0-9-]+)$")
        .map_err(|error| WeaverError::runtime(format!("invalid link validator: {error}")))?;
    let mut count = 0;
    for token in token_pattern.find_iter(content) {
        let captures = target_pattern
            .captures(token.as_str())
            .ok_or_else(|| WeaverError::runtime(format!("malformed story link in {label}")))?;
        let fragment = captures
            .get(1)
            .ok_or_else(|| WeaverError::runtime(format!("unreadable story link in {label}")))?;
        if !anchors.contains(fragment.as_str()) {
            return Err(WeaverError::runtime(format!(
                "story link #{} does not match exactly one explicit anchor",
                fragment.as_str()
            )));
        }
        count += 1;
    }
    Ok(count)
}

fn require_final_links(project: &Project, count: usize) -> AppResult<()> {
    if count == 0 {
        return Err(WeaverError::runtime(format!(
            "{} has no story links",
            project.output_relative(STAGES[4])
        )));
    }
    Ok(())
}

fn validate_blocked(
    project: &Project,
    review: &str,
    final_output: &str,
    final_link_count: usize,
) -> AppResult<()> {
    if final_link_count != 0 {
        return Err(WeaverError::runtime(format!(
            "{} links stories despite a blocked review",
            project.output_relative(STAGES[4])
        )));
    }
    let lines: Vec<&str> = final_output
        .split_terminator('\n')
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    if lines.len() != 3
        || lines[0] != "# No publishable narrative"
        || !lines[1].is_empty()
        || lines[2].is_empty()
        || is_blocked_markup(lines[2])
    {
        return Err(WeaverError::runtime(format!(
            "{} must contain only its blocked heading and one explanation sentence",
            project.output_relative(STAGES[4])
        )));
    }

    let explanations: Vec<&str> = review
        .lines()
        .filter_map(|line| line.strip_prefix("Blocked explanation: "))
        .collect();
    if explanations.len() != 1 || explanations[0].is_empty() {
        return Err(WeaverError::runtime(format!(
            "{} must supply exactly one blocked explanation",
            project.output_relative(STAGES[3])
        )));
    }
    if lines[2] != explanations[0] {
        return Err(WeaverError::runtime(format!(
            "{} does not copy the review's blocked explanation exactly",
            project.output_relative(STAGES[4])
        )));
    }
    Ok(())
}

fn is_blocked_markup(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with(['#', '>', '*', '+', '-']) {
        return true;
    }
    let digit_prefix = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    digit_prefix > 0
        && trimmed[digit_prefix..].starts_with('.')
        && trimmed[digit_prefix + 1..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{Verdict, check, parse_verdict};
    use crate::project::{Project, STAGES};

    fn must<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("unexpected error: {error}"),
        }
    }

    fn fixture() -> (TempDir, Project) {
        let temporary = must(TempDir::new());
        let root = temporary.path();
        must(fs::create_dir(root.join("narratives")));
        must(fs::create_dir_all(root.join("workflow/narrative")));
        must(fs::create_dir(root.join("narratives/example")));
        must(fs::write(root.join("narratives/example/basis.md"), "basis"));
        must(fs::write(root.join("narratives/example/brief.md"), "brief"));
        let project = must(Project::resolve(root, "example", false));
        must(project.prepare_outputs());
        (temporary, project)
    }

    fn install_permitted_outputs(project: &Project, verdict: Verdict) {
        let stories = b"# Stories\n\n<a id=\"one-story\"></a>\n\n## One story\n\nA useful story.";
        let themes = b"# Themes\n\n[Story](../01-stories/output.md#one-story)";
        let draft = b"# Draft\n\n[Read more](../01-stories/output.md#one-story)";
        let review = match verdict {
            Verdict::Pass => b"Verdict: PASS\n\nNo changes.".as_slice(),
            Verdict::Revise => b"Verdict: REVISE\n\nChange the opening.".as_slice(),
            Verdict::Blocked => {
                b"Verdict: BLOCKED\n\nBlocked explanation: The factual record is incomplete."
                    .as_slice()
            }
        };
        let final_output = match verdict {
            Verdict::Pass => draft.as_slice(),
            Verdict::Revise => {
                b"# Better draft\n\n[Read more](../01-stories/output.md#one-story)".as_slice()
            }
            Verdict::Blocked => {
                b"# No publishable narrative\n\nThe factual record is incomplete.".as_slice()
            }
        };
        for (stage, output) in STAGES.into_iter().zip([
            stories.as_slice(),
            themes.as_slice(),
            draft.as_slice(),
            review,
            final_output,
        ]) {
            must(project.write_stage_output(stage, output));
        }
    }

    #[test]
    fn parses_exact_review_verdicts() {
        assert_eq!(must(parse_verdict("Verdict: PASS\n")), Verdict::Pass);
        assert_eq!(must(parse_verdict("Verdict: REVISE\r\n")), Verdict::Revise);
        assert_eq!(must(parse_verdict("Verdict: BLOCKED")), Verdict::Blocked);
        assert!(parse_verdict("Verdict: pass").is_err());
    }

    #[test]
    fn accepts_each_structurally_valid_verdict() {
        for verdict in [Verdict::Pass, Verdict::Revise, Verdict::Blocked] {
            let (_temporary, project) = fixture();
            install_permitted_outputs(&project, verdict);
            assert_eq!(must(check(&project)), verdict);
        }
    }

    #[test]
    fn rejects_a_missing_anchor_and_trailing_whitespace() {
        let (_temporary, project) = fixture();
        install_permitted_outputs(&project, Verdict::Pass);
        must(fs::write(
            project.output_path(STAGES[2]),
            "# Draft \n\n[Read more](../01-stories/output.md#missing)",
        ));
        assert!(check(&project).is_err());
    }
}
