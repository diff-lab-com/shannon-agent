use shannon_types::provider_config::*;

#[test]
fn active_target_is_atomic_unit() {
    let t = ActiveTarget {
        provider_id: "glm".into(),
        model_id: "glm-4.6".into(),
        scope: Scope::Global,
    };
    let json = serde_json::to_value(&t).unwrap();
    assert_eq!(json["provider_id"], "glm");
    assert_eq!(json["model_id"], "glm-4.6");
    assert_eq!(json["scope"], "global");
}

#[test]
fn provider_quirks_minimal_and_optional() {
    let json = serde_json::to_string(&ProviderQuirks::default()).unwrap();
    // 默认 quirk 必须能省略所有可选字段
    assert!(json.contains("\"send_temperature\":true"));
}

#[test]
fn provider_profile_credential_is_credential_ref() {
    let p = ProviderProfile {
        id: "glm".into(),
        kind: ProviderKind::OpenAiCompatible,
        display_name: "Z.AI GLM".into(),
        base_url: "https://open.bigmodel.cn/api/paas/v4".into(),
        models_url: None,
        credential: CredentialRef::Env {
            var: "SHANNON_GLM_API_KEY".into(),
        },
        extra_headers: Default::default(),
        default_max_tokens: None,
        fallback_models: vec!["glm-4.6".into(), "glm-4.5-air".into()],
        quirks: ProviderQuirks::default(),
    };
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains(r#""backend":"env""#));
}

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

#[test]
fn v2_config_has_version_2_and_profiles() {
    let toml = r#"
version = 2
[profiles.default]
name = "default"
active_target = { provider_id = "glm", model_id = "glm-4.6", scope = "global" }
[[profiles.default.providers]]
id = "glm"
kind = "openai-compatible"
display_name = "Z.AI GLM"
base_url = "https://open.bigmodel.cn/api/paas/v4"
credential = { backend = "env", var = "SHANNON_GLM_API_KEY" }
"#;
    let cfg: ProviderModelConfig = toml::from_str(toml).unwrap();
    assert_eq!(cfg.version, 2);
    assert!(cfg.profiles.contains_key("default"));
}

#[test]
fn gateway_defaults_to_multiplex_off() {
    // B3 分期：默认 off → 字节级等同单 profile（profile_routes 被完全忽略）
    let toml = r#"version = 2
[profiles.default]
name = "default"
active_target = { provider_id = "glm", model_id = "glm-4.6", scope = "global" }
[[profiles.default.providers]]
id = "glm"
kind = "openai-compatible"
display_name = "GLM"
base_url = "https://x"
credential = { backend = "env", var = "SHANNON_GLM_API_KEY" }
"#;
    let cfg: ProviderModelConfig = toml::from_str(toml).unwrap();
    assert!(!cfg.gateway.multiplex_profiles); // 默认 off
    assert!(cfg.gateway.profile_routes.is_empty());
}

#[test]
fn route_specificity_session_beats_project_beats_tenant() {
    // 加权：session(8) > project(4) > tenant(2)；最具体者赢
    let r = ProfileRoute {
        name: "s".into(),
        tenant_id: Some("t".into()),
        project_path: Some("/p".into()),
        session_id: Some("s1".into()),
        client_id: None,
        profile: "most-specific".into(),
        enabled: true,
    };
    assert!(specificity_weight(&r) > 4);

    // Pin each weight independently
    let only_tenant = ProfileRoute {
        name: "t".into(),
        tenant_id: Some("t".into()),
        project_path: None,
        session_id: None,
        client_id: None,
        profile: "x".into(),
        enabled: true,
    };
    let only_project = ProfileRoute {
        name: "p".into(),
        tenant_id: None,
        project_path: Some("/p".into()),
        session_id: None,
        client_id: None,
        profile: "x".into(),
        enabled: true,
    };
    let only_session = ProfileRoute {
        name: "s".into(),
        tenant_id: None,
        project_path: None,
        session_id: Some("s1".into()),
        client_id: None,
        profile: "x".into(),
        enabled: true,
    };
    assert_eq!(specificity_weight(&only_tenant), 2);
    assert_eq!(specificity_weight(&only_project), 4);
    assert_eq!(specificity_weight(&only_session), 8);
    // And the comparative ordering the test name promises
    assert!(specificity_weight(&only_session) > specificity_weight(&only_project));
    assert!(specificity_weight(&only_project) > specificity_weight(&only_tenant));
}

#[test]
fn credential_scope_defaults_shared() {
    // C1 两层凭据解析：默认 Shared（沿用旧单 profile 语义）
    assert_eq!(CredentialScope::default(), CredentialScope::Shared);
}

#[test]
fn plaintext_api_key_in_provider_is_rejected_by_schema() {
    // 明文 key 不允许落 v2 结构（A1）——credential 必须是 CredentialRef；ProviderProfile 配置 deny_unknown_fields，
    // 故 `api_key` 显式被拒（而非静默丢弃），保证 A1「永不存明文」由「显式拒绝」强制。
    let bad = r#"{"version":2,"profiles":{"default":{"name":"default","active_target":{"provider_id":"x","model_id":"x","scope":"global"},"providers":[{"id":"x","kind":"openai-compatible","display_name":"x","base_url":"x","api_key":"sk-leak"}]}}}"#;
    let res: Result<ProviderModelConfig, _> = serde_json::from_str(bad);
    let err = res.unwrap_err().to_string();
    assert!(
        err.contains("api_key") || err.contains("unknown field"),
        "expected unknown-field rejection mentioning api_key, got: {err}"
    );
}
