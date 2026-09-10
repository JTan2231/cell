//! Product-owned installation layout and predecessor release proof.
use cell_install::legacy::{LegacyProof, LegacyProvider, LegacySpec};
use cell_install::simple::Spec;
use cell_install::transaction::LockKind;

#[must_use]
pub fn specification() -> Spec {
    Spec {
        product: "clockwork",
        application: "Clockwork",
        source_directory: "clockwork",
        provider_source: "clockwork/chancery",
        legacy_provider_path: "share/chancery/clockwork",
        legacy: &LegacySpec {
            format: "1",
            manifest: "manifest.txt",
            metadata: &["version", "product"],
            proofs: &[
                LegacyProof {
                    key: "binary_sha256",
                    paths: &["bin/clockwork"],
                },
                LegacyProof {
                    key: "deployer_sha256",
                    paths: &["package/deploy-user.sh"],
                },
                LegacyProof {
                    key: "uninstaller_sha256",
                    paths: &["package/uninstall-user.sh"],
                },
            ],
            providers: &[LegacyProvider {
                key: "chancery_sha256",
                provider: "clockwork",
                path: "share/chancery/clockwork",
                version_key: "version",
            }],
            hash_path_lines: true,
        },
        wrapper: None,
        lock_kind: LockKind::Shlock,
        lock_at_state: true,
        maintained: false,
    }
}

