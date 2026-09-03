use std::fs::{self, File};
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tempfile::NamedTempFile;

use crate::error::{Context as _, Error, Result};
use crate::lock::KeyLock;
use crate::manifest;
use crate::model::{BindingRecord, Manifest, Schedule};
use crate::paths::{Layout, current_uid};
use crate::store::Store;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransitionJournal {
    schema_version: u32,
    key: String,
    operation: TransitionOperation,
    prior_binding: Option<JournalBinding>,
    prior_plist: Option<Vec<u8>>,
    prior_loaded: bool,
    candidate_definition_digest: Option<String>,
    target_definition_digest: Option<String>,
    candidate_plist: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransitionOperation {
    Switch,
    Disable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalBinding {
    key: String,
    definition_digest: Option<String>,
    enabled: bool,
    plist_sha256: Option<String>,
    updated_at: i64,
}

impl TransitionJournal {
    fn capture_switch(
        key: &str,
        binding: Option<&BindingRecord>,
        plist: Option<&[u8]>,
        loaded: bool,
        definition_digest: &str,
        candidate_plist: &[u8],
    ) -> Self {
        Self {
            schema_version: 1,
            key: key.to_owned(),
            operation: TransitionOperation::Switch,
            prior_binding: binding.map(JournalBinding::from),
            prior_plist: plist.map(ToOwned::to_owned),
            prior_loaded: loaded,
            candidate_definition_digest: Some(definition_digest.to_owned()),
            target_definition_digest: Some(definition_digest.to_owned()),
            candidate_plist: Some(candidate_plist.to_owned()),
        }
    }

    fn capture_disable(
        key: &str,
        binding: Option<&BindingRecord>,
        plist: Option<&[u8]>,
        loaded: bool,
        target_definition_digest: Option<&str>,
    ) -> Self {
        Self {
            schema_version: 1,
            key: key.to_owned(),
            operation: TransitionOperation::Disable,
            prior_binding: binding.map(JournalBinding::from),
            prior_plist: plist.map(ToOwned::to_owned),
            prior_loaded: loaded,
            candidate_definition_digest: None,
            target_definition_digest: target_definition_digest.map(ToOwned::to_owned),
            candidate_plist: None,
        }
    }

    fn with_disable_intent(&self, target_definition_digest: Option<String>) -> Self {
        let mut journal = self.clone();
        journal.operation = TransitionOperation::Disable;
        journal.target_definition_digest = target_definition_digest;
        journal
    }

    fn prior_binding(&self) -> Option<BindingRecord> {
        self.prior_binding.as_ref().map(JournalBinding::binding)
    }
}

impl From<&BindingRecord> for JournalBinding {
    fn from(binding: &BindingRecord) -> Self {
        Self {
            key: binding.key.clone(),
            definition_digest: binding.definition_digest.clone(),
            enabled: binding.enabled,
            plist_sha256: binding.plist_sha256.clone(),
            updated_at: binding.updated_at,
        }
    }
}

impl JournalBinding {
    fn binding(&self) -> BindingRecord {
        BindingRecord {
            key: self.key.clone(),
            definition_digest: self.definition_digest.clone(),
            enabled: self.enabled,
            plist_sha256: self.plist_sha256.clone(),
            updated_at: self.updated_at,
        }
    }
}

pub(crate) trait ServiceManager {
    fn is_loaded(&self, label: &str) -> Result<bool>;
    fn bootout(&self, label: &str) -> Result<()>;
    fn bootstrap(&self, plist: &Path) -> Result<()>;
}

pub(crate) struct SystemLaunchd {
    domain: String,
}

impl SystemLaunchd {
    pub(crate) fn discover() -> Result<Self> {
        let output = Command::new("/usr/bin/id")
            .arg("-u")
            .output()
            .context("launchd_unavailable", "run /usr/bin/id -u")?;
        if !output.status.success() {
            return Err(Error::new("launchd_unavailable", "/usr/bin/id -u failed"));
        }
        let uid = std::str::from_utf8(&output.stdout)
            .context("launchd_unavailable", "decode current uid")?
            .trim()
            .parse::<u32>()
            .context("launchd_unavailable", "parse current uid")?;
        Ok(Self {
            domain: format!("gui/{uid}"),
        })
    }

    fn run(arguments: &[&str], operation: &str) -> Result<()> {
        let output = Command::new("/bin/launchctl")
            .args(arguments)
            .output()
            .context("launchd_operation_failed", format!("launchctl {operation}"))?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Error::new(
            "launchd_operation_failed",
            format!("launchctl {operation} failed: {}", stderr.trim()),
        ))
    }
}

impl ServiceManager for SystemLaunchd {
    fn is_loaded(&self, label: &str) -> Result<bool> {
        let target = format!("{}/{label}", self.domain);
        let output = Command::new("/bin/launchctl")
            .args(["print", &target])
            .stdout(Stdio::null())
            .output()
            .context("launchd_operation_failed", "inspect launchd service")?;
        if output.status.success() {
            return Ok(true);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Could not find service") {
            return Ok(false);
        }
        Err(Error::new(
            "launchd_operation_failed",
            format!("launchctl print failed: {}", stderr.trim()),
        ))
    }

    fn bootout(&self, label: &str) -> Result<()> {
        let target = format!("{}/{label}", self.domain);
        Self::run(&["bootout", &target], "bootout")
    }

    fn bootstrap(&self, plist: &Path) -> Result<()> {
        let plist = plist.to_str().ok_or_else(|| {
            Error::new(
                "launchd_operation_failed",
                "LaunchAgent path is not valid UTF-8",
            )
        })?;
        Self::run(&["bootstrap", &self.domain, plist], "bootstrap")
    }
}

pub(crate) fn switch_binding(
    store: &mut Store,
    layout: &Layout,
    manager: &dyn ServiceManager,
    key: &str,
    digest: &str,
) -> Result<BindingRecord> {
    manifest::validate_key(key)?;
    let _management_lock = KeyLock::acquire_management(layout, key)?;
    recover_pending_transition(store, layout, manager, key)?;
    let _key_lock = KeyLock::try_acquire_transition(layout, key)?.ok_or_else(|| {
        Error::new(
            "binding_busy",
            format!("binding {key} has an active activation"),
        )
    })?;
    store.recover_stale(Some(key))?;
    if store.has_running_activation(key)? {
        return Err(Error::new(
            "binding_busy",
            format!("binding {key} has an active or conservatively retained activation"),
        ));
    }

    let definition = store.definition(digest)?;
    if definition.key != key {
        return Err(Error::new(
            "binding_key_mismatch",
            format!(
                "definition {digest} belongs to {}, not {key}",
                definition.key
            ),
        ));
    }
    manifest::validate(&definition.manifest, layout)?;

    let prior_binding = store.optional_binding(key)?;
    let plist_path = layout.plist_path(key);
    let prior_plist = read_prior_plist(&plist_path)?;
    let binary = exact_current_executable(layout)?;
    let desired = render_plist(layout, &definition.manifest, &binary);
    let desired_hash = bytes_sha256(desired.as_bytes());
    let label = Layout::label(key);
    let was_loaded = manager.is_loaded(&label)?;
    validate_prior_service(
        key,
        &plist_path,
        prior_binding.as_ref(),
        prior_plist.as_deref(),
        was_loaded,
        false,
    )?;
    let journal = TransitionJournal::capture_switch(
        key,
        prior_binding.as_ref(),
        prior_plist.as_deref(),
        was_loaded,
        digest,
        desired.as_bytes(),
    );
    write_transition(layout, &journal)?;

    let transition = (|| -> Result<BindingRecord> {
        if was_loaded {
            manager.bootout(&label)?;
        }
        write_atomic(&plist_path, desired.as_bytes())?;
        let binding = store.switch_binding(key, digest, &desired_hash)?;
        manager.bootstrap(&plist_path)?;
        Ok(binding)
    })();

    match transition {
        Ok(binding) => match remove_transition(layout, key) {
            Ok(TransitionRemoval::Durable) => Ok(binding),
            Ok(TransitionRemoval::DurabilityUncertain(error)) => Err(Error::new(
                "binding_transition_commit_uncertain",
                format!(
                    "candidate state is coherent and was not rolled back, but journal removal durability is uncertain: {error}"
                ),
            )),
            Err(error) => Err(compensate_locked(
                store,
                layout,
                manager,
                &journal,
                Error::new(
                    "binding_transition_uncommitted",
                    format!(
                        "candidate became active but its transition journal could not be committed: {error}"
                    ),
                ),
            )),
        },
        Err(error) => Err(compensate_locked(store, layout, manager, &journal, error)),
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn disable_binding(
    store: &mut Store,
    layout: &Layout,
    manager: &dyn ServiceManager,
    key: &str,
    selected_digest: Option<&str>,
) -> Result<BindingRecord> {
    manifest::validate_key(key)?;
    let _management_lock = KeyLock::acquire_management(layout, key)?;
    let _ = exact_current_executable(layout)?;
    if let Some(digest) = selected_digest {
        let definition = store.definition(digest)?;
        if definition.key != key {
            return Err(Error::new(
                "binding_key_mismatch",
                format!(
                    "definition {digest} belongs to {}, not {key}",
                    definition.key
                ),
            ));
        }
    }
    if let Some(binding) = recover_pending_disable(store, layout, manager, key, selected_digest)? {
        return Ok(binding);
    }
    let prior_binding = store.optional_binding(key)?;
    let label = Layout::label(key);
    let was_loaded = manager.is_loaded(&label)?;
    let plist_path = layout.plist_path(key);
    let prior_plist = read_prior_plist(&plist_path)?;
    validate_prior_service(
        key,
        &plist_path,
        prior_binding.as_ref(),
        prior_plist.as_deref(),
        was_loaded,
        true,
    )?;
    if prior_binding.is_none() {
        return store.disable_binding(key, selected_digest);
    }

    let journal = TransitionJournal::capture_disable(
        key,
        prior_binding.as_ref(),
        prior_plist.as_deref(),
        was_loaded,
        selected_digest.or_else(|| {
            prior_binding
                .as_ref()
                .and_then(|binding| binding.definition_digest.as_deref())
        }),
    );
    write_transition(layout, &journal)?;
    if let Err(error) = store.disable_binding(key, selected_digest) {
        return Err(compensate(store, layout, manager, &journal, error));
    }

    // Admission is disabled before waiting. Taking the same lock as the broker lets an
    // already-running child finish naturally; bootout therefore never terminates it.
    let _key_lock = match KeyLock::acquire_transition(layout, key) {
        Ok(key_lock) => key_lock,
        Err(error) => {
            return Err(compensate(store, layout, manager, &journal, error));
        }
    };
    let transition = (|| -> Result<BindingRecord> {
        store.recover_stale(Some(key))?;
        if store.has_running_activation(key)? {
            return Err(Error::new(
                "activation_liveness_unknown",
                format!("binding {key} is disabled, but a retained activation is still live"),
            ));
        }
        if manager.is_loaded(&label)? {
            manager.bootout(&label)?;
        }
        remove_owned_plist(&plist_path)?;
        store.clear_plist_identity(key)
    })();
    match transition {
        Ok(binding) => match remove_transition(layout, key) {
            Ok(TransitionRemoval::Durable) => Ok(binding),
            Ok(TransitionRemoval::DurabilityUncertain(error)) => Err(Error::new(
                "binding_transition_commit_uncertain",
                format!(
                    "disabled state is coherent and was not rolled back, but journal removal durability is uncertain: {error}"
                ),
            )),
            Err(error) => Err(compensate_locked(
                store,
                layout,
                manager,
                &journal,
                Error::new(
                    "binding_transition_uncommitted",
                    format!(
                        "binding was disabled but its transition journal could not be committed: {error}"
                    ),
                ),
            )),
        },
        Err(error) => Err(compensate_locked(store, layout, manager, &journal, error)),
    }
}

pub(crate) fn pending_transitions(layout: &Layout) -> Result<Vec<String>> {
    let mut keys = Vec::new();
    for entry in fs::read_dir(layout.locks_root()).context(
        "binding_recovery_unavailable",
        "read Clockwork lock directory",
    )? {
        let entry = entry.context(
            "binding_recovery_unavailable",
            "read Clockwork lock directory entry",
        )?;
        let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".transition.json") else {
            continue;
        };
        let Some((owner, product)) = stem.split_once('.') else {
            return Err(Error::new(
                "binding_recovery_invalid",
                format!("invalid transition journal name {name}"),
            ));
        };
        if product.contains('.') {
            return Err(Error::new(
                "binding_recovery_invalid",
                format!("invalid transition journal name {name}"),
            ));
        }
        let key = format!("{owner}/{product}");
        manifest::validate_key(&key)?;
        read_transition(layout, &key)?.ok_or_else(|| {
            Error::new(
                "binding_recovery_unavailable",
                format!("transition journal disappeared for {key}"),
            )
        })?;
        keys.push(key);
    }
    keys.sort();
    Ok(keys)
}

pub(crate) fn require_no_pending_transition(layout: &Layout, key: &str) -> Result<()> {
    if read_transition(layout, key)?.is_some() {
        return Err(Error::new(
            "binding_recovery_required",
            format!("binding {key} has an incomplete transition"),
        ));
    }
    Ok(())
}

fn recover_pending_disable(
    store: &mut Store,
    layout: &Layout,
    manager: &dyn ServiceManager,
    key: &str,
    selected_digest: Option<&str>,
) -> Result<Option<BindingRecord>> {
    let Some(mut journal) = read_transition(layout, key)? else {
        return Ok(None);
    };
    let _key_lock = KeyLock::acquire_transition(layout, key)?;
    store.recover_stale(Some(key))?;
    if store.has_running_activation(key)? {
        return Err(Error::new(
            "activation_liveness_unknown",
            format!("binding {key} has a retained activation and remains recovery-gated"),
        ));
    }
    let plist_path = layout.plist_path(key);
    validate_transition_projection(store, &plist_path, &journal)?;

    let current_selection = store
        .optional_binding(key)?
        .and_then(|binding| binding.definition_digest);
    let target = selected_digest.map(ToOwned::to_owned).or_else(|| {
        if journal.operation == TransitionOperation::Disable {
            journal.target_definition_digest.clone()
        } else {
            current_selection
        }
    });
    if journal.operation != TransitionOperation::Disable
        || journal.target_definition_digest != target
    {
        journal = journal.with_disable_intent(target);
        replace_transition(layout, &journal)?;
    }

    let binding = complete_disable_locked(store, layout, manager, &journal)?;
    Ok(Some(binding))
}

fn recover_pending_transition(
    store: &mut Store,
    layout: &Layout,
    manager: &dyn ServiceManager,
    key: &str,
) -> Result<()> {
    let Some(journal) = read_transition(layout, key)? else {
        return Ok(());
    };
    let _key_lock = KeyLock::acquire_transition(layout, key)?;
    store.recover_stale(Some(key))?;
    if store.has_running_activation(key)? {
        return Err(Error::new(
            "activation_liveness_unknown",
            format!("binding {key} has a retained activation and remains recovery-gated"),
        ));
    }
    if journal.operation == TransitionOperation::Disable {
        complete_disable_locked(store, layout, manager, &journal)?;
        return Ok(());
    }
    match restore_transition_locked(store, layout, manager, &journal) {
        Ok(()) => Ok(()),
        Err(restore_error)
            if matches!(
                restore_error.code(),
                "binding_recovery_projection_unproven" | "binding_transition_commit_uncertain"
            ) =>
        {
            Err(Error::new(
                "binding_rollback_failed",
                format!(
                    "recovering the prior transition stopped without further mutation: {restore_error}; any surviving journal was retained"
                ),
            ))
        }
        Err(restore_error) => {
            let fail_disabled = fail_disabled_locked(store, layout, manager, &journal);
            Err(failed_transition(
                &format!("recovering the prior transition failed: {restore_error}"),
                fail_disabled,
            ))
        }
    }
}

fn complete_disable_locked(
    store: &mut Store,
    layout: &Layout,
    manager: &dyn ServiceManager,
    journal: &TransitionJournal,
) -> Result<BindingRecord> {
    validate_transition(journal)?;
    if journal.operation != TransitionOperation::Disable {
        return Err(Error::new(
            "binding_recovery_invalid",
            "disable recovery requires a durable disable intent",
        ));
    }
    let key = &journal.key;
    let label = Layout::label(key);
    let plist_path = layout.plist_path(key);
    validate_transition_projection(store, &plist_path, journal)?;

    store.disable_binding(key, journal.target_definition_digest.as_deref())?;
    ensure_unloaded(manager, &label)?;
    remove_owned_plist(&plist_path)?;
    let binding = store.clear_plist_identity(key)?;
    require_durable_transition_removal(remove_transition(layout, key)?)?;
    Ok(binding)
}

fn compensate(
    store: &mut Store,
    layout: &Layout,
    manager: &dyn ServiceManager,
    journal: &TransitionJournal,
    cause: Error,
) -> Error {
    match KeyLock::acquire_transition(layout, &journal.key) {
        Ok(_key_lock) => compensate_locked(store, layout, manager, journal, cause),
        Err(lock_error) => Error::new(
            "binding_rollback_failed",
            format!(
                "{cause}; the prior transition gate could not be recovered: {lock_error}; no unlocked cleanup was attempted and the journal was retained"
            ),
        ),
    }
}

fn compensate_locked(
    store: &mut Store,
    layout: &Layout,
    manager: &dyn ServiceManager,
    journal: &TransitionJournal,
    cause: Error,
) -> Error {
    match restore_transition_locked(store, layout, manager, journal) {
        Ok(()) => cause,
        Err(restore_error)
            if matches!(
                restore_error.code(),
                "binding_recovery_projection_unproven" | "binding_transition_commit_uncertain"
            ) =>
        {
            Error::new(
                "binding_rollback_failed",
                format!(
                    "{cause}; rollback stopped without further mutation: {restore_error}; any surviving journal was retained"
                ),
            )
        }
        Err(restore_error) => {
            let fail_disabled = fail_disabled_locked(store, layout, manager, journal);
            failed_transition(
                &format!("{cause}; restoring the durable prior transition failed: {restore_error}"),
                fail_disabled,
            )
        }
    }
}

fn fail_disabled_locked(
    store: &mut Store,
    layout: &Layout,
    manager: &dyn ServiceManager,
    journal: &TransitionJournal,
) -> Result<()> {
    let key = &journal.key;
    let plist = layout.plist_path(key);
    validate_transition_projection(store, &plist, journal)?;
    let target = if journal.operation == TransitionOperation::Disable {
        journal.target_definition_digest.clone()
    } else {
        store
            .optional_binding(key)?
            .and_then(|binding| binding.definition_digest)
    };
    let disable_journal = journal.with_disable_intent(target);
    if journal.operation != TransitionOperation::Disable
        || journal.target_definition_digest != disable_journal.target_definition_digest
    {
        replace_transition(layout, &disable_journal)?;
    }

    let label = Layout::label(key);
    force_disabled(store, manager, &disable_journal, key, &label, &plist)?;
    require_durable_transition_removal(remove_transition(layout, key)?)
}

fn restore_transition_locked(
    store: &mut Store,
    layout: &Layout,
    manager: &dyn ServiceManager,
    journal: &TransitionJournal,
) -> Result<()> {
    validate_transition(journal)?;
    let key = &journal.key;
    let label = Layout::label(key);
    let plist_path = layout.plist_path(key);
    validate_transition_projection(store, &plist_path, journal)?;
    ensure_unloaded(manager, &label)?;

    let prior_binding = journal.prior_binding();
    store.restore_binding(key, prior_binding.as_ref())?;
    restore_plist(&plist_path, journal.prior_plist.as_deref())?;
    if journal.prior_loaded {
        manager.bootstrap(&plist_path)?;
    }
    let loaded = manager.is_loaded(&label)?;
    if loaded != journal.prior_loaded {
        return Err(Error::new(
            "binding_recovery_incomplete",
            format!("the prior loaded state for {key} was not restored"),
        ));
    }
    if store.optional_binding(key)? != prior_binding {
        return Err(Error::new(
            "binding_recovery_incomplete",
            format!("the prior database binding for {key} was not restored"),
        ));
    }
    if read_prior_plist(&plist_path)? != journal.prior_plist {
        return Err(Error::new(
            "binding_recovery_incomplete",
            format!("the prior LaunchAgent bytes for {key} were not restored"),
        ));
    }
    require_durable_transition_removal(remove_transition(layout, key)?)
}

fn ensure_unloaded(manager: &dyn ServiceManager, label: &str) -> Result<()> {
    if !manager.is_loaded(label)? {
        return Ok(());
    }
    let bootout = manager.bootout(label);
    match manager.is_loaded(label) {
        Ok(false) => Ok(()),
        Ok(true) => Err(bootout.err().unwrap_or_else(|| {
            Error::new(
                "launchd_operation_failed",
                format!("launchd label {label} remained loaded after bootout"),
            )
        })),
        Err(inspection_error) => Err(Error::new(
            "launchd_operation_failed",
            format!("the loaded state of {label} is unknown after bootout: {inspection_error}"),
        )),
    }
}

#[allow(clippy::too_many_lines)]
fn validate_transition(journal: &TransitionJournal) -> Result<()> {
    if journal.schema_version != 1 {
        return Err(Error::new(
            "binding_recovery_invalid",
            "the transition journal has an unsupported schema",
        ));
    }
    manifest::validate_key(&journal.key)?;
    match journal.prior_binding.as_ref() {
        Some(binding) => {
            if binding.key != journal.key {
                return Err(Error::new(
                    "binding_recovery_invalid",
                    "the transition journal binding key does not match its identity",
                ));
            }
            if let Some(digest) = binding.definition_digest.as_deref() {
                manifest::validate_definition_digest(digest)?;
            }
            if let Some(expected) = binding.plist_sha256.as_deref() {
                manifest::validate_definition_digest(expected)?;
                if let Some(plist) = journal.prior_plist.as_deref()
                    && bytes_sha256(plist) != expected
                {
                    return Err(Error::new(
                        "binding_recovery_invalid",
                        "the transition journal plist does not match its prior digest",
                    ));
                }
            } else if journal.prior_plist.is_some() {
                return Err(Error::new(
                    "binding_recovery_invalid",
                    "retained prior LaunchAgent bytes require a recorded digest",
                ));
            }
            if binding.enabled
                && (binding.definition_digest.is_none()
                    || binding.plist_sha256.is_none()
                    || journal.prior_plist.is_none())
            {
                return Err(Error::new(
                    "binding_recovery_invalid",
                    "an enabled prior binding requires definition and LaunchAgent identity",
                ));
            }
        }
        None if journal.prior_plist.is_some() || journal.prior_loaded => {
            return Err(Error::new(
                "binding_recovery_invalid",
                "an absent prior binding cannot own plist or loaded state",
            ));
        }
        None => {}
    }
    if journal.prior_loaded && journal.prior_plist.is_none() {
        return Err(Error::new(
            "binding_recovery_invalid",
            "a prior loaded service requires retained plist bytes",
        ));
    }
    match (
        journal.candidate_definition_digest.as_deref(),
        journal.candidate_plist.as_deref(),
    ) {
        (Some(digest), Some(_)) => manifest::validate_definition_digest(digest)?,
        (None, None) => {}
        _ => {
            return Err(Error::new(
                "binding_recovery_invalid",
                "candidate definition identity and LaunchAgent bytes must be retained together",
            ));
        }
    }
    if journal.operation == TransitionOperation::Switch
        && (journal.candidate_plist.is_none()
            || journal.candidate_definition_digest.is_none()
            || journal.target_definition_digest != journal.candidate_definition_digest)
    {
        return Err(Error::new(
            "binding_recovery_invalid",
            "a switch transition requires one matching candidate definition and target",
        ));
    }
    if let Some(digest) = journal.target_definition_digest.as_deref() {
        manifest::validate_definition_digest(digest)?;
    }
    if journal.operation == TransitionOperation::Disable
        && journal.target_definition_digest.is_some()
        && journal.prior_binding.is_none()
        && journal.candidate_definition_digest.is_none()
    {
        // A disable tombstone can deliberately retain a selected registered definition even
        // when the key had no prior binding. That definition is checked by the public caller.
        if journal.prior_plist.is_some() || journal.prior_loaded {
            return Err(Error::new(
                "binding_recovery_invalid",
                "a new disabled selection cannot own prior launchd state",
            ));
        }
    }
    Ok(())
}

fn validate_transition_projection(
    store: &Store,
    path: &Path,
    journal: &TransitionJournal,
) -> Result<()> {
    let current = read_prior_plist(path).map_err(|error| {
        Error::new(
            "binding_recovery_projection_unproven",
            format!("cannot attribute current LaunchAgent state: {error}"),
        )
    })?;
    let matches_prior = current == journal.prior_plist;
    let matches_candidate = journal
        .candidate_plist
        .as_ref()
        .is_some_and(|candidate| current.as_ref() == Some(candidate));
    // Absence is safe to repair: there are no unrecognized bytes to overwrite or remove.
    // It is also a state Clockwork can itself leave after an interrupted cleanup.
    let attributable_absence = current.is_none();
    if matches_prior || matches_candidate || attributable_absence {
        let prior_binding = journal.prior_binding();
        let current_binding = store.optional_binding(&journal.key)?;
        let matches_prior_binding = current_binding == prior_binding;
        let candidate_hash = journal.candidate_plist.as_deref().map(bytes_sha256);
        let matches_candidate_binding = current_binding.as_ref().is_some_and(|binding| {
            journal.candidate_definition_digest.is_some()
                && candidate_hash.is_some()
                && binding.enabled
                && binding.definition_digest.as_deref()
                    == journal.candidate_definition_digest.as_deref()
                && binding.plist_sha256.as_deref() == candidate_hash.as_deref()
        });
        let prior_selection = prior_binding
            .as_ref()
            .and_then(|binding| binding.definition_digest.as_deref());
        let prior_hash = prior_binding
            .as_ref()
            .and_then(|binding| binding.plist_sha256.as_deref());
        let matches_disabled_intermediate = current_binding.as_ref().is_some_and(|binding| {
            let selection_is_attributable = binding.definition_digest.as_deref() == prior_selection
                || binding.definition_digest.as_deref()
                    == journal.candidate_definition_digest.as_deref()
                || binding.definition_digest.as_deref()
                    == journal.target_definition_digest.as_deref();
            let plist_identity_is_attributable = binding.plist_sha256.is_none()
                || binding.plist_sha256.as_deref() == prior_hash
                || binding.plist_sha256.as_deref() == candidate_hash.as_deref();
            !binding.enabled && selection_is_attributable && plist_identity_is_attributable
        });
        if matches_prior_binding || matches_candidate_binding || matches_disabled_intermediate {
            return Ok(());
        }
        return Err(Error::new(
            "binding_recovery_projection_unproven",
            format!(
                "the database binding for {} matches neither the durable prior nor an attributable Clockwork transition state",
                journal.key
            ),
        ));
    }
    Err(Error::new(
        "binding_recovery_projection_unproven",
        format!(
            "{} matches neither the durable prior nor Clockwork candidate bytes",
            path.display()
        ),
    ))
}

fn write_transition(layout: &Layout, journal: &TransitionJournal) -> Result<()> {
    let path = layout.transition_path(&journal.key);
    if path.exists() || path.is_symlink() {
        return Err(Error::new(
            "binding_recovery_required",
            format!("a transition journal already exists at {}", path.display()),
        ));
    }
    let contents = transition_bytes(journal)?;
    write_private_atomic(&path, &contents, "transition journal")
}

fn replace_transition(layout: &Layout, journal: &TransitionJournal) -> Result<()> {
    let path = layout.transition_path(&journal.key);
    if read_transition(layout, &journal.key)?.is_none() {
        return Err(Error::new(
            "binding_recovery_required",
            format!("the transition journal at {} disappeared", path.display()),
        ));
    }
    let contents = transition_bytes(journal)?;
    write_private_atomic(&path, &contents, "replacement transition journal")
}

fn transition_bytes(journal: &TransitionJournal) -> Result<Vec<u8>> {
    validate_transition(journal)?;
    let contents = serde_json::to_vec(journal).context(
        "binding_transition_unavailable",
        "serialize transition journal",
    )?;
    if contents.len() > 1_048_576 {
        return Err(Error::new(
            "binding_transition_unavailable",
            "transition journal exceeds 1 MiB",
        ));
    }
    Ok(contents)
}

fn read_transition(layout: &Layout, key: &str) -> Result<Option<TransitionJournal>> {
    let path = layout.transition_path(key);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::new(
                "binding_recovery_unavailable",
                format!("inspect {}: {error}", path.display()),
            ));
        }
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != current_uid()?
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > 1_048_576
    {
        return Err(Error::new(
            "binding_recovery_invalid",
            format!(
                "{} is not a private Clockwork transition journal",
                path.display()
            ),
        ));
    }
    let contents = fs::read(&path).context(
        "binding_recovery_unavailable",
        format!("read {}", path.display()),
    )?;
    let journal: TransitionJournal = serde_json::from_slice(&contents)
        .context("binding_recovery_invalid", "decode transition journal")?;
    if journal.key != key {
        return Err(Error::new(
            "binding_recovery_invalid",
            "transition journal key does not match its file",
        ));
    }
    validate_transition(&journal)?;
    Ok(Some(journal))
}

