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