/// Coordinate broker refresh while preserving product definitions and halt intent.
/// Direct program installation remains a selector-only operation.
///
/// # Errors
/// Refuses incomplete inventories, changed selections, unavailable Clockwork, or
/// an unconfirmed disabled or active binding transition.
pub fn deployment_lifecycle(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> cell_install::Result<serde_json::Value> {
    deployment_operation(context, operation)
        .map_err(|error| cell_install::Error::new(error.to_string()))
}

fn deployment_operation(
    context: &cell_install::adapter::Context,
    operation: cell_install::adapter::Operation,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    use crate::api::Client;
    use cell_install::adapter::Operation;
    use serde_json::json;
    let executable = context.home.join(".local/bin/clockwork");
    let client = Client::new(&executable);
    if operation == Operation::Inspect {
        // A first installation has no runtime inventory to open or initialize.
        let bindings = if context
            .home
            .join("Library/Application Support/Clockwork/clockwork.db")
            .try_exists()?
        {
            complete_bindings(&client)?
        } else {
            let agents = context.home.join("Library/LaunchAgents");
            if agents.try_exists()? {
                for entry in std::fs::read_dir(agents)? {
                    let name = entry?.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with("org.clockwork.") && name.ends_with(".plist") {
                        return Err("Clockwork runtime inventory is missing while a generated plist remains".into());
                    }
                }
            }
            Vec::new()
        };
        return Ok(json!({"bindings":bindings}));
    }
    let bindings: Vec<crate::api::BindingRecord> =
        serde_json::from_value(context.prior()?["lifecycle"]["bindings"].clone())?;
    if operation == Operation::Hold {
        for before in &bindings {
            let observed = client.binding(&before.key)?;
            if context.request.recovery.is_none()
                && (observed.definition_digest != before.definition_digest
                    || observed.halted_incident != before.halted_incident
                    || !before.enabled && observed.enabled)
            {
                return Err("Clockwork binding changed since deployment inspection".into());
            }
            client.disable(&before.key, None)?;
        }
    }
    if matches!(
        operation,
        Operation::Configure | Operation::Verify | Operation::Recover
    ) {
        for before in &bindings {
            let observed = client.binding(&before.key)?;
            preserve_halt(before, &observed)?;
            if observed.enabled {
                return Err("Clockwork binding is active before deployment activation".into());
            }
            if let Some(digest) = &observed.definition_digest {
                let definition = client.definition(digest)?;
                if definition.key != before.key || definition.digest != *digest {
                    return Err("Clockwork selected definition differs from its binding".into());
                }
            }
        }
    }
    if operation == Operation::Activate {
        activate_bindings(&client, &bindings, &context.request.activation_bindings)?;
    }
    Ok(json!({"binding_count":bindings.len()}))
}

fn complete_bindings(
    client: &crate::api::Client,
) -> Result<Vec<crate::api::BindingRecord>, crate::api::Error> {
    // The public list interface supplies a bounded prefix, not an offset cursor.
    let mut limit = 20;
    loop {
        let page = client.bindings_limit(limit)?;
        if !page.has_more {
            return Ok(page.items);
        }
        limit = limit
            .checked_mul(2)
            .ok_or_else(|| crate::api::Error("Clockwork binding inventory is too large".into()))?;
    }
}

fn preserve_halt(
    before: &crate::api::BindingRecord,
    observed: &crate::api::BindingRecord,
) -> Result<(), Box<dyn std::error::Error>> {
    if before.halted_incident.is_some() && observed.halted_incident != before.halted_incident {
        return Err("deployment changed a Clockwork failure halt".into());
    }
    Ok(())
}

fn activate_bindings(
    client: &crate::api::Client,
    bindings: &[crate::api::BindingRecord],
    managed: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    for before in bindings {
        if managed.iter().any(|key| key == &before.key) {
            // The owning adapter applies its requested activation intent.
            continue;
        }
        let observed = client.binding(&before.key)?;
        preserve_halt(before, &observed)?;
        if before.enabled {
            // Other product adapters may have selected their new immutable
            // definition while disabled. Reuse that selection with this broker.
            let digest = observed
                .definition_digest
                .as_deref()
                .ok_or("enabled intent has no selected definition")?;
            let selected = client.switch(&before.key, digest)?;
            preserve_halt(before, &selected)?;
            if !selected.enabled || selected.definition_digest.as_deref() != Some(digest) {
                return Err("Clockwork did not confirm refreshed broker selection".into());
            }
        } else if observed.enabled {
            return Err("deployment enabled a previously disabled Clockwork binding".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod deployment_tests {
    use super::*;
    use crate::api::{BindingRecord, Client};
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn complete_inventory_refreshes_custom_bindings_and_preserves_product_intent()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let executable = temp.path().join("clockwork");
        fs::write(
            &executable,
            r"#!/usr/bin/python3
import json,sys
from pathlib import Path
path=Path(__file__).with_name('state.json')
state=json.loads(path.read_text())
args=sys.argv[2:]
if args[:2]==['binding','list']:
    limit=int(args[3])
    data={'output_version':2,'items':state['bindings'][:limit],'has_more':len(state['bindings'])>limit}
elif args[:2]==['binding','show']:
    data=next(item for item in state['bindings'] if item['key']==args[2])
elif args[:2]==['binding','switch']:
    data=next(item for item in state['bindings'] if item['key']==args[2])
    data['enabled']=True
    data['definition_digest']=args[3]
    state['refreshed'].append(args[2])
else: raise RuntimeError('unexpected operation')
path.write_text(json.dumps(state))
print(json.dumps({'ok':True,'data':data}))
",
        )?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
        let mut bindings: Vec<_> = (0..25)
            .map(|index| BindingRecord {
                key: format!("annals/custom-{index}"),
                definition_digest: Some(format!("definition-{index}")),
                enabled: true,
                halted_incident: Some(format!("incident-{index}")),
                failure_policy_active: true,
                updated_at: 1,
            })
            .collect();
        bindings.push(BindingRecord {
            key: "mentor/worker".into(),
            ..bindings[0].clone()
        });
        bindings.push(BindingRecord {
            key: "custom/disabled".into(),
            enabled: false,
            ..bindings[0].clone()
        });
        let path = temp.path().join("state.json");
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({"bindings":bindings,"refreshed":[]}))?,
        )?;
        let client = Client::new(&executable);
        let captured = complete_bindings(&client)?;
        assert_eq!(captured.len(), 27);
        for binding in &mut bindings {
            binding.enabled = false;
        }
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({"bindings":bindings,"refreshed":[]}))?,
        )?;
        activate_bindings(&client, &captured, &["mentor/worker".into()])?;
        let state: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
        assert_eq!(
            state["refreshed"]
                .as_array()
                .ok_or("refreshed list absent")?
                .len(),
            25
        );
        assert!(!client.binding("mentor/worker")?.enabled);
        assert!(!client.binding("custom/disabled")?.enabled);
        for prior in captured.iter().take(25) {
            let current = client.binding(&prior.key)?;
            assert!(current.enabled);
            assert_eq!(current.halted_incident, prior.halted_incident);
            assert_eq!(current.definition_digest, prior.definition_digest);
        }
        Ok(())
    }
}