enum TransitionRemoval {
    Durable,
    DurabilityUncertain(Error),
}

fn remove_transition(layout: &Layout, key: &str) -> Result<TransitionRemoval> {
    let path = layout.transition_path(key);
    match fs::symlink_metadata(&path) {
        Ok(metadata)
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.nlink() != 1
                || metadata.uid() != current_uid()?
                || metadata.permissions().mode() & 0o077 != 0 =>
        {
            return Err(Error::new(
                "binding_recovery_invalid",
                format!("refuse to remove unsafe journal {}", path.display()),
            ));
        }
        Ok(_) => fs::remove_file(&path).context(
            "binding_recovery_unavailable",
            format!("remove {}", path.display()),
        )?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TransitionRemoval::Durable);
        }
        Err(error) => {
            return Err(Error::new(
                "binding_recovery_unavailable",
                format!("inspect {}: {error}", path.display()),
            ));
        }
    }
    match File::open(layout.locks_root())
        .and_then(|directory| directory.sync_all())
        .context(
            "binding_recovery_unavailable",
            "sync transition journal removal",
        ) {
        Ok(()) => Ok(TransitionRemoval::Durable),
        Err(error) => Ok(TransitionRemoval::DurabilityUncertain(error)),
    }
}

fn require_durable_transition_removal(removal: TransitionRemoval) -> Result<()> {
    match removal {
        TransitionRemoval::Durable => Ok(()),
        TransitionRemoval::DurabilityUncertain(error) => Err(Error::new(
            "binding_transition_commit_uncertain",
            format!(
                "coherent binding state was retained, but transition-journal removal durability is uncertain: {error}"
            ),
        )),
    }
}

