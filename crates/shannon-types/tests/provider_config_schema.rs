use shannon_types::provider_config::*;

#[test]
fn provider_kind_serializes_to_lowercase_kebab() {
    let kind = ProviderKind::OpenAiCompatible;
    let json = serde_json::to_string(&kind).unwrap();
    assert_eq!(json, "\"openai-compatible\"");
}

#[test]
fn scope_roundtrips() {
    let s = Scope::Project;
    let json = serde_json::to_string(&s).unwrap();
    let back: Scope = serde_json::from_str(&json).unwrap();
    assert_eq!(back, Scope::Project);
}

#[test]
fn model_source_default_is_user_declared() {
    let s = ModelSource::default();
    assert_eq!(s, ModelSource::UserDeclared);
}

#[test]
fn credential_env_serializes_with_backend_tag() {
    let c = CredentialRef::Env {
        var: "SHANNON_GLM_API_KEY".into(),
    };
    let json = serde_json::to_value(&c).unwrap();
    assert_eq!(json["backend"], "env");
    assert_eq!(json["var"], "SHANNON_GLM_API_KEY");
}

#[test]
fn credential_keyring_roundtrips() {
    let c = CredentialRef::Keyring {
        service: "shannon".into(),
        account: "glm".into(),
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: CredentialRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back, c);
}

#[test]
fn credential_ephemeral_has_no_secret_fields() {
    let c = CredentialRef::Ephemeral;
    let json = serde_json::to_string(&c).unwrap();
    // Ephemeral 绝不可携带任何可序列化的密钥材料
    assert_eq!(json, r#"{"backend":"ephemeral"}"#);
}
