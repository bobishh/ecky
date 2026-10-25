//! Desktop-independent Cairn registry engine and versioned distribution contract.
mod archive;
pub use archive::{
    inspect_archive, sha256, valid_digest, valid_segment, PackageDependency, PackageDistribution,
    Release, MAX_ARCHIVE_BYTES, MAX_RELEASE_METADATA_BYTES,
};
use axum::{
    body::to_bytes,
    extract::{DefaultBodyLimit, Path as RoutePath, Query, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::json;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

type ApiResult<T> = Result<T, ApiError>;
struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}
fn internal(error: impl std::fmt::Display) -> ApiError {
    eprintln!("Cairn storage error: {error}");
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Registry storage failed; inspect server logs".into(),
    )
}
fn invalid(error: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, error.into())
}
#[derive(Clone)]
struct Registry {
    root: Arc<PathBuf>,
    token: Arc<String>,
    source_slots: Arc<tokio::sync::Semaphore>,
    publish_slots: Arc<tokio::sync::Semaphore>,
}
impl Registry {
    fn connection(&self) -> ApiResult<Connection> {
        let conn = Connection::open(self.root.join("registry.sqlite")).map_err(internal)?;
        conn.busy_timeout(Duration::from_secs(10))
            .map_err(internal)?;
        Ok(conn)
    }
    fn authorize(&self, headers: &HeaderMap) -> ApiResult<()> {
        let received = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        // Compare all bytes for equal-length tokens; no credential appears in error responses or logs.
        let diff = received
            .as_bytes()
            .iter()
            .zip(self.token.as_bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b));
        if received.len() != self.token.len() || diff != 0 {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "Valid publish credential required".into(),
            ));
        }
        Ok(())
    }
}
async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> ApiResult<T> + Send + 'static,
) -> ApiResult<T> {
    tokio::task::spawn_blocking(job).await.map_err(internal)?
}

pub fn router(data_dir: &Path, publish_token: &str) -> Result<Router, String> {
    router_with_assets(data_dir, publish_token, None)
}

pub fn router_with_assets(
    data_dir: &Path,
    publish_token: &str,
    assets: Option<&Path>,
) -> Result<Router, String> {
    if publish_token.len() < 24 {
        return Err("Publish token must contain at least 24 bytes".into());
    }
    std::fs::create_dir_all(data_dir.join("blobs")).map_err(|e| e.to_string())?;
    let state = Registry {
        root: Arc::new(data_dir.to_path_buf()),
        token: Arc::new(publish_token.to_string()),
        source_slots: Arc::new(tokio::sync::Semaphore::new(4)),
        publish_slots: Arc::new(tokio::sync::Semaphore::new(1)),
    };
    let conn = state.connection().map_err(|e| e.1)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
        CREATE TABLE IF NOT EXISTS releases (package_id TEXT NOT NULL, version TEXT NOT NULL, archive_digest TEXT NOT NULL, yanked INTEGER NOT NULL DEFAULT 0, metadata TEXT NOT NULL, search_text TEXT NOT NULL, PRIMARY KEY(package_id, version));
        CREATE INDEX IF NOT EXISTS release_digest ON releases(archive_digest);
        CREATE VIRTUAL TABLE IF NOT EXISTS release_search USING fts5(package_id, version UNINDEXED, search_text, tokenize='unicode61');").map_err(|e| e.to_string())?;
    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"status":"ok","protocolVersion":1})) }),
        )
        .route("/v1/search", get(search))
        .route("/v1/packages/{package}", get(info))
        .route("/v1/packages/{package}/{version}", get(exact_release))
        .route(
            "/v1/packages/{package}/{version}/components/{component}",
            get(inspect_component),
        )
        .route(
            "/install.sh",
            get(|| async {
                (
                    [("content-type", "text/x-shellscript; charset=utf-8")],
                    include_str!("../assets/install.sh"),
                )
            }),
        )
        .nest_service(
            "/cli",
            tower_http::services::ServeDir::new(data_dir.join("cli")),
        )
        .route("/v1/blobs/{digest}", get(download))
        .route("/v1/releases", post(publish))
        .route("/v1/packages/{package}/{version}/yank", post(yank))
        .layer(DefaultBodyLimit::max(MAX_ARCHIVE_BYTES))
        .with_state(state);
    Ok(if let Some(assets) = assets {
        app.fallback_service(tower_http::services::ServeDir::new(assets))
    } else {
        app
    })
}