fn validate_prior_service(
    key: &str,
    plist_path: &Path,
    binding: Option<&BindingRecord>,
    plist: Option<&[u8]>,
    loaded: bool,
    allow_enabled_unloaded: bool,
) -> Result<()> {
    let Some(binding) = binding else {
        if plist.is_some() || loaded {
            return Err(Error::new(
                "launchd_label_collision",
                format!("{key} has a plist or loaded label without a Clockwork binding"),
            ));
        }
        return Ok(());
    };
    if !binding.enabled {
        match binding.plist_sha256.as_deref() {
            Some(expected_hash) => {
                if let Some(plist) = plist {
                    if bytes_sha256(plist) != expected_hash {
                        return Err(Error::new(
                            "launchd_plist_foreign",
                            format!(
                                "{} no longer matches Clockwork's recorded bytes",
                                plist_path.display()
                            ),
                        ));
                    }
                } else if loaded {
                    return Err(Error::new(
                        "launchd_state_incoherent",
                        format!("disabled binding {key} has a loaded label but no owned plist"),
                    ));
                }
            }
            None if plist.is_some() || loaded => {
                return Err(Error::new(
                    "launchd_state_incoherent",
                    format!("disabled binding {key} has unowned launchd state"),
                ));
            }
            None => {}
        }
        return Ok(());
    }
    let expected_hash = binding.plist_sha256.as_deref().ok_or_else(|| {
        Error::new(
            "launchd_state_incoherent",
            format!("enabled binding {key} has no recorded plist identity"),
        )
    })?;
    if !loaded && !allow_enabled_unloaded {
        return Err(Error::new(
            "launchd_state_incoherent",
            format!("enabled binding {key} is not loaded"),
        ));
    }
    let plist = plist.ok_or_else(|| {
        Error::new(
            "launchd_state_incoherent",
            format!(
                "enabled binding {key} has no plist at {}",
                plist_path.display()
            ),
        )
    })?;
    if bytes_sha256(plist) != expected_hash {
        return Err(Error::new(
            "launchd_plist_foreign",
            format!(
                "{} no longer matches Clockwork's recorded bytes",
                plist_path.display()
            ),
        ));
    }
    Ok(())
}

