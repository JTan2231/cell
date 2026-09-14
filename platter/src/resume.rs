//! Render plain Jackson bullets and, for new runs, the complete projects section.
//!
//! Templates are private runtime inputs. Never include a real resume in the crate.

use crate::projects::ProjectBullets;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::ops::Range;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const JACKSON: &str = "{Jackson National Life}";
const LIST_START: &str = "\\resumeItemListStart";
const LIST_END: &str = "\\resumeItemListEnd";
const ITEM: &str = "\\resumeItem{";
const PROJECTS_BEGIN: &str = "% PLATTER PROJECTS BEGIN";
const PROJECTS_END: &str = "% PLATTER PROJECTS END";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub name: String,
    pub description: String,
    pub dates: Option<String>,
    pub bullets: Vec<String>,
    /// Private, current source pointers for editorial review.
    pub sources: Vec<String>,
}

fn project_url(name: &str) -> Result<&'static str> {
    match name {
        "Cell" => Ok("https://github.com/jtan2231/cell"),
        "Wrought" => Ok("https://wrought.experimental.joeytan.dev"),
        _ => bail!("unknown project name"),
    }
}

pub(crate) fn validate_projects(projects: &[Project]) -> Result<()> {
    ensure!(
        (1..=2).contains(&projects.len()),
        "select one or two projects"
    );
    let mut names = std::collections::BTreeSet::new();
    for project in projects {
        project_url(&project.name)?;
        ensure!(names.insert(&project.name), "duplicate project");
        ensure!(
            (1..=8).contains(&project.bullets.len()),
            "project needs one to eight bullets"
        );
        ensure!(
            !project.sources.is_empty(),
            "project source notes are required"
        );
        for text in std::iter::once(&project.description)
            .chain(project.dates.iter())
            .chain(&project.bullets)
            .chain(&project.sources)
        {
            ensure!(
                !text.trim().is_empty()
                    && !text.chars().any(char::is_control)
                    && text.chars().count() <= 1000,
                "project text must be a nonempty plain paragraph of at most 1000 characters"
            );
        }
        ensure!(
            project
                .bullets
                .iter()
                .all(|text| !text.starts_with(['•', '*', '-'])),
            "project bullets must omit bullet markers"
        );
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct RendererFailure;

impl std::fmt::Display for RendererFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "resume renderer failed; repair its environment before starting fresh preparation",
        )
    }
}

impl std::error::Error for RendererFailure {}

#[derive(Debug, thiserror::Error)]
#[error("resume content does not fit the fixed template")]
pub(crate) struct LayoutFailure;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeTemplate {
    /// Canonical local path, or an exact repository/commit/path source locator.
    pub source_path: String,
    pub source_sha256: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedResume {
    pub source: String,
    pub pdf: Vec<u8>,
    pub pages: usize,
}

#[derive(Deserialize)]
struct PdfInspection {
    pages: usize,
    text: String,
}

impl ResumeTemplate {
    /// Capture an immutable copy of the designated original source.
    ///
    /// # Errors
    /// Rejects a missing origin or an ambiguous or malformed Jackson bullet list.
    pub fn from_source(source_path: String, source: String) -> Result<Self> {
        ensure!(
            !source_path.trim().is_empty(),
            "resume source origin is required"
        );
        jackson_range(&source)?;
        Ok(Self {
            source_path,
            source_sha256: digest(source.as_bytes()),
            source,
        })
    }

    /// Read and capture a private UTF-8 LaTeX source file.
    ///
    /// # Errors
    /// Returns filesystem errors and the validation errors from `from_source`.
    pub fn load(path: &Path) -> Result<Self> {
        let path = path
            .canonicalize()
            .context("locate original resume source")?;
        let source = fs::read_to_string(&path).context("read original resume source")?;
        Self::from_source(path.to_string_lossy().into_owned(), source)
    }

