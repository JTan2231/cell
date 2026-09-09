use clockwork::api::{BindingRecord, Client, Failure, Manifest, Success, decode};

#[test]
fn client_reads_isolated_cli_state_and_preserves_provider_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let state = temporary.path().join("Clockwork State");
    let client = Client::new(env!("CARGO_BIN_EXE_clockwork")).with_state_root(&state);
    assert!(client.definitions()?.items.is_empty());
    assert!(client.bindings()?.items.is_empty());
    assert!(client.history(None, 1)?.items.is_empty());
    assert!(state.join("clockwork.db").is_file());
    assert!(
        client
            .history(None, 0)
            .is_err_and(|error| { error.to_string().contains("history_limit_invalid") })
    );
    assert!(
        client
            .definition("invalid")
            .is_err_and(|error| { error.to_string().contains("manifest_invalid") })
    );
    Ok(())
}

#[test]
fn existing_binding_wire_shape_is_importable_without_private_state()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = br#"{"ok":true,"data":{"key":"example/worker","definition_digest":null,"enabled":false,"updated_at":42}}"#;
    let binding: BindingRecord = decode(fixture)?;
    assert_eq!(binding.key, "example/worker");
    assert!(!binding.enabled);
    let emitted = serde_json::to_value(Success {
        ok: true,
        data: binding,
    })?;
    assert_eq!(emitted["data"]["halted_incident"], serde_json::Value::Null);
    assert_eq!(emitted["data"]["failure_policy_active"], false);
    assert!(emitted["data"].get("plist_sha256").is_none());
    Ok(())
}

#[test]
fn provider_errors_and_unsupported_manifests_are_not_success() {
    let fixture = br#"{"ok":false,"error":{"code":"binding_not_found","message":"no binding"}}"#;
    let result = decode::<BindingRecord>(fixture);
    assert!(result.is_err_and(|error| error.to_string().contains("binding_not_found")));
    assert!(decode::<Failure>(br#"{"ok":"true","data":{}}"#).is_err());
    assert!(Manifest::from_toml("schema_version = 999").is_err());
}

#[test]
fn schema_one_digest_keeps_its_original_canonical_bytes() -> Result<(), Box<dyn std::error::Error>>
{
    use sha2::{Digest as _, Sha256};
    let source = r#"{"schema_version":1,"key":"example/worker","release_id":"0000000000000000000000000000000000000000000000000000000000000000","release_root":"/fixture/release","authority":"current-user-background","overlap":"skip","arguments":[],"cwd":"/fixture","schedule":{"kind":"interval","seconds":60,"run_at_load":false},"launch":{"kind":"direct","program":"/fixture/release/bin/worker","sha256":"0000000000000000000000000000000000000000000000000000000000000000"},"environment":{},"output":{"stdout":"/fixture/out","stderr":"/fixture/err"}}"#;
    let mut manifest: Manifest = serde_json::from_str(source)?;
    assert_eq!(
        manifest.digest()?,
        hex::encode(Sha256::digest(source.as_bytes()))
    );
    assert_eq!(serde_json::to_string(&manifest)?, source);
    manifest.schema_version = 2;
    assert_ne!(
        manifest.digest()?,
        hex::encode(Sha256::digest(source.as_bytes()))
    );
    assert_eq!(
        manifest.failure.on_abend,
        clockwork::api::AbendPolicy::HaltUntilApproved
    );
    Ok(())
}