pub(crate) fn render_plist(layout: &Layout, manifest: &Manifest, binary: &Path) -> String {
    let mut arguments = vec![binary.display().to_string()];
    if let Some(state_root) = layout.state_root_override() {
        arguments.push("--state-root".to_owned());
        arguments.push(state_root.display().to_string());
    }
    arguments.push("__launchd".to_owned());
    arguments.push(manifest.key.clone());
    let mut argument_xml = String::new();
    for argument in &arguments {
        argument_xml.push_str("        <string>");
        argument_xml.push_str(&xml_escape(argument));
        argument_xml.push_str("</string>\n");
    }
    let schedule_xml = match manifest.schedule {
        Schedule::Interval { seconds, .. } => {
            format!("    <key>StartInterval</key>\n    <integer>{seconds}</integer>\n")
        }
        Schedule::LocalCalendar { hour, minute, .. } => format!(
            "    <key>StartCalendarInterval</key>\n    <dict>\n        <key>Hour</key>\n        <integer>{hour}</integer>\n        <key>Minute</key>\n        <integer>{minute}</integer>\n    </dict>\n"
        ),
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
             <key>Label</key>\n\
             <string>{label}</string>\n\
             <key>ProgramArguments</key>\n\
             <array>\n\
         {argument_xml}\
             </array>\n\
         {schedule_xml}\
             <key>RunAtLoad</key>\n\
             <{run_at_load}/>\n\
             <key>ProcessType</key>\n\
             <string>Background</string>\n\
             <key>Umask</key>\n\
             <integer>63</integer>\n\
             <key>EnvironmentVariables</key>\n\
             <dict>\n\
                 <key>HOME</key>\n\
                 <string>{home}</string>\n\
             </dict>\n\
             <key>StandardOutPath</key>\n\
             <string>{stdout}</string>\n\
             <key>StandardErrorPath</key>\n\
             <string>{stderr}</string>\n\
         </dict>\n\
         </plist>\n",
        label = xml_escape(&Layout::label(&manifest.key)),
        run_at_load = if manifest.schedule.run_at_load() {
            "true"
        } else {
            "false"
        },
        home = xml_escape(&layout.home().display().to_string()),
        stdout = xml_escape(&layout.stdout_path(&manifest.key).display().to_string()),
        stderr = xml_escape(&layout.stderr_path(&manifest.key).display().to_string()),
    )
}

