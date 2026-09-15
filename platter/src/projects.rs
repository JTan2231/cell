//! Request project prose from Weaver; retain its results and fit fixed bullet regions.
use crate::{
    resume::{LayoutFailure, ResumeTemplate},
    store::Store,
};
use anyhow::{Context, Result, bail, ensure};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use weaver::api::{AuthoringOutcome, Client, DocumentView};

pub const CELL_DIRECTION: &str = "bullet points for cell. Editorial direction: Creator wants to convey how developing cell has accelerated and made easier the job search process. Write a maximum of 3 resume bullet points. Keep each bullet short in form, with one medium-length sentence. Return only the bullet list.";
pub const WROUGHT_DIRECTION: &str = "bullet points for wrought. Editorial direction: Creator wanted a shareable, dynamic tabletop experience to enjoy with his friends. Write a maximum of 3 resume bullet points. Keep each bullet short in form, with one medium-length sentence. Return only the bullet list.";
pub const SHORTEN_DIRECTION: &str = "Shorten these resume bullets to fit a one-page resume. Use at most 3 short bullets, one brief sentence each. Keep the strongest contributions and return only the bullet list.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectBullets {
    pub cell: Vec<String>,
    pub wrought: Vec<String>,
}
impl ProjectBullets {
    pub fn validate(&self) -> Result<()> {
        for bullets in [&self.cell, &self.wrought] {
            ensure!(
                (1..=3).contains(&bullets.len()),
                "each project requires one through three bullets"
            );
            for text in bullets {
                ensure!(
                    !text.trim().is_empty()
                        && text.chars().count() <= 1000
                        && !text.chars().any(char::is_control),
                    "project bullets must be nonempty plain paragraphs of at most 1000 characters"
                );
            }
        }
        Ok(())
    }
}

pub fn parse_bullets(markdown: &str) -> Result<Vec<String>> {
    let mut bullets = Vec::new();
    let mut current: Option<String> = None;
    let mut list = false;
    let mut seen_list = false;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::List(None)) if !list && !seen_list => {
                list = true;
                seen_list = true;
            }
            Event::End(TagEnd::List(false)) if list => {
                list = false;
            }
            Event::Start(Tag::Item) if list && current.is_none() => current = Some(String::new()),
            Event::End(TagEnd::Item) => {
                let text = current.take().context("unexpected bullet end")?;
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                ensure!(!text.is_empty(), "empty Weaver bullet");
                bullets.push(text);
            }
            Event::Start(Tag::Paragraph | Tag::Emphasis | Tag::Strong)
            | Event::End(TagEnd::Emphasis | TagEnd::Strong)
                if current.is_some() => {}
            Event::Text(text) | Event::Code(text) if current.is_some() => {
                current.as_mut().context("bullet missing")?.push_str(&text);
            }
            Event::End(TagEnd::Paragraph) | Event::SoftBreak | Event::HardBreak
                if current.is_some() =>
            {
                current.as_mut().context("bullet missing")?.push(' ');
            }
            _ => bail!("Weaver must return only a flat Markdown bullet list"),
        }
    }
    ensure!(
        !list && current.is_none() && (1..=3).contains(&bullets.len()),
        "Weaver must return one through three bullets"
    );
    Ok(bullets)
}

#[derive(Debug, thiserror::Error)]
#[error("Weaver authoring deferred: {0}")]
pub struct Deferred(pub String);

#[derive(Serialize, Deserialize)]
struct Assignment {
    id: String,
    direction: String,
    revise: Option<String>,
}

pub fn job_ids(store: &Store, run: &str) -> Result<Vec<nucleus_core::JobId>> {
    let mut ids = Vec::new();
    for name in ["cell", "wrought", "cell-short", "wrought-short"] {
        if let Some(value) = store.execution(run, &format!("weaver-{name}"))? {
            let assignment: Assignment = serde_json::from_value(value)?;
            ids.push(assignment.id.into());
        }
    }
    Ok(ids)
}

