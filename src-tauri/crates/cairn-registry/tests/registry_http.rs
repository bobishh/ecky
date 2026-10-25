use axum::http::StatusCode;
use base64::Engine;
use serde_json::json;
use std::{io::Write, path::PathBuf};
const TOKEN: &str = "test-only-publish-token-long-enough";

struct Registry {
    root: PathBuf,
    url: String,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Registry {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
async fn start() -> Registry {
    let root = std::env::temp_dir().join(format!("cairn-http-{}", uuid::Uuid::new_v4()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = cairn_registry::router(&root, TOKEN).unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Registry { root, url, server }
}
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        zip.start_file(*name, zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(body).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn archive(size: u32) -> Vec<u8> {
    archive_at_version(size, "1.0.0")
}
fn archive_at_version(size: u32, version: &str) -> Vec<u8> {
    let manifest = json!({"schemaVersion":1,"packageId":"cairn.stones","version":version,"displayName":"Low poly stones","visibility":"source","tags":["stone"],"components":[{"componentId":"stone","version":"1.0.0","displayName":"Stone","sourceRef":"stone.ecky"}]});
    let manifest = serde_json::to_vec(&manifest).unwrap();
    let source = format!("(define-component stone () (box {size} 2 1))");
    let payload = zip(&[
        ("ecky-package.json", &manifest),
        ("stone.ecky", source.as_bytes()),
    ]);
    let encoded = base64::engine::general_purpose::STANDARD.encode(payload);
    zip(&[
        ("ecky-header.json", &manifest),
        ("ecky-payload.b64", encoded.as_bytes()),
    ])
}

#[tokio::test]
async fn given_registry_when_publishing_and_yanking_then_immutable_release_and_locked_download_survive(
) {
    let registry = start().await;
    let client = reqwest::Client::new();
    let endpoint = format!("{}/v1/releases", registry.url);
    let bytes = archive(3);
    assert_eq!(
        client
            .post(&endpoint)
            .body(bytes.clone())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let published = client
        .post(&endpoint)
        .bearer_auth(TOKEN)
        .body(bytes.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(published.status(), StatusCode::CREATED);
    let release: serde_json::Value = published.json().await.unwrap();
    assert_eq!(release["packageId"], "cairn.stones");
    let inspected: serde_json::Value = client
        .get(format!(
            "{}/v1/packages/cairn.stones/1.0.0/components/stone",
            registry.url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        inspected["source"],
        "(define-component stone () (box 3 2 1))"
    );
    assert_eq!(inspected["archiveDigest"], release["archiveDigest"]);

    let digest = release["archiveDigest"].as_str().unwrap();
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(TOKEN)
            .body(bytes.clone())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(TOKEN)
            .body(archive(4))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let info = format!("{}/v1/packages/cairn.stones", registry.url);
    let response = client.get(&info).send().await.unwrap();
    let etag = response.headers().get("etag").unwrap().clone();
    assert_eq!(
        client
            .get(&info)
            .header("if-none-match", etag)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_MODIFIED
    );
    let search: serde_json::Value = client
        .get(format!("{}/v1/search?q=stone", registry.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(search["releases"].as_array().unwrap().len(), 1);
    assert_eq!(
        client
            .post(format!(
                "{}/v1/packages/cairn.stones/1.0.0/yank",
                registry.url
            ))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let search: serde_json::Value = client
        .get(format!("{}/v1/search?q=stone", registry.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(search["releases"].as_array().unwrap().is_empty());
    let download = client
        .get(format!("{}/v1/blobs/{digest}", registry.url))
        .send()
        .await
        .unwrap();
    assert_eq!(download.bytes().await.unwrap().as_ref(), bytes.as_slice());
}

#[tokio::test]
async fn given_invalid_archives_when_published_then_metadata_is_never_advertised() {
    let registry = start().await;
    let client = reqwest::Client::new();
    for body in [b"broken".to_vec(), zip(&[("../escape", b"payload")])] {
        assert_eq!(
            client
                .post(format!("{}/v1/releases", registry.url))
                .bearer_auth(TOKEN)
                .body(body)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let response: serde_json::Value = client
        .get(format!("{}/v1/search", registry.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(response["releases"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn given_unauthorized_oversized_publish_when_requested_then_authentication_precedes_body_allocation(
) {
    let registry = start().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/releases", registry.url))
        .body(vec![0u8; cairn_registry::MAX_ARCHIVE_BYTES + 1])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn given_many_releases_when_reading_metadata_then_exact_lookup_and_bounded_pages_preserve_access(
) {
    let registry = start().await;
    let client = reqwest::Client::new();
    for patch in 0..26 {
        let response = client
            .post(format!("{}/v1/releases", registry.url))
            .bearer_auth(TOKEN)
            .body(archive_at_version(3, &format!("1.0.{patch:02}")))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }
    let response = client
        .get(format!("{}/v1/packages/cairn.stones/1.0.25", registry.url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let exact: serde_json::Value = response.json().await.unwrap();
    assert_eq!(exact["releases"][0]["version"], "1.0.25");
    let page: serde_json::Value = client
        .get(format!("{}/v1/packages/cairn.stones", registry.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page["releases"].as_array().unwrap().len(), 24);
    assert_eq!(page["nextOffset"], 24);
    let next: serde_json::Value = client
        .get(format!(
            "{}/v1/packages/cairn.stones?offset=24",
            registry.url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(next["releases"].as_array().unwrap().len(), 2);
    assert!(next["nextOffset"].is_null());
}

#[tokio::test]
async fn given_authenticated_upload_when_body_or_public_metadata_exceeds_limits_then_no_release_is_advertised(
) {
    let registry = start().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/releases", registry.url))
        .bearer_auth(TOKEN)
        .body(vec![0; cairn_registry::MAX_ARCHIVE_BYTES + 1])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let manifest = json!({"schemaVersion":1,"packageId":"too-big","version":"1.0.0","displayName":"x".repeat(cairn_registry::MAX_RELEASE_METADATA_BYTES),"visibility":"source","components":[]});
    let raw = serde_json::to_vec(&manifest).unwrap();
    let bytes = zip(&[
        ("ecky-header.json", &raw),
        (
            "ecky-payload.b64",
            base64::engine::general_purpose::STANDARD
                .encode(zip(&[("ecky-package.json", &raw)]))
                .as_bytes(),
        ),
    ]);
    let response = client
        .post(format!("{}/v1/releases", registry.url))
        .bearer_auth(TOKEN)
        .body(bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let search: serde_json::Value = client
        .get(format!("{}/v1/search", registry.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(search["releases"].as_array().unwrap().is_empty());
}