fn exact_current_executable(layout: &Layout) -> Result<PathBuf> {
    let path = std::env::current_exe().context(
        "clockwork_binary_unavailable",
        "locate current Clockwork executable",
    )?;
    let path = path.canonicalize().context(
        "clockwork_binary_unavailable",
        "canonicalize Clockwork executable",
    )?;
    let metadata = fs::symlink_metadata(&path).context(
        "clockwork_binary_unavailable",
        "inspect Clockwork executable",
    )?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o100 == 0
        || metadata.permissions().mode() & 0o022 != 0
        || metadata.uid() != current_uid()?
    {
        return Err(Error::new(
            "clockwork_binary_unsafe",
            "Clockwork executable must be a current-user-owned non-hard-linked, non-writable regular executable",
        ));
    }
    if layout.state_root_override().is_none() {
        verify_installed_binary(layout, &path)?;
    }
    Ok(path)
}

fn verify_installed_binary(layout: &Layout, binary: &Path) -> Result<()> {
    let release_root = binary
        .parent()
        .filter(|parent| parent.file_name().and_then(|name| name.to_str()) == Some("bin"))
        .and_then(Path::parent)
        .ok_or_else(|| {
            Error::new(
                "clockwork_binary_uninstalled",
                "binding changes require an installed Clockwork release binary",
            )
        })?;
    let installed_releases = layout.state_root().join("install/releases");
    if release_root.parent() != Some(installed_releases.as_path()) {
        return Err(Error::new(
            "clockwork_binary_uninstalled",
            "binding changes require a Clockwork binary beneath the installed releases directory",
        ));
    }
    let release_id = release_root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| {
            Error::new(
                "clockwork_binary_uninstalled",
                "installed Clockwork release directory has no content identity",
            )
        })?;
    let manifest_path = release_root.join("manifest.txt");
    let manifest = fs::read_to_string(&manifest_path).context(
        "clockwork_binary_unavailable",
        format!("read {}", manifest_path.display()),
    )?;
    let manifest_release = manifest
        .lines()
        .find_map(|line| line.strip_prefix("release_id="));
    let expected_binary = manifest
        .lines()
        .find_map(|line| line.strip_prefix("binary_sha256="));
    let actual_binary = bytes_sha256(&fs::read(binary).context(
        "clockwork_binary_unavailable",
        "hash installed Clockwork executable",
    )?);
    if manifest.lines().nth(1) != Some("product=clockwork")
        || manifest_release != Some(release_id)
        || expected_binary.is_none()
        || expected_binary != Some(actual_binary.as_str())
    {
        return Err(Error::new(
            "clockwork_binary_tampered",
            "Clockwork executable does not match its installed release manifest",
        ));
    }
    Ok(())
}

