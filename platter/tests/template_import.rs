use anyhow::{Context, Result};
use platter::{resume::ResumeTemplate, store::Store, workflow};
use serde_json::json;
use std::fs;

const SOURCE: &str = r"Fixed header
{Jackson National Life}
\resumeItemListStart
\resumeItem{Existing Jackson work}
\resumeItemListEnd
\section{Projects}
Cell
% PLATTER CELL BULLETS BEGIN
\resumeItem{Existing Cell work}
% PLATTER CELL BULLETS END
Wrought
% PLATTER WROUGHT BULLETS BEGIN
\resumeItem{Existing Wrought work}
% PLATTER WROUGHT BULLETS END
\end{document}";

#[test]
fn full_import_preserves_prior_template_and_settings_and_rejects_invalid_regions() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let store = Store::open(root)?;
    let original = store.put_artifact(
        None,
        "template",
        "original.tex",
        "application/x-tex",
        SOURCE.as_bytes(),
    )?;
    store.set_setting("template", &original.id)?;
    let config = json!({"sentinel":"unchanged configuration"});
    store.set_setting("config", &config)?;
    let candidate = SOURCE.replace("Fixed header", "New typography and header");
    let path = root.join("candidate.tex");
    fs::write(&path, &candidate)?;

    assert!(workflow::import_projects_template(root, &path).is_err());
    workflow::import_template(root, &path)?;
    let selected: String = store.setting("template")?.context("selected template")?;
    assert_ne!(selected, original.id);
    assert_eq!(store.template()?.source, candidate);
    assert_eq!(store.template_artifact(&original.id)?.source, SOURCE);
    assert_eq!(store.setting::<serde_json::Value>("config")?, Some(config));

    for invalid in [
        candidate.replace("% PLATTER WROUGHT BULLETS END", ""),
        candidate.replace("{Jackson National Life}", "{Other employer}"),
    ] {
        fs::write(&path, invalid)?;
        assert!(workflow::import_template(root, &path).is_err());
        assert_eq!(store.setting::<String>("template")?, Some(selected.clone()));
    }
    assert!(workflow::import_template(root, std::path::Path::new("relative.tex")).is_err());
    ResumeTemplate::from_source("old capture".into(), SOURCE.into())?.validate_fixed_projects()?;
    Ok(())
}
