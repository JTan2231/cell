//! The product-owned setup command invoked by a Cell manifest.
use super::{
    Install, lifecycle,
    support::{ACTIVE, Paths, binding, definition},
};
use cell_install::adapter::Context;
use cell_install::{Error, Result};
use serde_json::{Value, json};
use std::path::PathBuf;

pub fn deploy() -> Result<Value> {
    let context = Context::read(
        "krisis",
        "products/decisions",
        "krisis-install",
        env!("CARGO_PKG_VERSION"),
    )?;
    context.validate_settings(&["codex_bin"], &["enabled"])?;
    let paths = Paths::new(context.home.clone())?;
    let clockwork = context.home.join(".local/bin/clockwork");
    let settings = context.request.settings.as_ref();
    let configured = settings
        .and_then(|value| value.get("codex_bin"))
        .and_then(Value::as_str);
    let codex = if let Some(path) = configured {
        PathBuf::from(path)
    } else {
        let prior = binding(&paths, &clockwork, ACTIVE)?;
        if let Some(reference) = prior.definition_digest {
            definition(&paths, &clockwork, &reference)?
                .environment
                .get("CONVERSATIONS_CODEX")
                .map(PathBuf::from)
                .ok_or_else(|| Error::new("observer has no Codex pin"))?
        } else {
            PathBuf::from(conversations::DEFAULT_CODEX_PATH)
        }
    };
    let config = context
        .home
        .join("Library/Application Support/Annals/decisions/config.toml");
    let parsed: toml::Value = toml::from_str(&std::fs::read_to_string(&config)?)
        .map_err(|error| Error::new(error.to_string()))?;
    let library = parsed
        .get("decision_feed")
        .and_then(|value| value.get("expected_library_id"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| Error::new("Annals decisions configuration has no library reference"))?;
    let options = Install {
        binary: context.binary("krisis")?,
        source_root: Some(context.request.source_root.join("products/decisions")),
        home: Some(context.home.clone()),
        codex,
        annals: std::fs::canonicalize(
            context
                .home
                .join("Library/Application Support/Annals/install/current/libexec/annals"),
        )?,
        annals_config: config,
        annals_library_id: library.into(),
        clockwork,
        launchctl: "/bin/launchctl".into(),
        final_cutover: false,
        keep_maintenance: false,
        release_maintenance: false,
        expected_current: None,
    };
    let enabled = settings
        .and_then(|value| value.get("enabled"))
        .and_then(Value::as_bool);
    lifecycle::deploy(&options, enabled)?;
    Ok(json!({"installed":true}))
}