#[allow(clippy::too_many_lines)] // Keep exact assignment admission and result recovery together.
async fn author(
    store: &Store,
    run: &str,
    name: &str,
    direction: &str,
    revise: Option<&str>,
    client: &Client,
    deadline: Option<Instant>,
) -> Result<DocumentView> {
    let kind = format!("weaver-{name}");
    if let Some(saved) = store.content::<DocumentView>(run, &kind)? {
        ensure!(
            saved.error.is_none(),
            "Weaver saved output but execution failed: {:?}",
            saved.error
        );
        return Ok(saved);
    }
    let assignment = if let Some(value) = store.execution(run, &kind)? {
        let retained: Assignment = serde_json::from_value(value)?;
        ensure!(
            retained.direction == direction && retained.revise.as_deref() == revise,
            "project assignment input conflict"
        );
        retained
    } else {
        let value = Assignment {
            id: format!("weaver-{run}-{name}"),
            direction: direction.into(),
            revise: revise.map(str::to_owned),
        };
        store.save_execution(run, &kind, &value)?;
        value
    };
    let operation = async {
        if let Some(parent) = &assignment.revise {
            client
                .revise(&assignment.id, parent, &assignment.direction)
                .await
        } else {
            client.write(&assignment.id, &assignment.direction).await
        }
    };
    let outcome = if let Some(deadline) = deadline {
        if let Ok(result) = tokio::time::timeout(
            deadline.saturating_duration_since(Instant::now()),
            operation,
        )
        .await
        {
            result
        } else {
            let nucleus = nucleus_client::NucleusClient::for_current_user()?;
            let _ = tokio::time::timeout(
                Duration::from_secs(10),
                nucleus.cancel_job(&assignment.id.clone().into()),
            )
            .await;
            bail!(
                "project deadline reached; exact Weaver job cancellation requested; run retained"
            );
        }
    } else {
        operation.await
    };
    let document = match outcome {
        Ok(AuthoringOutcome::Document(document)) => document,
        Ok(AuthoringOutcome::Deferred {
            id,
            outcome,
            detail,
        }) => {
            ensure!(
                id == assignment.id && outcome == "quota_deferred",
                "unexpected Weaver deferral"
            );
            return Err(Deferred(detail).into());
        }
        Err(error) => {
            // Weaver may have committed Markdown before a later runtime failure.
            if let Ok(document) = client.show(&assignment.id).await
                && let Some(markdown) = &document.markdown
            {
                store.put_artifact(
                    Some(run),
                    &format!("{kind}-markdown"),
                    &format!("{kind}.md"),
                    "text/markdown",
                    markdown.as_bytes(),
                )?;
                if document.finished_at.is_some() {
                    store.put_content(run, &kind, &document)?;
                }
            }
            return Err(error);
        }
    };
    ensure!(
        document.id == assignment.id
            && document.nucleus_job_id == assignment.id
            && document.direction == direction,
        "Weaver returned another assignment"
    );
    ensure!(
        document.markdown.is_some(),
        "Weaver completed without a document"
    );
    store.put_content(run, &kind, &document)?;
    ensure!(
        document.error.is_none(),
        "Weaver execution failed: {:?}",
        document.error
    );
    Ok(document)
}

pub async fn prepare(
    store: &Store,
    run: &str,
    template: &ResumeTemplate,
    directions: &[String; 3],
    deadline: Option<Instant>,
) -> Result<ProjectBullets> {
    if let Some(saved) = store.content::<ProjectBullets>(run, "project-bullets")? {
        return Ok(saved);
    }
    template.validate_fixed_projects()?;
    let client = Client::new(crate::workflow::config(store.root())?.weaver_executable)?;
    let cell = author(store, run, "cell", &directions[0], None, &client, deadline).await?;
    let wrought = author(
        store,
        run,
        "wrought",
        &directions[1],
        None,
        &client,
        deadline,
    )
    .await?;
    let parse = |cell: &DocumentView, wrought: &DocumentView| -> Result<ProjectBullets> {
        let result = ProjectBullets {
            cell: parse_bullets(cell.markdown.as_deref().context("Cell output missing")?)?,
            wrought: parse_bullets(
                wrought
                    .markdown
                    .as_deref()
                    .context("Wrought output missing")?,
            )?,
        };
        result.validate()?;
        Ok(result)
    };
    let mut bullets = parse(&cell, &wrought)?;
    render_time(deadline)?;
    if let Err(error) = template.render_pdf_fixed(None, &bullets, store.root()) {
        if !error.is::<LayoutFailure>() {
            return Err(error);
        }
        let cell = author(
            store,
            run,
            "cell-short",
            &directions[2],
            Some(&cell.id),
            &client,
            deadline,
        )
        .await?;
        let wrought = author(
            store,
            run,
            "wrought-short",
            &directions[2],
            Some(&wrought.id),
            &client,
            deadline,
        )
        .await?;
        bullets = parse(&cell, &wrought)?;
        render_time(deadline)?;
        template.render_pdf_fixed(None, &bullets, store.root())?;
    }
    store.put_content(run, "project-bullets", &bullets)?;
    Ok(bullets)
}

