use serde_json::json;
use std::path::{Path, PathBuf};
struct Project(PathBuf);
impl ecky_cad_lib::models::PathResolver for Project {
    fn app_data_dir(&self) -> PathBuf {
        self.0.join("data")
    }
    fn app_config_dir(&self) -> PathBuf {
        self.0.join("data/config")
    }
    fn resource_path(&self, _: &str) -> Option<PathBuf> {
        None
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn command(
    root: &Path,
    project: &Path,
    registry: &str,
    args: &[&str],
) -> Result<serde_json::Value, Box<ecky_cad_lib::contracts::AppError>> {
    let mut arguments = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    arguments.extend([
        "--project".into(),
        project.display().to_string(),
        "--data-dir".into(),
        root.join("data").display().to_string(),
        "--registry".into(),
        registry.into(),
        "--json".into(),
    ]);
    ecky_cad_lib::cairn::run(&arguments).await.map_err(Box::new)
}
#[tokio::test]
async fn given_published_collection_when_added_then_frozen_offline_install_preserves_exact_source_and_ignores_private_files(
) {
    let root = Project(std::env::temp_dir().join(format!("cairn-client-{}", uuid::Uuid::new_v4())));
    let package = root.0.join("package");
    let consumer = root.0.join("consumer");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::create_dir_all(&consumer).unwrap();
    let token = "test-only-client-publish-token-long-enough";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let registry = format!("http://{}", listener.local_addr().unwrap());
    let server = cairn_registry::router(&root.0.join("registry"), token).unwrap();
    let handle = tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
    command(&root.0, &package, &registry, &["init", "cairn.stones"])
        .await
        .unwrap();
    let manifest = package.join("ecky-package.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    value["components"] = json!([{"componentId":"stone","version":"0.1.0","displayName":"Stone","sourceRef":"stone.ecky"}]);
    value["cairn"] = json!({"license":"MIT","dependencies":[]});
    std::fs::write(&manifest, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    std::fs::write(
        package.join("stone.ecky"),
        "(define-component stone () (box 3 2 1))",
    )
    .unwrap();
    std::fs::write(package.join(".env"), "PRIVATE=secret").unwrap();
    let archive = root.0.join("stones.ecky-package");
    command(
        &root.0,
        &package,
        &registry,
        &["pack", "--out", archive.to_str().unwrap()],
    )
    .await
    .unwrap();
    let bytes = std::fs::read(&archive).unwrap();
    let release = cairn_registry::inspect_archive(&bytes).unwrap();
    let incomplete =
        ecky_cad_lib::component_package_runtime::payload_store_dir(&root, &release.package_digest)
            .unwrap();
    std::fs::create_dir_all(&incomplete).unwrap();
    std::fs::write(incomplete.join("interrupted.tmp"), "partial payload").unwrap();
    let payload =
        ecky_cad_lib::component_package_runtime::read_decoded_package_payload(&archive).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(payload)).unwrap();
    assert!(zip.by_name(".env").is_err());
    assert_eq!(
        reqwest::Client::new()
            .post(format!("{registry}/v1/releases"))
            .bearer_auth(token)
            .body(bytes)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::CREATED
    );
    command(&root.0, &consumer, &registry, &["init", "consumer"])
        .await
        .unwrap();
    command(
        &root.0,
        &consumer,
        &registry,
        &["add", "cairn.stones@0.1.0"],
    )
    .await
    .unwrap();
    assert!(!incomplete.join("interrupted.tmp").exists());
    assert!(incomplete.join("ecky-integrity.json").is_file());
    let lock = std::fs::read(consumer.join("ecky.lock.edn")).unwrap();
    let found = command(&root.0, &consumer, &registry, &["search", "stone"])
        .await
        .unwrap();
    assert!(found.to_string().contains("cairn.stones"));
    command(&root.0, &consumer, &registry, &["install", "--frozen"])
        .await
        .unwrap();
    handle.abort();
    command(
        &root.0,
        &consumer,
        &registry,
        &["install", "--frozen", "--offline"],
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(consumer.join("ecky.lock.edn")).unwrap(), lock);
    let cached = root
        .0
        .join("data/cairn-cache/archives")
        .join(format!("{}.ecky-package", &release.archive_digest[7..]));
    std::fs::write(&cached, "corrupt archive").unwrap();
    assert!(command(
        &root.0,
        &consumer,
        &registry,
        &["install", "--frozen", "--offline"]
    )
    .await
    .is_err());
    assert_eq!(std::fs::read(consumer.join("ecky.lock.edn")).unwrap(), lock);
    assert!(incomplete.join("ecky-integrity.json").is_file());
    std::fs::copy(&archive, cached).unwrap();
    let mut declared: serde_json::Value =
        serde_json::from_slice(&std::fs::read(consumer.join("ecky-package.json")).unwrap())
            .unwrap();
    declared["cairn"]["dependencies"][0]["version"] = json!("9.0.0");
    std::fs::write(
        consumer.join("ecky-package.json"),
        serde_json::to_vec_pretty(&declared).unwrap(),
    )
    .unwrap();
    assert!(command(
        &root.0,
        &consumer,
        &registry,
        &["install", "--frozen", "--offline"]
    )
    .await
    .is_err());
    assert_eq!(std::fs::read(consumer.join("ecky.lock.edn")).unwrap(), lock);
}