    /// Retained inputs must still match their recorded digest before use.
    ///
    /// # Errors
    /// Rejects a changed digest, missing origin, or invalid Jackson bullet list.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            digest(self.source.as_bytes()) == self.source_sha256,
            "resume template no longer matches its captured SHA-256"
        );
        ensure!(
            !self.source_path.trim().is_empty(),
            "resume source origin is required"
        );
        jackson_range(&self.source)?;
        Ok(())
    }

    /// Replace only Jackson bullets, escaping all model text as literal LaTeX text.
    ///
    /// # Errors
    /// Rejects invalid templates and empty or multiline model bullets.
    pub fn render_latex(&self, bullets: &[String]) -> Result<String> {
        self.validate()?;
        ensure!(
            !bullets.is_empty(),
            "at least one Jackson bullet is required"
        );
        let range = jackson_range(&self.source)?;
        let mut rendered = self.source[..range.start].to_owned();
        for (index, bullet) in bullets.iter().enumerate() {
            let bullet = bullet.trim();
            ensure!(!bullet.is_empty(), "Jackson bullet {} is empty", index + 1);
            ensure!(
                !bullet.chars().any(char::is_control),
                "Jackson bullet {} must be one plain-text paragraph without control characters",
                index + 1
            );
            if index > 0 {
                rendered.push('\n');
            }
            rendered.push_str(ITEM);
            rendered.push_str(&escape_latex(bullet));
            rendered.push('}');
        }
        rendered.push_str(&self.source[range.end..]);
        // This check also guards future changes to this deliberately narrow renderer.
        self.validate_fixed_content(&rendered)?;
        Ok(rendered)
    }

    /// Verify byte equality everywhere outside the replaceable Jackson bullet span.
    ///
    /// # Errors
    /// Rejects malformed input or any edit outside the permitted span.
    pub fn validate_fixed_content(&self, rendered: &str) -> Result<()> {
        self.validate()?;
        let original = jackson_range(&self.source)?;
        let candidate = jackson_range(rendered)?;
        ensure!(
            self.source[..original.start] == rendered[..candidate.start]
                && self.source[original.end..] == rendered[candidate.end..],
            "resume changes outside Jackson bullet points are forbidden"
        );
        Ok(())
    }

    /// Check the supported projects section or explicit project markers.
    ///
    /// # Errors
    /// Rejects a missing, ambiguous, reversed or overlapping region.
    pub fn validate_projects_region(&self) -> Result<()> {
        self.validate()?;
        let projects = projects_range(&self.source)?;
        let jackson = jackson_range(&self.source)?;
        ensure!(
            projects.end <= jackson.start || jackson.end <= projects.start,
            "projects and Jackson regions overlap"
        );
        Ok(())
    }

    /// Render both editable regions. None preserves the legacy Jackson-only path.
    ///
    /// # Errors
    /// Rejects invalid project text, unsupported templates or changed fixed bytes.
    pub fn render_latex_with_projects(
        &self,
        bullets: &[String],
        projects: Option<&[Project]>,
    ) -> Result<String> {
        let mut rendered = self.render_latex(bullets)?;
        let Some(projects) = projects else {
            return Ok(rendered);
        };
        self.validate_projects_region()?;
        validate_projects(projects)?;
        let range = projects_range(&rendered)?;
        let mut content = String::new();
        for project in projects {
            content.push_str("\n\\par\\noindent\\textbf{\\href{");
            content.push_str(project_url(&project.name)?);
            content.push_str("}{");
            content.push_str(&escape_latex(&project.name));
            content.push_str("}}");
            if let Some(dates) = &project.dates {
                content.push_str("\\hfill ");
                content.push_str(&escape_latex(dates));
            }
            content.push_str("\\par\n\\noindent\\emph{");
            content.push_str(&escape_latex(&project.description));
            content.push_str("}\n");
            content.push_str(LIST_START);
            for bullet in &project.bullets {
                content.push('\n');
                content.push_str(ITEM);
                content.push_str(&escape_latex(bullet));
                content.push('}');
            }
            content.push('\n');
            content.push_str(LIST_END);
            content.push('\n');
        }
        rendered.replace_range(range, &content);
        self.validate_fixed_regions(&rendered)?;
        Ok(rendered)
    }

    fn validate_fixed_regions(&self, rendered: &str) -> Result<()> {
        let mut original = [jackson_range(&self.source)?, projects_range(&self.source)?];
        let mut candidate = [jackson_range(rendered)?, projects_range(rendered)?];
        original.sort_by_key(|range| range.start);
        candidate.sort_by_key(|range| range.start);
        let (mut left, mut right) = (0, 0);
        for (a, b) in original.iter().zip(&candidate) {
            ensure!(
                self.source[left..a.start] == rendered[right..b.start],
                "resume changes outside Jackson and projects are forbidden"
            );
            left = a.end;
            right = b.end;
        }
        ensure!(
            self.source[left..] == rendered[right..],
            "resume changes outside Jackson and projects are forbidden"
        );
        Ok(())
    }

    /// Compile in disposable private space and return validated bytes.
    /// Tectonic and Python 3 with pypdf use the same explicit resolution as doctor.
    ///
    /// # Errors
    /// Returns input, filesystem, compiler, overflow, page count and text-check
    /// failures. Each external command has a two-minute execution timeout.
    pub fn render_pdf(&self, bullets: &[String], output_dir: &Path) -> Result<RenderedResume> {
        self.render_pdf_with_projects(bullets, None, output_dir)
    }

    /// Render the selected project entries with the same PDF and layout checks.
    ///
    /// # Errors
    /// Returns content, template, compiler, extraction and one-page layout errors.
    pub fn render_pdf_with_projects(
        &self,
        bullets: &[String],
        projects: Option<&[Project]>,
        output_dir: &Path,
    ) -> Result<RenderedResume> {
        let source = self.render_latex_with_projects(bullets, projects)?;
        let expected = projects
            .into_iter()
            .flatten()
            .flat_map(|project| {
                std::iter::once(project.name.clone())
                    .chain(std::iter::once(project.description.clone()))
                    .chain(project.dates.clone())
                    .chain(project.bullets.clone())
            })
            .collect::<Vec<_>>();
        Self::compile(source, &expected, output_dir)
    }

    pub fn render_latex_fixed(
        &self,
        jackson: Option<&[String]>,
        projects: &ProjectBullets,
    ) -> Result<String> {
        projects.validate()?;
        self.validate_fixed_projects()?;
        let mut source = if let Some(bullets) = jackson {
            self.render_latex(bullets)?
        } else {
            self.source.clone()
        };
        for (name, bullets) in [("CELL", &projects.cell), ("WROUGHT", &projects.wrought)] {
            let range = fixed_project_range(&source, name)?;
            let mut replacement = String::new();
            for bullet in bullets {
                replacement.push_str("\n\\resumeItem{");
                replacement.push_str(&escape_latex(bullet));
                replacement.push('}');
            }
            replacement.push('\n');
            source.replace_range(range, &replacement);
        }
        self.validate_fixed_project_bytes(&source)?;
        Ok(source)
    }

    pub fn render_pdf_fixed(
        &self,
        jackson: Option<&[String]>,
        projects: &ProjectBullets,
        output_dir: &Path,
    ) -> Result<RenderedResume> {
        let source = self.render_latex_fixed(jackson, projects)?;
        let expected = ["Cell".into(), "Wrought".into()]
            .into_iter()
            .chain(projects.cell.clone())
            .chain(projects.wrought.clone())
            .collect::<Vec<_>>();
        Self::compile(source, &expected, output_dir)
    }

    pub fn validate_fixed_projects(&self) -> Result<()> {
        self.validate_projects_region()?;
        let section = projects_range(&self.source)?;
        let cell = fixed_project_range(&self.source, "CELL")?;
        let wrought = fixed_project_range(&self.source, "WROUGHT")?;
        ensure!(
            cell.end <= wrought.start || wrought.end <= cell.start,
            "project bullet regions overlap"
        );
        for range in [cell, wrought] {
            ensure!(
                section.start <= range.start && range.end <= section.end,
                "project bullet region is outside Projects"
            );
        }
        Ok(())
    }

    fn validate_fixed_project_bytes(&self, source: &str) -> Result<()> {
        let spans = |text: &str| -> Result<Vec<Range<usize>>> {
            let mut ranges = vec![
                jackson_range(text)?,
                fixed_project_range(text, "CELL")?,
                fixed_project_range(text, "WROUGHT")?,
            ];
            ranges.sort_by_key(|r| r.start);
            Ok(ranges)
        };
        let (mut left, mut right) = (0, 0);
        for (a, b) in spans(&self.source)?.iter().zip(spans(source)?) {
            ensure!(
                self.source[left..a.start] == source[right..b.start],
                "fixed resume content changed"
            );
            left = a.end;
            right = b.end;
        }
        ensure!(
            self.source[left..] == source[right..],
            "fixed resume content changed"
        );
        Ok(())
    }

    pub fn validate_project_template_import(&self, candidate: &Self) -> Result<()> {
        candidate.validate_fixed_projects()?;
        let old = projects_range(&self.source)?;
        let new = projects_range(&candidate.source)?;
        ensure!(
            self.source[..old.start] == candidate.source[..new.start]
                && self.source[old.end..] == candidate.source[new.end..],
            "template import may change only the projects section"
        );
        Ok(())
    }

    fn compile(source: String, expected: &[String], output_dir: &Path) -> Result<RenderedResume> {
        create_private_dir(output_dir)?;
        // The caller holds Platter's database admission lock. Recover abandoned
        // renderer work from an interrupted invocation before creating new work.
        for entry in fs::read_dir(output_dir)? {
            let entry = entry?;
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(".resume-build-")
            {
                ensure!(
                    entry.file_type()?.is_dir() && !entry.file_type()?.is_symlink(),
                    "unexpected renderer scratch entry"
                );
                fs::remove_dir_all(entry.path())?;
            }
        }
        let build = tempfile::Builder::new()
            .prefix(".resume-build-")
            .tempdir_in(output_dir)
            .context("create private resume build directory")?;
        fs::write(build.path().join("resume.tex"), &source)?;
        // The original was authored for pdfTeX. Tectonic's XeTeX engine already
        // emits Unicode mappings; these pdfTeX-only controls are compatibility
        // no-ops in a separate wrapper, never edits to the captured template.
        fs::write(
            build.path().join("render.tex"),
            "\\ifdefined\\pdfgentounicode\\else\\newcount\\pdfgentounicode\\fi\n\
             \\ifdefined\\pdfglyphtounicode\\else\\def\\pdfglyphtounicode#1#2{}\\fi\n\
             \\input{resume.tex}\n",
        )?;
        let mut compiler =
            Command::new(crate::readiness::renderer("tectonic").context(RendererFailure)?);
        compiler
            .args(["--untrusted", "--keep-logs", "--outdir", ".", "render.tex"])
            .current_dir(build.path());
        isolate_environment(&mut compiler, build.path())?;
        compiler.env("HOME", build.path().join("cache"));
        run_checked(&mut compiler, build.path(), "tectonic").context(RendererFailure)?;
        let log = fs::read_to_string(build.path().join("render.log"))?;
        (|| -> Result<()> {
        ensure!(
            !log.contains("Overfull \\hbox") && !log.contains("Overfull \\vbox"),
            "resume text overflows the fixed template; shorten the editable resume content and render again"
        );
        ensure!(
            !log.contains("Missing character:"),
            "resume contains characters the fixed template cannot render; revise the editable text"
        );

        Ok(())
        })().context(LayoutFailure)?;

        let mut inspect =
            Command::new(crate::readiness::renderer("python3").context(RendererFailure)?);
        inspect
            .args([
                "-c",
                "import json, sys; from pypdf import PdfReader; r = PdfReader(sys.argv[1]); print(json.dumps({'pages': len(r.pages), 'text': '\\n'.join(p.extract_text() or '' for p in r.pages)}))",
                "render.pdf",
            ])
            .current_dir(build.path());
        isolate_environment(&mut inspect, build.path())?;
        // Preserve Python's user-package lookup, as in doctor. Temporary files
        // remain private and Python bytecode writes stay disabled.
        let inspection =
            run_checked(&mut inspect, build.path(), "pdf-inspection").context(RendererFailure)?;
        let PdfInspection { pages, text } =
            serde_json::from_str(&inspection).context("read PDF inspection result")?;
        (|| -> Result<()> {
        ensure!(
            pages == 1,
            "resume has {pages} pages; shorten the editable content to preserve the original one-page layout"
        );
        ensure!(
            !text.trim().is_empty(),
            "resume PDF has no extractable text"
        );
        ensure!(
            text.contains("Jackson National Life"),
            "resume PDF is missing the Jackson heading"
        );

        let compact = |value: &str| value.chars().filter(|ch| !ch.is_whitespace()).collect::<String>();
        let extracted = compact(&text);
        for expected in expected {
            ensure!(extracted.contains(&compact(expected)), "resume PDF is missing project text; revise unsupported characters or layout");
        }
        Ok(())
        })().context(LayoutFailure)?;
        let pdf = fs::read(build.path().join("render.pdf"))?;
        Ok(RenderedResume { source, pdf, pages })
    }
}