fn render_time(deadline: Option<Instant>) -> Result<()> {
    ensure!(
        deadline.is_none_or(|d| d.saturating_duration_since(Instant::now()).as_secs() > 245),
        "not enough invocation time remains to render project content; run retained"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_conversion_preserves_words_and_limits_structure() -> Result<()> {
        assert_eq!(
            parse_bullets(
                "- Built **Cell** with `Rust`.\n  Kept state durable.\n- A second contribution."
            )?,
            vec![
                "Built Cell with Rust. Kept state durable.",
                "A second contribution."
            ]
        );
        for invalid in [
            "Which project?",
            "# Heading\n- Bullet",
            "- Outer\n  - Nested",
            "- One\n- Two\n- Three\n- Four",
            "- [Link](https://example.com)",
        ] {
            assert!(parse_bullets(invalid).is_err(), "{invalid}");
        }
        Ok(())
    }
    #[tokio::test]
    async fn deferral_and_response_loss_reuse_the_saved_weaver_identity() -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir()?;
        let store = Store::open(root.path())?;
        store.insert(&crate::store::PacketRecord {
            id: "packet-1".into(),
            opportunity: "role".into(),
            job_id: "job".into(),
            company: "Company".into(),
            title: "Role".into(),
            status: "preparing".into(),
            directory: String::new(),
        })?;
        let executable = root.path().join("weaver");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\n' \"$@\" > \"$0.args\"\n/bin/cat \"$0.response\"\n",
        )?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        let response = root.path().join("weaver.response");
        let client = Client::new(executable)?;
        let id = "weaver-packet-1-cell";
        std::fs::write(&response, serde_json::json!({"ok":true,"data":{"id":id,"outcome":"quota_deferred","detail":"paused"}}).to_string())?;
        let error = author(
            &store,
            "packet-1",
            "cell",
            CELL_DIRECTION,
            None,
            &client,
            None,
        )
        .await
        .err()
        .context("must defer")?;
        assert!(error.is::<Deferred>());
        assert_eq!(
            job_ids(&store, "packet-1")?,
            vec![nucleus_core::JobId::new(id)]
        );
        std::fs::write(&response, "incomplete response")?;
        assert!(
            author(
                &store,
                "packet-1",
                "cell",
                CELL_DIRECTION,
                None,
                &client,
                None
            )
            .await
            .is_err()
        );
        let document = DocumentView {
            id: id.into(),
            nucleus_job_id: id.into(),
            direction: CELL_DIRECTION.into(),
            created_at: 1,
            markdown: Some("- Built Cell".into()),
            finished_at: Some(2),
            error: None,
        };
        std::fs::write(
            &response,
            serde_json::json!({"ok":true,"data":document}).to_string(),
        )?;
        let saved = author(
            &store,
            "packet-1",
            "cell",
            CELL_DIRECTION,
            None,
            &client,
            None,
        )
        .await?;
        assert_eq!(saved.id, id);
        let args = std::fs::read_to_string(root.path().join("weaver.args"))?;
        assert!(args.contains(&format!("--id\n{id}\n")));
        std::fs::remove_file(&response)?;
        assert_eq!(
            author(
                &store,
                "packet-1",
                "cell",
                CELL_DIRECTION,
                None,
                &client,
                None
            )
            .await?
            .markdown,
            saved.markdown
        );
        Ok(())
    }
}
