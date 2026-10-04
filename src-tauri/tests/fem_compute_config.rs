use ecky_cad_lib::contracts::{
    decode_config, encode_config, Config, EngineKind, FemComputeConfig, FemComputeQuality,
    GeometryBackend, McpConfig, ProviderModels, SourceLanguage, VoiceConfig,
};

#[test]
fn fem_compute_policy_round_trips_through_canonical_edn_value() {
    let config = Config {
        engines: vec![],
        selected_engine_id: String::new(),
        freecad_cmd: String::new(),
        cad_text_font_path: String::new(),
        freecad_library_roots: vec![],
        assets: vec![],
        microwave: None,
        voice: VoiceConfig::default(),
        mcp: McpConfig::default(),
        fem_compute: FemComputeConfig {
            quality: FemComputeQuality::Fine,
            maximum_wall_time_minutes: 45,
            maximum_memory_mib: 12_288,
            thread_count: 8,
        },
        has_seen_onboarding: true,
        connection_type: None,
        provider_models: ProviderModels::default(),
        jev_classifier: Default::default(),
        default_engine_kind: EngineKind::EckyIrV0,
        default_source_language: SourceLanguage::EckyIrV0,
        default_geometry_backend: GeometryBackend::EckyRust,
        max_generation_attempts: 3,
        max_verify_attempts: 2,
        projects_root: None,
    };

    let encoded = encode_config(&config).unwrap();
    let decoded = decode_config(&encoded).unwrap();

    assert_eq!(decoded, config);
    assert_eq!(decoded.fem_compute.topology_iteration_limit(), 240);
    assert_eq!(decoded.fem_compute.maximum_wall_time_ms(), 2_700_000);
    assert_eq!(
        decoded.fem_compute.maximum_working_memory_bytes(),
        12_884_901_888
    );
    assert_eq!(decoded.fem_compute.maximum_fem_elements(), 1_000_000);
    assert_eq!(decoded.fem_compute.maximum_fem_nodes(), 1_000_000);
    assert_eq!(decoded.fem_compute.maximum_fem_dofs(), 3_000_000);
    assert_eq!(
        decoded.fem_compute.maximum_fem_sparse_nonzeros(),
        192_000_000
    );
    assert_eq!(
        decoded.fem_compute.maximum_fem_result_bytes(),
        512 * 1024 * 1024
    );
}