fn fixed_project_range(source: &str, name: &str) -> Result<Range<usize>> {
    let begin = format!("% PLATTER {name} BULLETS BEGIN");
    let end = format!("% PLATTER {name} BULLETS END");
    ensure!(
        source.matches(&begin).count() == 1 && source.matches(&end).count() == 1,
        "template needs exactly one {name} bullet region"
    );
    let start = source.find(&begin).context("project start missing")? + begin.len();
    let end = source.find(&end).context("project end missing")?;
    ensure!(start < end, "project bullet markers are reversed");
    Ok(start..end)
}

fn projects_range(source: &str) -> Result<Range<usize>> {
    if source.contains(PROJECTS_BEGIN) || source.contains(PROJECTS_END) {
        ensure!(
            source.matches(PROJECTS_BEGIN).count() == 1
                && source.matches(PROJECTS_END).count() == 1,
            "projects markers must occur exactly once"
        );
        let start = source
            .find(PROJECTS_BEGIN)
            .context("projects start missing")?
            + PROJECTS_BEGIN.len();
        let end = source.find(PROJECTS_END).context("projects end missing")?;
        ensure!(start < end, "projects markers are reversed");
        return Ok(start..end);
    }
    let headings = ["\\section{Projects}", "\\section{Side Projects}"];
    let matches: Vec<_> = headings
        .iter()
        .flat_map(|heading| {
            source
                .match_indices(heading)
                .map(|(offset, text)| offset + text.len())
        })
        .collect();
    ensure!(
        matches.len() == 1,
        "template needs one Projects or Side Projects section, or explicit PLATTER PROJECTS markers"
    );
    let start = matches[0];
    let rest = &source[start..];
    let end = rest
        .find("\\section{")
        .or_else(|| rest.find("\\end{document}"))
        .context("projects section has no end boundary")?;
    Ok(start..start + end)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn jackson_range(source: &str) -> Result<Range<usize>> {
    ensure!(
        source.matches(JACKSON).count() == 1,
        "resume template must have exactly one Jackson National Life heading"
    );
    let heading = source.find(JACKSON).context("Jackson heading is missing")?;
    let after_heading = &source[heading + JACKSON.len()..];
    let list_relative = after_heading
        .find(LIST_START)
        .context("Jackson bullet list is missing")?;
    let before_list = &after_heading[..list_relative];
    ensure!(
        !before_list.contains("\\resumeSubheading") && !before_list.contains("\\section"),
        "Jackson heading has no directly associated bullet list"
    );
    let list_start = heading + JACKSON.len() + list_relative + LIST_START.len();
    let list_end = list_start
        + source[list_start..]
            .find(LIST_END)
            .context("Jackson bullet list is unterminated")?;
    let list = &source[list_start..list_end];
    ensure!(
        !list.contains(LIST_START),
        "nested Jackson bullet lists are unsupported"
    );
    let leading = list.len() - list.trim_start().len();
    let mut cursor = leading;
    let mut final_item_boundary = None;
    while cursor < list.len() {
        let remaining = &list[cursor..];
        if remaining.trim().is_empty() {
            break;
        }
        ensure!(
            remaining.starts_with(ITEM),
            "Jackson list must contain only resumeItem commands"
        );
        cursor += ITEM.len();
        let mut depth = 1;
        let mut escaped = false;
        for (offset, ch) in list[cursor..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        cursor += offset + 1;
                        final_item_boundary = Some(cursor);
                        break;
                    }
                }
                _ => {}
            }
        }
        ensure!(depth == 0, "Jackson resumeItem has unbalanced braces");
        cursor += list[cursor..].len() - list[cursor..].trim_start().len();
    }
    let end = final_item_boundary.context("Jackson list must have at least one original bullet")?;
    Ok(list_start + leading..list_start + end)
}

