//! The model may replace Jackson's bullet text; all other resume bytes are fixed.
//!
//! Templates are private runtime inputs. Never include a real resume in the crate.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const JACKSON: &str = "{Jackson National Life}";
const LIST_START: &str = "\\resumeItemListStart";
const LIST_END: &str = "\\resumeItemListEnd";
const ITEM: &str = "\\resumeItem{";

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
    pub source_path: PathBuf,
    pub pdf_path: PathBuf,
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

    /// Compile in private temporary space. Publish artifacts only after checks pass.
    /// Tectonic and Python 3 with pypdf must be available on PATH.
    ///
    /// # Errors
    /// Returns input, filesystem, compiler, overflow, page count and text-check
    /// failures. Each external command has a two-minute execution timeout.
    pub fn render_pdf(&self, bullets: &[String], output_dir: &Path) -> Result<RenderedResume> {
        let source = self.render_latex(bullets)?;
        create_private_dir(output_dir)?;
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
        let mut compiler = Command::new("tectonic");
        compiler
            .args(["--untrusted", "--keep-logs", "--outdir", ".", "render.tex"])
            .current_dir(build.path());
        run_checked(&mut compiler, build.path(), "tectonic")?;
        let log = fs::read_to_string(build.path().join("render.log"))?;
        ensure!(
            !log.contains("Overfull \\hbox") && !log.contains("Overfull \\vbox"),
            "resume text overflows the fixed template; shorten Jackson bullets and render again"
        );
        ensure!(
            !log.contains("Missing character:"),
            "resume contains characters the fixed template cannot render; revise Jackson text"
        );

        let mut inspect = Command::new("python3");
        inspect
            .args([
                "-c",
                "import json, sys; from pypdf import PdfReader; r = PdfReader(sys.argv[1]); print(json.dumps({'pages': len(r.pages), 'text': '\\n'.join(p.extract_text() or '' for p in r.pages)}))",
                "render.pdf",
            ])
            .current_dir(build.path());
        let inspection = run_checked(&mut inspect, build.path(), "pdf-inspection")?;
        let PdfInspection { pages, text } =
            serde_json::from_str(&inspection).context("read PDF inspection result")?;
        ensure!(
            pages == 1,
            "resume has {pages} pages; shorten Jackson bullets to preserve the original one-page layout"
        );
        ensure!(
            !text.trim().is_empty(),
            "resume PDF has no extractable text"
        );
        ensure!(
            text.contains("Jackson National Life"),
            "resume PDF is missing the Jackson heading"
        );

        let source_path = output_dir.join("resume.tex");
        let pdf_path = output_dir.join("resume.pdf");
        fs::rename(build.path().join("resume.tex"), &source_path)?;
        fs::rename(build.path().join("render.pdf"), &pdf_path)?;
        Ok(RenderedResume {
            source_path,
            pdf_path,
            pages,
        })
    }
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
}
