use clockwork::api::{BindingRecord, Failure, Manifest, Success, decode};

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

#[test]
fn runtime_definitions_keep_exact_release_identity_and_closed_paths()
-> Result<(), Box<dyn std::error::Error>> {
    use clockwork::api::LaunchImage;
    let release = "/fixture/install/releases/6ce29a62-15b0-4e71-b5c0-4c5db83bb38d";
    let source = serde_json::json!({
        "schema_version":2,"key":"example/worker",
        "release_id":"6ce29a62-15b0-4e71-b5c0-4c5db83bb38d","release_root":release,
        "authority":"current-user-background","overlap":"skip","arguments":[],
        "cwd":"/fixture/state","schedule":{"kind":"interval","seconds":60,"run_at_load":false},
        "launch":{"kind":"direct","program":format!("{release}/bin/worker"),"sha256":"a".repeat(64)},
        "environment":{},"output":{"stdout":"/fixture/out","stderr":"/fixture/err"}
    });
    let mut manifest: Manifest = serde_json::from_value(source)?;
    let old = manifest.clone();
    let old_digest = old.digest()?;
    manifest.use_runtime_paths()?;
    assert_eq!(manifest.schema_version, 3);
    assert_eq!(manifest.release_root, old.release_root);
    assert_eq!(manifest.release_id, old.release_id);
    assert!(
        matches!(&manifest.launch, LaunchImage::Direct { program, sha256 }
        if program == "/fixture/install/runtime/bin/worker" && sha256 == &"a".repeat(64))
    );
    assert_ne!(manifest.digest()?, old_digest);
    let runtime_digest = manifest.digest()?;
    manifest.use_runtime_paths()?;
    assert_eq!(manifest.digest()?, runtime_digest);
    assert_eq!(Manifest::from_toml(&manifest.to_toml()?)?, manifest);
    assert_eq!(Manifest::from_toml(&old.to_toml()?)?, old);

    for program in [
        "/foreign/bin/worker".to_owned(),
        format!("{release}/../other/bin/worker"),
        "/fixture/install/runtime/../foreign".to_owned(),
    ] {
        let mut invalid = old.clone();
        invalid.launch = LaunchImage::Direct {
            program,
            sha256: "a".repeat(64),
        };
        assert!(invalid.use_runtime_paths().is_err());
    }

    manifest = old;
    manifest.launch = LaunchImage::Interpreted {
        interpreter: "/bin/sh".into(),
        interpreter_sha256: "b".repeat(64),
        script: format!("{release}/bin/worker"),
        script_sha256: "c".repeat(64),
    };
    manifest.use_runtime_paths()?;
    assert!(
        matches!(manifest.launch, LaunchImage::Interpreted { interpreter, interpreter_sha256, script, script_sha256 }
        if interpreter == "/bin/sh" && interpreter_sha256 == "b".repeat(64)
        && script == "/fixture/install/runtime/bin/worker" && script_sha256 == "c".repeat(64))
    );
    Ok(())
}

#[test]
fn runtime_mapping_requires_the_owned_installation_shape() {
    use clockwork::api::runtime_root;
    use std::path::Path;
    for root in [
        "/fixture/releases/6ce29a62-15b0-4e71-b5c0-4c5db83bb38d",
        "/fixture/install/releases/not-a-release",
        "/fixture/install/releases/../6ce29a62-15b0-4e71-b5c0-4c5db83bb38d",
        "install/releases/6ce29a62-15b0-4e71-b5c0-4c5db83bb38d",
    ] {
        assert!(runtime_root(Path::new(root)).is_err());
    }
}