#[must_use]
pub fn escape_latex(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => result.push_str("\\textbackslash{}"),
            '{' => result.push_str("\\{"),
            '}' => result.push_str("\\}"),
            '$' => result.push_str("\\$"),
            '&' => result.push_str("\\&"),
            '#' => result.push_str("\\#"),
            '%' => result.push_str("\\%"),
            '_' => result.push_str("\\_"),
            '~' => result.push_str("\\textasciitilde{}"),
            '^' => result.push_str("\\textasciicircum{}"),
            _ => result.push(ch),
        }
    }
    result
}

fn create_private_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .context("create private resume output directory")?;
    ensure!(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "resume output directory must not be a symlink"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn isolate_environment(command: &mut Command, directory: &Path) -> Result<()> {
    let cache = directory.join("cache");
    create_private_dir(&cache)?;
    for name in [
        "TMPDIR",
        "TMP",
        "TEMP",
        "XDG_CACHE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "TECTONIC_CACHE_DIR",
        "TEXMFOUTPUT",
        "TEXMFVAR",
        "TEXMFCONFIG",
    ] {
        command.env(name, &cache);
    }
    command.env("PYTHONDONTWRITEBYTECODE", "1");
    Ok(())
}

fn run_checked(command: &mut Command, directory: &Path, label: &str) -> Result<String> {
    let stdout_path = directory.join(format!("{label}.stdout"));
    let stderr_path = directory.join(format!("{label}.stderr"));
    command.stdout(Stdio::from(fs::File::create(&stdout_path)?));
    command.stderr(Stdio::from(fs::File::create(&stderr_path)?));
    let mut child = command
        .spawn()
        .with_context(|| format!("start required resume tool {label}"))?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("resume tool {label} exceeded its 120-second execution timeout");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stdout = fs::read_to_string(stdout_path)?;
    if !status.success() {
        let stderr = fs::read_to_string(stderr_path)?;
        // Return only a bounded diagnostic; compiler output can contain private text.
        let diagnostic = stderr
            .chars()
            .rev()
            .take(1500)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        bail!("resume tool {label} failed ({status}): {diagnostic}");
    }
    Ok(stdout)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixtures should fail immediately when malformed.
mod tests {
    use super::*;

    const FIXTURE: &str = "\\documentclass{article}\n\\begin{document}\nFixed identity and education\n\\resumeSubheading{Fixed title}{Fixed dates}{Jackson National Life}{Fixed location}\n  \\resumeItemListStart\n    \\resumeItem{Original {nested} bullet with C\\# and 40\\%}\n    \\resumeItem{Second original bullet}\n  \\resumeItemListEnd\n\\resumeSubheading{Other employer}{dates}{role}{place}\n\\resumeItemListStart\n\\resumeItem{Fixed other employer bullet}\n\\resumeItemListEnd\nFixed projects and skills\n\\end{document}\n";

    fn template() -> ResumeTemplate {
        ResumeTemplate::from_source("synthetic fixture".into(), FIXTURE.into()).unwrap()
    }

    #[test]
    fn replaces_only_jackson_bullets_and_preserves_fixed_bytes() {
        let template = template();
        let rendered = template.render_latex(&["New work".into()]).unwrap();
        assert_eq!(
            rendered,
            FIXTURE.replace(
                "\\resumeItem{Original {nested} bullet with C\\# and 40\\%}\n    \\resumeItem{Second original bullet}",
                "\\resumeItem{New work}"
            )
        );
        template.validate_fixed_content(&rendered).unwrap();
        assert!(
            template
                .validate_fixed_content(&rendered.replace("Fixed dates", "new dates"))
                .is_err()
        );
        assert!(
            template
                .validate_fixed_content(
                    &rendered.replace("Fixed other employer bullet", "authored elsewhere")
                )
                .is_err()
        );
    }

    #[test]
    fn model_text_cannot_execute_latex_or_escape_the_jackson_list() {
        let text = r"C# & 40% $ _ { } ~ ^ \input{/etc/passwd} \resumeItemListEnd";
        let escaped = escape_latex(text);
        assert_eq!(
            escaped,
            r"C\# \& 40\% \$ \_ \{ \} \textasciitilde{} \textasciicircum{} \textbackslash{}input\{/etc/passwd\} \textbackslash{}resumeItemListEnd"
        );
        template().render_latex(&[text.into()]).unwrap();
    }

    #[test]
    fn rejects_tampered_snapshot_and_ambiguous_or_malformed_template() {
        let mut changed = template();
        changed.source.push(' ');
        assert!(changed.render_latex(&["text".into()]).is_err());
        for source in [
            FIXTURE.replace(JACKSON, "Other employer"),
            FIXTURE.replace(JACKSON, &format!("{JACKSON}{JACKSON}")),
            FIXTURE.replace("Original {nested}", "Original {unclosed"),
            FIXTURE.replace("\\resumeItem{Second original bullet}", "\\input{evil}"),
        ] {
            assert!(ResumeTemplate::from_source("fixture".into(), source).is_err());
        }
    }

    #[test]
    fn rejects_empty_or_multiline_model_bullets() {
        for bullets in [
            vec![],
            vec![" ".into()],
            vec!["first\nsecond".into()],
            vec!["null\0byte".into()],
        ] {
            assert!(template().render_latex(&bullets).is_err());
        }
    }

    #[test]
    fn projects_replace_only_their_region_and_keep_sources_private() {
        let source = FIXTURE.replace(
            "Fixed projects and skills",
            "\\section{Projects}\nOld project\n\\section{Skills}\nFixed skills",
        );
        let template = ResumeTemplate::from_source("fixture".into(), source.clone()).unwrap();
        let projects = vec![
            Project {
                name: "Cell".into(),
                description: "Rust & SQLite".into(),
                dates: None,
                bullets: vec![r"Built recovery; 40% \input{untrusted}".into()],
                sources: vec!["private/source.rs:42".into()],
            },
            Project {
                name: "Wrought".into(),
                description: "Go and React".into(),
                dates: Some("2026".into()),
                bullets: vec!["Built a world editor".into()],
                sources: vec!["Annals work label".into()],
            },
        ];
        let rendered = template
            .render_latex_with_projects(&["Jackson work".into()], Some(&projects))
            .unwrap();
        assert!(rendered.contains("https://github.com/jtan2231/cell"));
        assert!(rendered.contains("https://wrought.experimental.joeytan.dev"));
        assert!(rendered.contains(r"40\% \textbackslash{}input\{untrusted\}"));
        assert!(!rendered.contains("private/source.rs") && !rendered.contains("Annals work label"));
        assert!(!rendered.contains("Old project"));
        assert!(rendered.contains("Fixed other employer bullet"));
        template.validate_fixed_regions(&rendered).unwrap();
        assert!(
            template
                .validate_fixed_regions(&rendered.replace("Fixed skills", "Changed skills"))
                .is_err()
        );
        assert!(template.validate_fixed_content(&rendered).is_err());
        assert!(
            template
                .render_latex_with_projects(&["Jackson work".into()], None)
                .unwrap()
                .contains("Old project")
        );

        let marked = source.replace(
            "\\section{Projects}\nOld project\n",
            "% PLATTER PROJECTS BEGIN\nOld project\n% PLATTER PROJECTS END\n",
        );
        let marked = ResumeTemplate::from_source("fixture".into(), marked).unwrap();
        marked
            .render_latex_with_projects(&["Jackson work".into()], Some(&projects))
            .unwrap();
        assert!(self::template().validate_projects_region().is_err());
        assert!(validate_projects(&[projects[0].clone(), projects[0].clone()]).is_err());
    }
    #[test]
    fn fixed_project_bullets_preserve_headers_technologies_and_other_bytes() {
        let base = template();
        let section = r"\section{Projects}
Fixed Cell header | Rust
\resumeItemListStart
% PLATTER CELL BULLETS BEGIN
\resumeItem{Old Cell}
% PLATTER CELL BULLETS END
\resumeItemListEnd
Fixed Wrought header | Go
\resumeItemListStart
% PLATTER WROUGHT BULLETS BEGIN
\resumeItem{Old Wrought}
% PLATTER WROUGHT BULLETS END
\resumeItemListEnd
\section{Skills}
Fixed skills";
        let original = ResumeTemplate::from_source(
            "fixture".into(),
            base.source.replace(
                "Fixed projects and skills",
                "\\section{Projects}\nOld projects\n\\section{Skills}\nFixed skills",
            ),
        )
        .unwrap();
        let candidate = ResumeTemplate::from_source(
            "fixture".into(),
            base.source.replace("Fixed projects and skills", section),
        )
        .unwrap();
        original
            .validate_project_template_import(&candidate)
            .unwrap();
        let bullets = ProjectBullets {
            cell: vec!["Built C# & Rust".into()],
            wrought: vec!["Preserved player choice".into()],
        };
        let rendered = candidate
            .render_latex_fixed(Some(&["Jackson contribution".into()]), &bullets)
            .unwrap();
        assert!(rendered.contains("Fixed Cell header | Rust"));
        assert!(rendered.contains("Fixed Wrought header | Go"));
        assert!(rendered.contains(r"Built C\# \& Rust"));
        assert!(
            candidate
                .validate_fixed_project_bytes(
                    &rendered.replace("Fixed Cell header", "Changed header")
                )
                .is_err()
        );
        let mut invalid = candidate.clone();
        invalid.source = invalid.source.replace("Fixed identity", "Changed identity");
        invalid.source_sha256 = digest(invalid.source.as_bytes());
        assert!(original.validate_project_template_import(&invalid).is_err());
    }
}