fn read_prior_plist(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.nlink() != 1
                || metadata.uid() != current_uid()?
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(Error::new(
                    "launchd_plist_unsafe",
                    format!(
                        "{} must be a private current-user-owned, non-symbolic, non-hard-linked regular file",
                        path.display()
                    ),
                ));
            }
            if metadata.len() > 65_536 {
                return Err(Error::new(
                    "launchd_plist_unsafe",
                    format!("{} exceeds 64 KiB", path.display()),
                ));
            }
            fs::read(path).map(Some).context(
                "launchd_plist_unavailable",
                format!("read {}", path.display()),
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::new(
            "launchd_plist_unavailable",
            format!("inspect {}: {error}", path.display()),
        )),
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        Error::new(
            "launchd_plist_unavailable",
            format!("{} has no parent", path.display()),
        )
    })?;
    let mut temporary = NamedTempFile::new_in(parent)
        .context("launchd_plist_unavailable", "create staged LaunchAgent")?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .context(
            "launchd_plist_unavailable",
            "make staged LaunchAgent private",
        )?;
    temporary
        .write_all(contents)
        .context("launchd_plist_unavailable", "write staged LaunchAgent")?;
    temporary
        .as_file()
        .sync_all()
        .context("launchd_plist_unavailable", "sync staged LaunchAgent")?;
    temporary.persist(path).map_err(|error| {
        Error::new(
            "launchd_plist_unavailable",
            format!("install {}: {}", path.display(), error.error),
        )
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .context("launchd_plist_unavailable", "sync LaunchAgent directory")?;
    Ok(())
}

fn write_private_atomic(path: &Path, contents: &[u8], label: &str) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        Error::new(
            "binding_transition_unavailable",
            format!("{label} {} has no parent", path.display()),
        )
    })?;
    let mut temporary = NamedTempFile::new_in(parent).context(
        "binding_transition_unavailable",
        format!("create staged {label}"),
    )?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .context(
            "binding_transition_unavailable",
            format!("make staged {label} private"),
        )?;
    temporary.write_all(contents).context(
        "binding_transition_unavailable",
        format!("write staged {label}"),
    )?;
    temporary.as_file().sync_all().context(
        "binding_transition_unavailable",
        format!("sync staged {label}"),
    )?;
    temporary.persist(path).map_err(|error| {
        Error::new(
            "binding_transition_unavailable",
            format!("install {label} {}: {}", path.display(), error.error),
        )
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .context(
            "binding_transition_unavailable",
            format!("sync {label} directory"),
        )?;
    Ok(())
}

fn restore_plist(path: &Path, prior: Option<&[u8]>) -> Result<()> {
    match prior {
        Some(contents) => write_atomic(path, contents),
        None => remove_file_synced(path, "remove candidate LaunchAgent"),
    }
}

fn remove_owned_plist(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.nlink() != 1
                || metadata.uid() != current_uid()?
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(Error::new(
                    "launchd_plist_unsafe",
                    format!("refuse to remove unowned or unsafe {}", path.display()),
                ));
            }
            remove_file_synced(path, "remove owned LaunchAgent")
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::new(
            "launchd_plist_unavailable",
            format!("inspect {}: {error}", path.display()),
        )),
    }
}

