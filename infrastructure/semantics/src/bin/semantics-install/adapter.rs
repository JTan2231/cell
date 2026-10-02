//! The product-owned setup command invoked by a Cell manifest.
use super::{Candidate, HomeArgs, Result, lifecycle};
use cell_install::adapter::Context;
use serde_json::{Value, json};

pub(super) fn deploy() -> Result<Value> {
    let context = Context::read(
        "semantics",
        "infrastructure/semantics",
        "semantics-install",
        env!("CARGO_PKG_VERSION"),
    )?;
    context.validate_settings(&[], &["enabled"])?;
    let enabled = context
        .request
        .settings
        .as_ref()
        .and_then(|value| value.get("enabled"))
        .and_then(Value::as_bool);
    let args = Candidate {
        binary: context.binary("semantics")?,
        bundle: context
            .request
            .source_root
            .join("infrastructure/semantics/chancery"),
        home: HomeArgs {
            home: context.home,
            clockwork: None,
            launchctl: "/bin/launchctl".into(),
        },
        expected_current: None,
        final_decisions_watermark: None,
        keep_maintenance: false,
    };
    lifecycle::deploy(&args, enabled)?;
    Ok(json!({"installed": true}))
}
