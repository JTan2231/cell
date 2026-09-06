use krisis_api::account::{parse, render};

#[test]
fn retained_canonical_bytes_survive_provider_codec() -> Result<(), Box<dyn std::error::Error>> {
    let retained = include_str!("fixtures/account-v1.md");
    let account = parse(retained, "d_0123456789abcdef0123")?;
    assert_eq!(render(&account)?, retained);
    assert!(parse(retained, "different-key").is_err());
    assert!(
        parse(
            &retained.replace("\"schema_version\": 1", "\"schema_version\": 2"),
            "d_0123456789abcdef0123"
        )
        .is_err()
    );
    assert!(
        parse(
            &retained.replace("\"end\": 26", "\"end\": 4"),
            "d_0123456789abcdef0123"
        )
        .is_err()
    );
    Ok(())
}