fn remove_file_synced(path: &Path, operation: &str) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {
            let parent = path.parent().ok_or_else(|| {
                Error::new(
                    "launchd_plist_unavailable",
                    format!("{} has no parent", path.display()),
                )
            })?;
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .context(
                    "launchd_plist_unavailable",
                    format!("sync directory after {operation}"),
                )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::new(
            "launchd_plist_unavailable",
            format!("{operation} {}: {error}", path.display()),
        )),
    }
}

fn force_disabled(
    store: &mut Store,
    manager: &dyn ServiceManager,
    journal: &TransitionJournal,
    key: &str,
    label: &str,
    plist: &Path,
) -> Result<()> {
    validate_transition_projection(store, plist, journal)?;
    let mut failures = Vec::new();
    if let Err(error) = store.disable_binding(key, journal.target_definition_digest.as_deref()) {
        failures.push(error.to_string());
    }
    match manager.is_loaded(label) {
        Ok(true) => {
            if let Err(error) = manager.bootout(label) {
                failures.push(error.to_string());
            }
        }
        Ok(false) => {}
        Err(error) => {
            failures.push(error.to_string());
        }
    }
    if let Err(error) = remove_owned_plist(plist) {
        failures.push(error.to_string());
    }
    if failures.is_empty()
        && let Err(error) = store.clear_plist_identity(key)
    {
        failures.push(error.to_string());
    }
    match store.binding(key) {
        Ok(binding) if !binding.enabled => {}
        Ok(_) => failures.push("binding remained enabled".to_owned()),
        Err(error) => failures.push(error.to_string()),
    }
    match manager.is_loaded(label) {
        Ok(false) => {}
        Ok(true) => failures.push("launchd label remained loaded".to_owned()),
        Err(error) => failures.push(error.to_string()),
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(Error::new("fail_disabled_unproven", failures.join("; ")))
    }
}