#[derive(Deserialize, Default)]
struct Search {
    #[serde(default)]
    q: String,
    limit: Option<usize>,
}
fn conditional(headers: &HeaderMap, body: &serde_json::Value) -> ApiResult<Response> {
    let bytes = serde_json::to_vec(body).map_err(internal)?;
    let etag = format!("\"{}\"", sha256(&bytes));
    let status =
        if headers.get("if-none-match").and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
            StatusCode::NOT_MODIFIED
        } else {
            StatusCode::OK
        };
    let body = if status == StatusCode::NOT_MODIFIED {
        Vec::new()
    } else {
        bytes
    };
    Ok((
        status,
        [
            ("etag", etag),
            ("content-type", "application/json".into()),
            ("cache-control", "public, max-age=0, must-revalidate".into()),
        ],
        body,
    )
        .into_response())
}
async fn search(
    State(state): State<Registry>,
    Query(request): Query<Search>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let body = blocking(move || {
        if request.q.len() > 512 { return Err(invalid("Search query exceeds 512 bytes")); }
        let conn = state.connection()?;
        let query = request.q.split_whitespace().map(|word| format!("\"{}\"", word.replace('"', "\"\""))).collect::<Vec<_>>().join(" OR ");
        let limit = request.limit.unwrap_or(20).clamp(1,24) as i64;
        let mut releases = Vec::new();
        let (sql, value) = if query.is_empty() { ("SELECT metadata FROM releases WHERE yanked=0 ORDER BY package_id, version LIMIT ?2", "".to_string()) } else { ("SELECT r.metadata FROM release_search s JOIN releases r ON r.package_id=s.package_id AND r.version=s.version WHERE release_search MATCH ?1 AND r.yanked=0 ORDER BY bm25(release_search), r.package_id, r.version LIMIT ?2",query) };
        // Both statements bind the limit at index 2; the empty-query statement has an unused index 1.
        let mut stmt = conn.prepare(sql).map_err(internal)?;
        for row in stmt.query_map(params![value,limit], |row| row.get::<_,String>(0)).map_err(internal)? { releases.push(serde_json::from_str::<Release>(&row.map_err(internal)?).map_err(internal)?); }
        Ok(json!({"releases":releases}))
    }).await?;
    conditional(&headers, &body)
}
#[derive(Deserialize, Default)]
struct ReleasePage {
    #[serde(default)]
    offset: usize,
}
async fn info(
    State(state): State<Registry>,
    RoutePath(package): RoutePath<String>,
    Query(page): Query<ReleasePage>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    if !valid_segment(&package) || page.offset > 1_000_000 {
        return Err(invalid("Invalid package ID or metadata offset"));
    }
    let body = blocking(move || {
        let conn = state.connection()?;
        let mut stmt = conn.prepare("SELECT metadata FROM releases WHERE package_id=?1 ORDER BY version LIMIT 25 OFFSET ?2").map_err(internal)?;
        let mut releases = stmt.query_map(params![package,page.offset as i64], |row| row.get::<_, String>(0))
            .map_err(internal)?.map(|row| serde_json::from_str::<Release>(&row.map_err(internal)?).map_err(internal))
            .collect::<ApiResult<Vec<_>>>()?;
        if releases.is_empty() && page.offset == 0 {
            return Err(ApiError(StatusCode::NOT_FOUND, "Package not found".into()));
        }
        let next=if releases.len()>24 {releases.truncate(24);Some(page.offset+24)}else{None};
        Ok(json!({"releases":releases,"nextOffset":next}))
    }).await?;
    conditional(&headers, &body)
}
async fn exact_release(
    State(state): State<Registry>,
    RoutePath((package, version)): RoutePath<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    if !valid_segment(&package) || !valid_segment(&version) {
        return Err(invalid("Invalid release coordinate"));
    }
    let body = blocking(move || {
        let conn = state.connection()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT metadata FROM releases WHERE package_id=?1 AND version=?2",
                params![package, version],
                |row| row.get(0),
            )
            .optional()
            .map_err(internal)?;
        let release: Release = serde_json::from_str(
            &raw.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Release not found".into()))?,
        )
        .map_err(internal)?;
        Ok(json!({"releases":[release]}))
    })
    .await?;
    conditional(&headers, &body)
}
async fn publish(State(state): State<Registry>, request: Request) -> ApiResult<Response> {
    state.authorize(request.headers())?;
    let permit = state
        .publish_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Another publication is in progress; retry shortly".into(),
            )
        })?;
    let body = tokio::time::timeout(
        Duration::from_secs(30),
        to_bytes(request.into_body(), MAX_ARCHIVE_BYTES),
    )
    .await
    .map_err(|_| ApiError(StatusCode::REQUEST_TIMEOUT, "Upload timed out".into()))?
    .map_err(|_| {
        ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Archive exceeds 32 MiB or upload failed".into(),
        )
    })?;
    let (created, release) = blocking(move || {
        let _permit = permit;
        let release = inspect_archive(&body).map_err(invalid)?;
        let mut conn = state.connection()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT metadata FROM releases WHERE package_id=?1 AND version=?2",
                params![release.package_id, release.version],
                |row| row.get(0),
            )
            .optional()
            .map_err(internal)?;
        if let Some(existing) = existing {
            let existing: Release = serde_json::from_str(&existing).map_err(internal)?;
            if existing.archive_digest != release.archive_digest {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "Coordinate already contains different immutable archive bytes".into(),
                ));
            }
            return Ok((false, existing));
        }
        // A final blob exists before its metadata is visible. Interrupted staging leaves only unreferenced bytes.
        let final_path = state.root.join("blobs").join(&release.archive_digest[7..]);
        let stage = state
            .root
            .join("blobs")
            .join(format!(".stage-{}", uuid::Uuid::new_v4()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)
            .map_err(internal)?;
        file.write_all(&body)
            .and_then(|_| file.sync_all())
            .map_err(internal)?;
        std::fs::rename(&stage, &final_path).map_err(internal)?;
        std::fs::File::open(state.root.join("blobs"))
            .and_then(|dir| dir.sync_all())
            .map_err(internal)?;
        let metadata = serde_json::to_string(&release).map_err(internal)?;
        let text = format!(
            "{} {} {}",
            release.package_id, release.display_name, release.header
        );
        tx.execute(
            "INSERT INTO releases VALUES (?1,?2,?3,0,?4,?5)",
            params![
                release.package_id,
                release.version,
                release.archive_digest,
                metadata,
                text
            ],
        )
        .map_err(internal)?;
        tx.execute(
            "INSERT INTO release_search VALUES (?1,?2,?3)",
            params![release.package_id, release.version, text],
        )
        .map_err(internal)?;
        tx.commit().map_err(internal)?;
        Ok((true, release))
    })
    .await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(release),
    )
        .into_response())
}
async fn download(
    State(state): State<Registry>,
    RoutePath(digest): RoutePath<String>,
) -> ApiResult<Response> {
    if !valid_digest(&digest) {
        return Err(invalid("Invalid archive digest"));
    }
    let bytes = blocking(move || {
        let conn = state.connection()?;
        let exists = conn
            .query_row(
                "SELECT 1 FROM releases WHERE archive_digest=?1 LIMIT 1",
                [&digest],
                |_| Ok(()),
            )
            .optional()
            .map_err(internal)?
            .is_some();
        if !exists {
            return Err(ApiError(StatusCode::NOT_FOUND, "Archive not found".into()));
        }
        let bytes = std::fs::read(state.root.join("blobs").join(&digest[7..])).map_err(internal)?;
        if sha256(&bytes) != digest {
            return Err(internal("Stored archive integrity mismatch"));
        }
        Ok(bytes)
    })
    .await?;
    Ok((
        [
            ("content-type", "application/octet-stream"),
            ("cache-control", "public, max-age=31536000, immutable"),
        ],
        bytes,
    )
        .into_response())
}
async fn yank(
    State(state): State<Registry>,
    RoutePath((package, version)): RoutePath<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Json<Release>> {
    state.authorize(&headers)?;
    blocking(move || {
        let mut conn = state.connection()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        let raw: Option<String> = tx
            .query_row(
                "SELECT metadata FROM releases WHERE package_id=?1 AND version=?2",
                params![package, version],
                |row| row.get(0),
            )
            .optional()
            .map_err(internal)?;
        let mut release: Release = serde_json::from_str(
            &raw.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Release not found".into()))?,
        )
        .map_err(internal)?;
        release.yanked = true;
        tx.execute(
            "UPDATE releases SET yanked=1, metadata=?3 WHERE package_id=?1 AND version=?2",
            params![
                package,
                version,
                serde_json::to_string(&release).map_err(internal)?
            ],
        )
        .map_err(internal)?;
        tx.commit().map_err(internal)?;
        Ok(Json(release))
    })
    .await
}

async fn inspect_component(
    State(state): State<Registry>,
    RoutePath((package, version, component)): RoutePath<(String, String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    if !valid_segment(&package) || !valid_segment(&version) || !valid_segment(&component) {
        return Err(invalid("Invalid component coordinate"));
    }
    let permit = state
        .source_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(internal)?;
    blocking(move|| {
        let _permit = permit;
        let conn=state.connection()?;
        let raw:Option<String>=conn.query_row("SELECT metadata FROM releases WHERE package_id=?1 AND version=?2",params![package,version],|row|row.get(0)).optional().map_err(internal)?;
        let release:Release=serde_json::from_str(&raw.ok_or_else(||ApiError(StatusCode::NOT_FOUND,"Release not found".into()))?).map_err(internal)?;
        let bytes=std::fs::read(state.root.join("blobs").join(&release.archive_digest[7..])).map_err(internal)?;
        if sha256(&bytes)!=release.archive_digest {return Err(internal("Stored archive integrity mismatch"));}
        let source=archive::component_source(&bytes,&component).map_err(invalid)?;
        Ok(Json(json!({"packageId":release.package_id,"version":release.version,"componentId":component,"archiveDigest":release.archive_digest,"packageDigest":release.package_digest,"license":release.distribution.license,"source":source,"notice":"Binary assets or source larger than 512 KiB can be inspected after CLI installation."})))
    }).await
}
