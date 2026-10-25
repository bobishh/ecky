use ecky_cad_lib::contracts::{ComponentDependencyLock, ComponentPackage};
use serde_json::json;

#[test]
fn given_package_distribution_when_round_tripped_then_exact_dependencies_and_license_survive() {
    let value = json!({"schemaVersion":1,"packageId":"cairn.stones","version":"1.0.0","displayName":"Stone collection","visibility":"source","components":[],"cairn":{"license":"MIT","repository":"https://example.com/stones","dependencies":[{"packageId":"cairn.base","version":"1.0.0"}]}});
    let package: ComponentPackage = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(package).unwrap()["cairn"],
        value["cairn"]
    );
}

#[test]
fn given_registry_lock_when_mirrored_then_origin_integrity_and_graph_survive_canonically() {
    let value = json!({"schemaVersion":1,"dependencies":[],"cairn":{"registry":"https://cairn.example.com","roots":[{"packageId":"cairn.stones","version":"1.0.0"}],"releases":[{"packageId":"cairn.stones","version":"1.0.0","packageDigest":format!("sha256:{}","a".repeat(64)),"archiveDigest":format!("sha256:{}","b".repeat(64)),"dependencies":[]}]}});
    let lock: ComponentDependencyLock = serde_json::from_value(value.clone()).unwrap();
    let path = std::env::temp_dir().join(format!("cairn-lock-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    ecky_cad_lib::project_mirror::write_project_lock(&path, &lock).unwrap();
    let recovered = ecky_cad_lib::project_mirror::read_project_lock(&path)
        .unwrap()
        .unwrap();
    std::fs::remove_dir_all(path).unwrap();
    assert_eq!(
        serde_json::to_value(recovered).unwrap()["cairn"],
        value["cairn"]
    );
}

#[test]
fn given_self_hosted_registry_when_config_is_saved_then_json_and_data_only_edn_preserve_it() {
    let value = json!({"engines":[],"selectedEngineId":"","cairn":{"registryUrl":"https://hub.ecky-cad.com"}});
    let config: ecky_cad_lib::contracts::Config = serde_json::from_value(value).unwrap();
    assert_eq!(
        serde_json::to_value(&config).unwrap()["cairn"]["registryUrl"],
        "https://hub.ecky-cad.com"
    );
    let encoded = ecky_cad_lib::contracts::encode_config(&config).unwrap();
    let decoded = ecky_cad_lib::contracts::decode_config(&encoded).unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap()["cairn"]["registryUrl"],
        "https://hub.ecky-cad.com"
    );
}

proptest::proptest! {
    #[test]
    fn canonical_graph_identity_survives_permutation_and_edn_round_trip(
        suffix in "[a-z][a-z0-9]{0,12}",
        major in 0u16..100, minor in 0u16..100, patch in 0u16..100,
        order in 0usize..100,
        reverse in proptest::bool::ANY,
    ) {
        use ecky_cad_lib::contracts::{CairnDependency,CairnLock,CairnLockedRelease};
        let version=format!("{major}.{minor}.{patch}");
        let leaf_a=CairnDependency {package_id:format!("leaf-{suffix}"),version:version.clone()};
        let leaf_b=CairnDependency {package_id:format!("support-{suffix}"),version:version.clone()};
        let root=CairnDependency {package_id:format!("kit-{suffix}"),version:version.clone()};
        let release=|dependency:&CairnDependency,dependencies:Vec<CairnDependency>|CairnLockedRelease {
            package_id:dependency.package_id.clone(),version:dependency.version.clone(),
            package_digest:format!("sha256:{}","a".repeat(64)),archive_digest:format!("sha256:{}","b".repeat(64)),dependencies,
        };
        let graph=CairnLock {registry:"https://hub.example.test".into(),roots:vec![root.clone(),leaf_a.clone()],releases:vec![release(&root,vec![leaf_a.clone(),leaf_b.clone()]),release(&leaf_a,vec![]),release(&leaf_b,vec![])]};
        let lock=ComponentDependencyLock {schema_version:1,dependencies:vec![],cairn:Some(graph.clone())};
        lock.validate().unwrap();
        let mut shuffled=graph;
        shuffled.roots.rotate_left(order%2);shuffled.releases.rotate_left(order%3);
        if reverse {shuffled.roots.reverse();shuffled.releases.reverse();}
        for release in &mut shuffled.releases {if reverse {release.dependencies.reverse();}}
        let shuffled=ComponentDependencyLock {cairn:Some(shuffled),..lock.clone()};
        proptest::prop_assert_eq!(lock.canonical_bytes().unwrap(),shuffled.canonical_bytes().unwrap());
        let path=std::env::temp_dir().join(format!("cairn-property-{}",uuid::Uuid::new_v4()));std::fs::create_dir_all(&path).unwrap();
        ecky_cad_lib::project_mirror::write_project_lock(&path,&shuffled).unwrap();
        let recovered=ecky_cad_lib::project_mirror::read_project_lock(&path).unwrap().unwrap();
        std::fs::remove_dir_all(path).unwrap();
        proptest::prop_assert_eq!(recovered,lock.canonical());
    }
}

#[test]
fn given_shared_dependency_tail_when_previsited_then_depth_limit_cannot_be_bypassed() {
    use ecky_cad_lib::contracts::{CairnDependency, CairnLock, CairnLockedRelease};
    let dependency = |id: usize| CairnDependency {
        package_id: format!("node-{id}"),
        version: "1.0.0".into(),
    };
    let graph = CairnLock {
        registry: "https://hub.example.test".into(),
        roots: vec![dependency(10), dependency(0)],
        releases: (0..20)
            .map(|id| CairnLockedRelease {
                package_id: dependency(id).package_id,
                version: "1.0.0".into(),
                package_digest: format!("sha256:{}", "a".repeat(64)),
                archive_digest: format!("sha256:{}", "b".repeat(64)),
                dependencies: if id < 19 {
                    vec![dependency(id + 1)]
                } else {
                    vec![]
                },
            })
            .collect(),
    };
    assert!(
        graph.validate().is_err(),
        "shared-tail caching must retain the full root-to-leaf depth"
    );
    let mut reordered = graph;
    reordered.roots.reverse();
    assert!(reordered.validate().is_err());
}

#[test]
fn given_invalid_dependency_graph_when_validated_then_cycles_conflicts_and_missing_releases_are_rejected(
) {
    use ecky_cad_lib::contracts::CairnLock;
    let release = |id: &str, dependencies: serde_json::Value| json!({"packageId":id,"version":"1.0.0","packageDigest":format!("sha256:{}","a".repeat(64)),"archiveDigest":format!("sha256:{}","b".repeat(64)),"dependencies":dependencies});
    let a = json!({"packageId":"a","version":"1.0.0"});
    let b = json!({"packageId":"b","version":"1.0.0"});
    for releases in [
        vec![
            release("a", json!([b.clone()])),
            release("b", json!([a.clone()])),
        ],
        vec![release("a", json!([b.clone()]))],
        vec![release("a", json!([])), release("a", json!([]))],
        vec![release("a", json!([])), release("b", json!([]))],
    ] {
        let graph: CairnLock = serde_json::from_value(
            json!({"registry":"https://hub.example.test","roots":[a.clone()],"releases":releases}),
        )
        .unwrap();
        assert!(graph.validate().is_err());
    }
}