fn failed_transition(message: &str, fail_disabled: Result<()>) -> Error {
    match fail_disabled {
        Ok(()) => Error::new(
            "binding_rollback_failed",
            format!("{message}; the binding was forced disabled"),
        ),
        Err(error) => Error::new(
            "binding_rollback_failed",
            format!("{message}; fail-disabled state could not be proved: {error}"),
        ),
    }
}

fn bytes_sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use tempfile::tempdir;

    use super::render_plist;
    use crate::model::{Authority, LaunchImage, Manifest, Output, OverlapPolicy, Schedule};
    use crate::paths::Layout;

    #[test]
    fn plist_contains_only_the_private_clockwork_entrypoint() {
        let temporary = tempdir().expect("temporary directory");
        let layout = Layout::isolated(temporary.path());
        let manifest = Manifest {
            schema_version: 1,
            key: "annals/inbox".to_owned(),
            release_id: "a".repeat(64),
            release_root: "/release".to_owned(),
            authority: Authority::CurrentUserBackground,
            overlap: OverlapPolicy::Skip,
            timeout_seconds: None,
            arguments: vec!["inbox".to_owned(), "process-one".to_owned()],
            cwd: "/release".to_owned(),
            schedule: Schedule::Interval {
                seconds: 300,
                run_at_load: true,
            },
            launch: LaunchImage::Direct {
                program: "/release/annals".to_owned(),
                sha256: "b".repeat(64),
            },
            environment: BTreeMap::new(),
            output: Output {
                stdout: "/product/stdout".to_owned(),
                stderr: "/product/stderr".to_owned(),
            },
        };

        let plist = render_plist(&layout, &manifest, Path::new("/release/clockwork"));
        assert!(plist.contains("__launchd"));
        assert!(plist.contains("annals/inbox"));
        assert!(!plist.contains("process-one"));
        assert!(!plist.contains("/product/stdout"));
    }
}
