use crate::contracts::{
    normalize_design_output, upgraded_or_default_genie_traits, AgentDraft, AppError,
    ArtifactBundle, DeletedMessage, DeletedThreadSummary, DeletedThreadsPage, DesignOutput,
    DesignParams, GenieTraits, Message, MessageRole, MessageStatus, ModelManifest, TargetLeaseInfo,
    Thread, ThreadMessagesPage, ThreadReference, ThreadStatus, UiSpec,
};
use rusqlite::{params, Connection, OptionalExtension, Result as SqlResult};
use serde::de::{DeserializeOwned, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};

#[derive(Debug, Clone)]
struct ThreadMessageRow {
    message: Message,
    deleted_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct LatestSuccessfulTarget {
    pub thread_id: String,
    pub message_id: String,
}

const PAYLOAD_READ_CHUNK_BYTES: usize = 256 * 1024;
const PAYLOAD_CODEC_VERSION: i64 = 1;
const PAYLOAD_CODEC_MAGIC: &[u8; 4] = b"EKP1";
const TOPOLOGY_CHUNK_ITEMS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadOwnerKind {
    Message,
    Draft,
}

impl PayloadOwnerKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Draft => "draft",
        }
    }

    fn table(self) -> &'static str {
        match self {
            Self::Message => "messages",
            Self::Draft => "agent_drafts",
        }
    }

    fn id_column(self) -> &'static str {
        match self {
            Self::Message => "id",
            Self::Draft => "preview_id",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadColumn {
    ArtifactBundle,
    ModelManifest,
}

impl PayloadColumn {
    fn as_str(self) -> &'static str {
        match self {
            Self::ArtifactBundle => "artifact_bundle",
            Self::ModelManifest => "model_manifest",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DenseField {
    Edge,
    Face,
    Selection,
}

impl DenseField {
    fn json_key(self) -> &'static str {
        match self {
            Self::Edge => "edgeTargets",
            Self::Face => "faceTargets",
            Self::Selection => "selectionTargets",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PayloadProjection {
    model_id: Option<String>,
    edge_count: usize,
    face_count: usize,
    selection_count: usize,
}

#[derive(Debug, Default)]
struct EncodedCadPayload {
    artifact_core: Option<Vec<u8>>,
    model_manifest_core: Option<Vec<u8>>,
    projection: PayloadProjection,
}

#[derive(Debug, Default)]
struct DenseIndexes {
    edge: Vec<u64>,
    face: Vec<u64>,
    selection: Vec<u64>,
}

#[derive(Debug, Default)]
struct JsonObjectProjection {
    core_json: String,
    edge_count: usize,
    face_count: usize,
    selection_count: usize,
    page_items: Vec<String>,
}

#[derive(Debug, Default)]
struct DenseArrayProjection {
    count: usize,
    page_items: Vec<String>,
}

struct DenseArraySeed {
    collect_page: bool,
    offset: usize,
    limit: usize,
}

impl<'de> DeserializeSeed<'de> for DenseArraySeed {
    type Value = DenseArrayProjection;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(DenseArrayVisitor {
            collect_page: self.collect_page,
            offset: self.offset,
            limit: self.limit,
        })
    }
}

struct DenseArrayVisitor {
    collect_page: bool,
    offset: usize,
    limit: usize,
}

impl<'de> Visitor<'de> for DenseArrayVisitor {
    type Value = DenseArrayProjection;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a dense topology array or null")
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(DenseArrayProjection::default())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(DenseArrayProjection::default())
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut result = DenseArrayProjection::default();
        loop {
            let collect = self.collect_page
                && result.count >= self.offset
                && result.count < self.offset.saturating_add(self.limit);
            let present = if collect {
                match sequence.next_element::<serde_json::Value>()? {
                    Some(value) => {
                        result
                            .page_items
                            .push(serde_json::to_string(&value).map_err(serde::de::Error::custom)?);
                        true
                    }
                    None => false,
                }
            } else {
                sequence.next_element::<IgnoredAny>()?.is_some()
            };
            if !present {
                break;
            }
            result.count += 1;
        }
        Ok(result)
    }
}

struct JsonObjectProjectionSeed {
    page_field: Option<DenseField>,
    offset: usize,
    limit: usize,
}

impl<'de> DeserializeSeed<'de> for JsonObjectProjectionSeed {
    type Value = JsonObjectProjection;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(JsonObjectProjectionVisitor {
            page_field: self.page_field,
            offset: self.offset,
            limit: self.limit,
        })
    }
}

struct JsonObjectProjectionVisitor {
    page_field: Option<DenseField>,
    offset: usize,
    limit: usize,
}

impl<'de> Visitor<'de> for JsonObjectProjectionVisitor {
    type Value = JsonObjectProjection;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON object payload")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut core = serde_json::Map::new();
        let mut result = JsonObjectProjection::default();
        while let Some(key) = map.next_key::<String>()? {
            let dense_field = [DenseField::Edge, DenseField::Face, DenseField::Selection]
                .into_iter()
                .find(|field| field.json_key() == key);
            if let Some(field) = dense_field {
                let dense = map.next_value_seed(DenseArraySeed {
                    collect_page: self.page_field == Some(field),
                    offset: self.offset,
                    limit: self.limit,
                })?;
                match field {
                    DenseField::Edge => result.edge_count = dense.count,
                    DenseField::Face => result.face_count = dense.count,
                    DenseField::Selection => result.selection_count = dense.count,
                }
                if self.page_field == Some(field) {
                    result.page_items = dense.page_items;
                }
            } else {
                core.insert(key, map.next_value::<serde_json::Value>()?);
            }
        }
        result.core_json = serde_json::to_string(&core).map_err(serde::de::Error::custom)?;
        Ok(result)
    }
}

fn sqlite_conversion_error(
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

fn project_json_reader(
    reader: impl Read,
    page_field: Option<DenseField>,
    offset: usize,
    limit: usize,
) -> SqlResult<JsonObjectProjection> {
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let projection = JsonObjectProjectionSeed {
        page_field,
        offset,
        limit,
    }
    .deserialize(&mut deserializer)
    .map_err(sqlite_conversion_error)?;
    deserializer.end().map_err(sqlite_conversion_error)?;
    Ok(projection)
}

struct PositionedReader<R> {
    inner: BufReader<R>,
    position: u64,
}

impl<R: Read> PositionedReader<R> {
    fn new(reader: R) -> Self {
        Self::with_position(reader, 0)
    }

    fn with_position(reader: R, position: u64) -> Self {
        Self {
            inner: BufReader::with_capacity(PAYLOAD_READ_CHUNK_BYTES, reader),
            position,
        }
    }

    fn peek(&mut self) -> io::Result<Option<u8>> {
        Ok(self.inner.fill_buf()?.first().copied())
    }

    fn next(&mut self) -> io::Result<Option<u8>> {
        let byte = self.peek()?;
        if byte.is_some() {
            self.inner.consume(1);
            self.position += 1;
        }
        Ok(byte)
    }

    fn skip_whitespace(&mut self) -> io::Result<()> {
        while self.peek()?.is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.next()?;
        }
        Ok(())
    }
}

fn read_raw_json_value<R: Read>(
    reader: &mut PositionedReader<R>,
    max_bytes: usize,
) -> io::Result<Option<Vec<u8>>> {
    reader.skip_whitespace()?;
    if reader.peek()? == Some(b']') {
        return Ok(None);
    }
    let first = reader
        .next()?
        .ok_or_else(|| invalid_json("missing indexed topology item"))?;
    let mut raw = vec![first];
    let mut push = |byte: u8| -> io::Result<()> {
        if raw.len() >= max_bytes {
            return Err(invalid_json("dense topology item exceeds transport budget"));
        }
        raw.push(byte);
        Ok(())
    };
    if first == b'"' {
        let mut escaped = false;
        loop {
            let byte = reader
                .next()?
                .ok_or_else(|| invalid_json("unterminated indexed JSON string"))?;
            push(byte)?;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                return Ok(Some(raw));
            }
        }
    }
    if first == b'{' || first == b'[' {
        let mut depth = 1usize;
        let mut in_string = false;
        let mut escaped = false;
        while depth > 0 {
            let byte = reader
                .next()?
                .ok_or_else(|| invalid_json("unterminated indexed JSON value"))?;
            push(byte)?;
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        return Ok(Some(raw));
    }
    while let Some(byte) = reader.peek()? {
        if matches!(byte, b',' | b']') || byte.is_ascii_whitespace() {
            break;
        }
        reader.next()?;
        push(byte)?;
    }
    Ok(Some(raw))
}

fn invalid_json(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

fn read_json_string<R: Read>(reader: &mut PositionedReader<R>) -> io::Result<String> {
    if reader.next()? != Some(b'"') {
        return Err(invalid_json("expected JSON string"));
    }
    let mut raw = vec![b'"'];
    let mut escaped = false;
    loop {
        let byte = reader
            .next()?
            .ok_or_else(|| invalid_json("unterminated JSON string"))?;
        raw.push(byte);
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            break;
        }
    }
    serde_json::from_slice(&raw).map_err(io::Error::other)
}

fn skip_json_value<R: Read>(reader: &mut PositionedReader<R>) -> io::Result<()> {
    reader.skip_whitespace()?;
    let first = reader
        .peek()?
        .ok_or_else(|| invalid_json("missing JSON value"))?;
    if first == b'"' {
        read_json_string(reader)?;
        return Ok(());
    }
    if first != b'{' && first != b'[' {
        while let Some(byte) = reader.peek()? {
            if matches!(byte, b',' | b']' | b'}') || byte.is_ascii_whitespace() {
                break;
            }
            reader.next()?;
        }
        return Ok(());
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    loop {
        let byte = reader
            .next()?
            .ok_or_else(|| invalid_json("unterminated composite JSON value"))?;
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
}

fn scan_dense_array<R: Read>(reader: &mut PositionedReader<R>) -> io::Result<Vec<u64>> {
    if reader.next()? != Some(b'[') {
        return Err(invalid_json("dense topology field is not an array"));
    }
    let mut offsets = Vec::new();
    let mut count = 0usize;
    loop {
        reader.skip_whitespace()?;
        if reader.peek()? == Some(b']') {
            reader.next()?;
            return Ok(offsets);
        }
        if count % 500 == 0 {
            offsets.push(reader.position);
        }
        skip_json_value(reader)?;
        count += 1;
        reader.skip_whitespace()?;
        match reader.next()? {
            Some(b',') => {}
            Some(b']') => return Ok(offsets),
            _ => return Err(invalid_json("invalid dense topology array delimiter")),
        }
    }
}

fn scan_dense_indexes(reader: impl Read) -> SqlResult<DenseIndexes> {
    let mut reader = PositionedReader::new(reader);
    reader.skip_whitespace().map_err(sqlite_conversion_error)?;
    if reader.next().map_err(sqlite_conversion_error)? != Some(b'{') {
        return Err(sqlite_conversion_error(invalid_json(
            "payload projection root is not an object",
        )));
    }
    let mut indexes = DenseIndexes::default();
    loop {
        reader.skip_whitespace().map_err(sqlite_conversion_error)?;
        if reader.peek().map_err(sqlite_conversion_error)? == Some(b'}') {
            reader.next().map_err(sqlite_conversion_error)?;
            return Ok(indexes);
        }
        let key = read_json_string(&mut reader).map_err(sqlite_conversion_error)?;
        reader.skip_whitespace().map_err(sqlite_conversion_error)?;
        if reader.next().map_err(sqlite_conversion_error)? != Some(b':') {
            return Err(sqlite_conversion_error(invalid_json(
                "missing JSON object colon",
            )));
        }
        reader.skip_whitespace().map_err(sqlite_conversion_error)?;
        match key.as_str() {
            "edgeTargets" => {
                indexes.edge = scan_dense_array(&mut reader).map_err(sqlite_conversion_error)?
            }
            "faceTargets" => {
                indexes.face = scan_dense_array(&mut reader).map_err(sqlite_conversion_error)?
            }
            "selectionTargets" => {
                indexes.selection =
                    scan_dense_array(&mut reader).map_err(sqlite_conversion_error)?
            }
            _ => skip_json_value(&mut reader).map_err(sqlite_conversion_error)?,
        }
        reader.skip_whitespace().map_err(sqlite_conversion_error)?;
        match reader.next().map_err(sqlite_conversion_error)? {
            Some(b',') => {}
            Some(b'}') => return Ok(indexes),
            _ => {
                return Err(sqlite_conversion_error(invalid_json(
                    "invalid JSON object delimiter",
                )))
            }
        }
    }
}

fn ensure_payload_projection_schema(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS payload_projections (
            owner_kind TEXT NOT NULL CHECK(owner_kind IN ('message', 'draft')),
            owner_id TEXT NOT NULL,
            codec_version INTEGER NOT NULL DEFAULT 1,
            model_id TEXT,
            edge_count INTEGER NOT NULL DEFAULT 0,
            face_count INTEGER NOT NULL DEFAULT 0,
            selection_count INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(owner_kind, owner_id)
         ) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS dense_topology_chunks (
            owner_kind TEXT NOT NULL CHECK(owner_kind IN ('message', 'draft')),
            owner_id TEXT NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('edge', 'face', 'selection')),
            chunk_index INTEGER NOT NULL,
            item_count INTEGER NOT NULL,
            codec_version INTEGER NOT NULL DEFAULT 1,
            payload BLOB NOT NULL,
            PRIMARY KEY(owner_kind, owner_id, kind, chunk_index)
         ) WITHOUT ROWID;
         CREATE TRIGGER IF NOT EXISTS delete_message_payload_projection
         AFTER DELETE ON messages BEGIN
           DELETE FROM payload_projections
           WHERE owner_kind = 'message' AND owner_id = OLD.id;
           DELETE FROM dense_topology_chunks
           WHERE owner_kind = 'message' AND owner_id = OLD.id;
         END;
         CREATE TRIGGER IF NOT EXISTS delete_draft_payload_projection
         AFTER DELETE ON agent_drafts BEGIN
           DELETE FROM payload_projections
           WHERE owner_kind = 'draft' AND owner_id = OLD.preview_id;
           DELETE FROM dense_topology_chunks
           WHERE owner_kind = 'draft' AND owner_id = OLD.preview_id;
         END;",
    )?;
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS invalidate_message_payload_projection;
         DROP TRIGGER IF EXISTS invalidate_draft_payload_projection;",
    )?;
    let _ = conn.execute(
        "ALTER TABLE payload_projections ADD COLUMN codec_version INTEGER NOT NULL DEFAULT 1",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE payload_projections ADD COLUMN model_id TEXT",
        [],
    );
    Ok(())
}

fn encode_payload<T: serde::Serialize>(value: &T) -> SqlResult<Vec<u8>> {
    let encoded = rmp_serde::to_vec_named(value).map_err(sqlite_conversion_error)?;
    let mut payload = Vec::with_capacity(PAYLOAD_CODEC_MAGIC.len() + encoded.len());
    payload.extend_from_slice(PAYLOAD_CODEC_MAGIC);
    payload.extend_from_slice(&encoded);
    Ok(payload)
}

fn decode_payload<T: DeserializeOwned>(payload: &[u8]) -> SqlResult<T> {
    if !payload.starts_with(PAYLOAD_CODEC_MAGIC) {
        return Err(rusqlite::Error::InvalidParameterName(
            "Unsupported CAD payload codec header.".to_string(),
        ));
    }
    rmp_serde::from_slice(&payload[PAYLOAD_CODEC_MAGIC.len()..]).map_err(sqlite_conversion_error)
}

fn cached_payload_projection(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
) -> SqlResult<Option<PayloadProjection>> {
    ensure_payload_projection_schema(conn)?;
    conn.query_row(
        "SELECT codec_version, model_id, edge_count, face_count, selection_count
         FROM payload_projections WHERE owner_kind = ?1 AND owner_id = ?2",
        params![owner.as_str(), owner_id],
        |row| {
            Ok(PayloadProjection {
                model_id: {
                    let version = row.get::<_, i64>(0)?;
                    if version != PAYLOAD_CODEC_VERSION {
                        return Err(rusqlite::Error::InvalidParameterName(format!(
                            "Unsupported CAD payload codec version {version}."
                        )));
                    }
                    row.get(1)?
                },
                edge_count: row.get::<_, i64>(2)?.max(0) as usize,
                face_count: row.get::<_, i64>(3)?.max(0) as usize,
                selection_count: row.get::<_, i64>(4)?.max(0) as usize,
            })
        },
    )
    .optional()
}

fn store_payload_projection(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
    projection: &PayloadProjection,
) -> SqlResult<()> {
    ensure_payload_projection_schema(conn)?;
    conn.execute(
        "INSERT INTO payload_projections (
           owner_kind, owner_id, codec_version, model_id,
           edge_count, face_count, selection_count
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(owner_kind, owner_id) DO UPDATE SET
           codec_version = excluded.codec_version,
           model_id = excluded.model_id,
           edge_count = excluded.edge_count,
           face_count = excluded.face_count,
           selection_count = excluded.selection_count",
        params![
            owner.as_str(),
            owner_id,
            PAYLOAD_CODEC_VERSION,
            projection.model_id,
            projection.edge_count as i64,
            projection.face_count as i64,
            projection.selection_count as i64,
        ],
    )?;
    Ok(())
}

fn artifact_bundle_core(bundle: &ArtifactBundle) -> ArtifactBundle {
    ArtifactBundle {
        schema_version: bundle.schema_version,
        model_id: bundle.model_id.clone(),
        source_kind: bundle.source_kind.clone(),
        engine_kind: bundle.engine_kind,
        source_language: bundle.source_language,
        geometry_backend: bundle.geometry_backend,
        content_hash: bundle.content_hash.clone(),
        artifact_version: bundle.artifact_version,
        fcstd_path: bundle.fcstd_path.clone(),
        manifest_path: bundle.manifest_path.clone(),
        macro_path: bundle.macro_path.clone(),
        model_stl_path: bundle.model_stl_path.clone(),
        viewer_assets: bundle.viewer_assets.clone(),
        edge_targets: Vec::new(),
        face_targets: Vec::new(),
        callout_anchors: bundle.callout_anchors.clone(),
        measurement_guides: bundle.measurement_guides.clone(),
        export_artifacts: bundle.export_artifacts.clone(),
        geometry_provenance: bundle.geometry_provenance.clone(),
        component_dependency_lock: bundle.component_dependency_lock.clone(),
        component_dependency_lock_digest: bundle.component_dependency_lock_digest.clone(),
        component_import_origins: bundle.component_import_origins.clone(),
        component_placement_evidence: bundle.component_placement_evidence.clone(),
    }
}

fn model_manifest_core(manifest: &ModelManifest) -> ModelManifest {
    ModelManifest {
        schema_version: manifest.schema_version,
        model_id: manifest.model_id.clone(),
        source_kind: manifest.source_kind.clone(),
        source_digest: manifest.source_digest.clone(),
        core_digest: manifest.core_digest.clone(),
        ast_schema_version: manifest.ast_schema_version,
        engine_kind: manifest.engine_kind,
        source_language: manifest.source_language,
        geometry_backend: manifest.geometry_backend,
        document: manifest.document.clone(),
        parts: manifest.parts.clone(),
        parameter_groups: manifest.parameter_groups.clone(),
        control_primitives: manifest.control_primitives.clone(),
        control_relations: manifest.control_relations.clone(),
        control_views: manifest.control_views.clone(),
        preview_views: manifest.preview_views.clone(),
        advisories: manifest.advisories.clone(),
        selection_targets: Vec::new(),
        measurement_annotations: manifest.measurement_annotations.clone(),
        tagged_anchors: manifest.tagged_anchors.clone(),
        feature_graph: manifest.feature_graph.clone(),
        correspondence_graph: manifest.correspondence_graph.clone(),
        analysis_declarations: manifest.analysis_declarations.clone(),
        warnings: manifest.warnings.clone(),
        enrichment_state: manifest.enrichment_state.clone(),
        geometry_provenance: manifest.geometry_provenance.clone(),
        component_import_origins: manifest.component_import_origins.clone(),
        component_placement_evidence: manifest.component_placement_evidence.clone(),
    }
}

fn replace_topology_chunks<T: serde::Serialize>(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
    kind: &str,
    items: &[T],
) -> SqlResult<()> {
    conn.execute(
        "DELETE FROM dense_topology_chunks
         WHERE owner_kind = ?1 AND owner_id = ?2 AND kind = ?3",
        params![owner.as_str(), owner_id, kind],
    )?;
    for (chunk_index, chunk) in items.chunks(TOPOLOGY_CHUNK_ITEMS).enumerate() {
        let payload = encode_payload(&chunk)?;
        conn.execute(
            "INSERT INTO dense_topology_chunks (
               owner_kind, owner_id, kind, chunk_index, item_count, codec_version, payload
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                owner.as_str(),
                owner_id,
                kind,
                chunk_index as i64,
                chunk.len() as i64,
                PAYLOAD_CODEC_VERSION,
                payload,
            ],
        )?;
    }
    Ok(())
}

fn encode_cad_payload(
    artifact_bundle: Option<&ArtifactBundle>,
    model_manifest: Option<&ModelManifest>,
) -> SqlResult<EncodedCadPayload> {
    Ok(EncodedCadPayload {
        artifact_core: artifact_bundle
            .map(|bundle| encode_payload(&artifact_bundle_core(bundle)))
            .transpose()?,
        model_manifest_core: model_manifest
            .map(|manifest| encode_payload(&model_manifest_core(manifest)))
            .transpose()?,
        projection: PayloadProjection {
            model_id: artifact_bundle
                .map(|bundle| bundle.model_id.clone())
                .or_else(|| model_manifest.map(|manifest| manifest.model_id.clone())),
            edge_count: artifact_bundle.map_or(0, |bundle| bundle.edge_targets.len()),
            face_count: artifact_bundle.map_or(0, |bundle| bundle.face_targets.len()),
            selection_count: model_manifest.map_or(0, |manifest| manifest.selection_targets.len()),
        },
    })
}

fn store_payload_sidecars_from_structs(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
    artifact_bundle: Option<&ArtifactBundle>,
    model_manifest: Option<&ModelManifest>,
    projection: &PayloadProjection,
) -> SqlResult<()> {
    ensure_payload_projection_schema(conn)?;
    store_payload_projection(conn, owner, owner_id, projection)?;
    replace_topology_chunks(
        conn,
        owner,
        owner_id,
        "edge",
        artifact_bundle.map_or(&[][..], |bundle| bundle.edge_targets.as_slice()),
    )?;
    replace_topology_chunks(
        conn,
        owner,
        owner_id,
        "face",
        artifact_bundle.map_or(&[][..], |bundle| bundle.face_targets.as_slice()),
    )?;
    replace_topology_chunks(
        conn,
        owner,
        owner_id,
        "selection",
        model_manifest.map_or(&[][..], |manifest| manifest.selection_targets.as_slice()),
    )?;
    Ok(())
}

fn load_topology_chunks<T: DeserializeOwned>(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
    kind: &str,
) -> SqlResult<Vec<T>> {
    let mut statement = conn.prepare(
        "SELECT codec_version, payload FROM dense_topology_chunks
         WHERE owner_kind = ?1 AND owner_id = ?2 AND kind = ?3
         ORDER BY chunk_index ASC",
    )?;
    let rows = statement
        .query_map(params![owner.as_str(), owner_id, kind], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<SqlResult<Vec<_>>>()?;
    let mut result = Vec::new();
    for (version, payload) in rows {
        if version != PAYLOAD_CODEC_VERSION {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "Unsupported topology codec version {version}."
            )));
        }
        let mut chunk: Vec<T> = decode_payload(&payload)?;
        result.append(&mut chunk);
    }
    Ok(result)
}

fn load_topology_json_page<T: DeserializeOwned + serde::Serialize>(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
    kind: &str,
    offset: usize,
    limit: usize,
) -> SqlResult<Vec<String>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut result = Vec::with_capacity(limit);
    let mut absolute = offset;
    while result.len() < limit {
        let chunk_index = absolute / TOPOLOGY_CHUNK_ITEMS;
        let within_chunk = absolute % TOPOLOGY_CHUNK_ITEMS;
        let row = conn
            .query_row(
                "SELECT codec_version, payload FROM dense_topology_chunks
                 WHERE owner_kind = ?1 AND owner_id = ?2 AND kind = ?3 AND chunk_index = ?4",
                params![owner.as_str(), owner_id, kind, chunk_index as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?;
        let Some((version, payload)) = row else {
            break;
        };
        if version != PAYLOAD_CODEC_VERSION {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "Unsupported topology codec version {version}."
            )));
        }
        let chunk: Vec<T> = decode_payload(&payload)?;
        if within_chunk >= chunk.len() {
            break;
        }
        for item in chunk.iter().skip(within_chunk).take(limit - result.len()) {
            result.push(serde_json::to_string(item).map_err(sqlite_conversion_error)?);
        }
        absolute = (chunk_index + 1) * TOPOLOGY_CHUNK_ITEMS;
    }
    Ok(result)
}

fn load_payload_core(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
) -> SqlResult<(
    Option<ArtifactBundle>,
    Option<ModelManifest>,
    PayloadProjection,
)> {
    let projection = cached_payload_projection(conn, owner, owner_id)?.unwrap_or_default();
    let sql = format!(
        "SELECT artifact_bundle, model_manifest FROM {} WHERE {} = ?1",
        owner.table(),
        owner.id_column(),
    );
    let (artifact_blob, manifest_blob): (Option<Vec<u8>>, Option<Vec<u8>>) =
        conn.query_row(&sql, [owner_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let artifact = artifact_blob.as_deref().map(decode_payload).transpose()?;
    let manifest = manifest_blob.as_deref().map(decode_payload).transpose()?;
    Ok((artifact, manifest, projection))
}

fn load_payload_full(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
) -> SqlResult<(Option<ArtifactBundle>, Option<ModelManifest>)> {
    let (mut artifact, mut manifest, projection) = load_payload_core(conn, owner, owner_id)?;
    if let Some(bundle) = artifact.as_mut() {
        bundle.edge_targets = load_topology_chunks(conn, owner, owner_id, "edge")?;
        bundle.face_targets = load_topology_chunks(conn, owner, owner_id, "face")?;
        if bundle.edge_targets.len() != projection.edge_count
            || bundle.face_targets.len() != projection.face_count
        {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "Topology chunk count mismatch for {} {owner_id}.",
                owner.as_str()
            )));
        }
    }
    if let Some(value) = manifest.as_mut() {
        value.selection_targets = load_topology_chunks(conn, owner, owner_id, "selection")?;
        if value.selection_targets.len() != projection.selection_count {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "Selection chunk count mismatch for {} {owner_id}.",
                owner.as_str()
            )));
        }
    }
    Ok((artifact, manifest))
}

fn stream_payload_column(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
    column: PayloadColumn,
    page_field: Option<DenseField>,
    offset: usize,
    limit: usize,
) -> SqlResult<Option<JsonObjectProjection>> {
    let sql = format!(
        "SELECT rowid FROM {} WHERE {} = ?1 AND {} IS NOT NULL",
        owner.table(),
        owner.id_column(),
        column.as_str(),
    );
    let rowid = conn
        .query_row(&sql, [owner_id], |row| row.get::<_, i64>(0))
        .optional()?;
    let Some(rowid) = rowid else {
        return Ok(None);
    };
    let blob = conn.blob_open("main", owner.table(), column.as_str(), rowid, true)?;
    project_json_reader(
        BufReader::with_capacity(PAYLOAD_READ_CHUNK_BYTES, blob),
        page_field,
        offset,
        limit,
    )
    .map(Some)
}

fn stream_payload_indexes(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
    column: PayloadColumn,
) -> SqlResult<Option<DenseIndexes>> {
    let sql = format!(
        "SELECT rowid FROM {} WHERE {} = ?1 AND {} IS NOT NULL",
        owner.table(),
        owner.id_column(),
        column.as_str(),
    );
    let rowid = conn
        .query_row(&sql, [owner_id], |row| row.get::<_, i64>(0))
        .optional()?;
    let Some(rowid) = rowid else {
        return Ok(None);
    };
    let blob = conn.blob_open("main", owner.table(), column.as_str(), rowid, true)?;
    scan_dense_indexes(blob).map(Some)
}

fn read_indexed_dense_chunk<R: Read + Seek>(
    reader: &mut R,
    checkpoint_offset: u64,
    limit: usize,
) -> SqlResult<Vec<String>> {
    reader
        .seek(SeekFrom::Start(checkpoint_offset))
        .map_err(sqlite_conversion_error)?;
    let mut reader = PositionedReader::with_position(reader, checkpoint_offset);
    let mut items = Vec::with_capacity(limit);
    while items.len() < limit {
        let Some(raw) = read_raw_json_value(
            &mut reader,
            crate::transport_budget::TOPOLOGY_PAGE_MAX_BYTES + 1,
        )
        .map_err(sqlite_conversion_error)?
        else {
            break;
        };
        items.push(String::from_utf8(raw).map_err(sqlite_conversion_error)?);
        reader.skip_whitespace().map_err(sqlite_conversion_error)?;
        match reader.peek().map_err(sqlite_conversion_error)? {
            Some(b',') => {
                reader.next().map_err(sqlite_conversion_error)?;
            }
            Some(b']') | None => break,
            _ => {
                return Err(sqlite_conversion_error(invalid_json(
                    "invalid indexed topology delimiter",
                )))
            }
        }
    }
    Ok(items)
}

fn migrate_legacy_topology_kind<T: DeserializeOwned + serde::Serialize>(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
    column: PayloadColumn,
    kind: &str,
    offsets: &[u64],
    expected_count: usize,
) -> SqlResult<()> {
    conn.execute(
        "DELETE FROM dense_topology_chunks
         WHERE owner_kind = ?1 AND owner_id = ?2 AND kind = ?3",
        params![owner.as_str(), owner_id, kind],
    )?;
    if offsets.is_empty() {
        if expected_count == 0 {
            return Ok(());
        }
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "CAD payload migration found no {kind} checkpoints for {} {owner_id}; expected {expected_count} items.",
            owner.as_str()
        )));
    }
    let rowid_sql = format!(
        "SELECT rowid FROM {} WHERE {} = ?1 AND {} IS NOT NULL",
        owner.table(),
        owner.id_column(),
        column.as_str(),
    );
    let rowid = conn.query_row(&rowid_sql, [owner_id], |row| row.get::<_, i64>(0))?;
    let mut blob = conn.blob_open("main", owner.table(), column.as_str(), rowid, true)?;
    let mut migrated_count = 0usize;
    for (chunk_index, checkpoint_offset) in offsets.iter().copied().enumerate() {
        let raw_items =
            read_indexed_dense_chunk(&mut blob, checkpoint_offset, TOPOLOGY_CHUNK_ITEMS)?;
        let items = raw_items
            .into_iter()
            .map(|raw| serde_json::from_str::<T>(&raw).map_err(sqlite_conversion_error))
            .collect::<SqlResult<Vec<_>>>()?;
        migrated_count += items.len();
        let payload = encode_payload(&items)?;
        conn.execute(
            "INSERT INTO dense_topology_chunks (
               owner_kind, owner_id, kind, chunk_index, item_count, codec_version, payload
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                owner.as_str(),
                owner_id,
                kind,
                chunk_index as i64,
                items.len() as i64,
                PAYLOAD_CODEC_VERSION,
                payload,
            ],
        )?;
    }
    if migrated_count != expected_count {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "CAD payload migration count mismatch for {} {owner_id} {kind}: expected {expected_count}, wrote {migrated_count}.",
            owner.as_str()
        )));
    }
    Ok(())
}

fn migrate_legacy_payload_owner(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
) -> SqlResult<()> {
    let artifact = stream_payload_column(
        conn,
        owner,
        owner_id,
        PayloadColumn::ArtifactBundle,
        None,
        0,
        0,
    )?;
    let manifest = stream_payload_column(
        conn,
        owner,
        owner_id,
        PayloadColumn::ModelManifest,
        None,
        0,
        0,
    )?;
    let artifact_indexes =
        stream_payload_indexes(conn, owner, owner_id, PayloadColumn::ArtifactBundle)?
            .unwrap_or_default();
    let manifest_indexes =
        stream_payload_indexes(conn, owner, owner_id, PayloadColumn::ModelManifest)?
            .unwrap_or_default();

    let artifact_core = artifact
        .as_ref()
        .map(|value| {
            serde_json::from_str::<ArtifactBundle>(&value.core_json)
                .map_err(sqlite_conversion_error)
        })
        .transpose()?;
    let manifest_core = manifest
        .as_ref()
        .map(|value| {
            serde_json::from_str::<ModelManifest>(&value.core_json).map_err(sqlite_conversion_error)
        })
        .transpose()?;
    let encoded_artifact = artifact_core.as_ref().map(encode_payload).transpose()?;
    let encoded_manifest = manifest_core.as_ref().map(encode_payload).transpose()?;
    let projection = PayloadProjection {
        model_id: artifact_core
            .as_ref()
            .map(|value| value.model_id.clone())
            .or_else(|| manifest_core.as_ref().map(|value| value.model_id.clone())),
        edge_count: artifact.as_ref().map_or(0, |value| value.edge_count),
        face_count: artifact.as_ref().map_or(0, |value| value.face_count),
        selection_count: manifest.as_ref().map_or(0, |value| value.selection_count),
    };
    conn.execute(
        "DELETE FROM dense_topology_chunks WHERE owner_kind = ?1 AND owner_id = ?2",
        params![owner.as_str(), owner_id],
    )?;
    store_payload_projection(conn, owner, owner_id, &projection)?;
    migrate_legacy_topology_kind::<crate::contracts::ViewerEdgeTarget>(
        conn,
        owner,
        owner_id,
        PayloadColumn::ArtifactBundle,
        "edge",
        &artifact_indexes.edge,
        projection.edge_count,
    )?;
    migrate_legacy_topology_kind::<crate::contracts::ViewerFaceTarget>(
        conn,
        owner,
        owner_id,
        PayloadColumn::ArtifactBundle,
        "face",
        &artifact_indexes.face,
        projection.face_count,
    )?;
    migrate_legacy_topology_kind::<crate::contracts::SelectionTarget>(
        conn,
        owner,
        owner_id,
        PayloadColumn::ModelManifest,
        "selection",
        &manifest_indexes.selection,
        projection.selection_count,
    )?;

    let update_sql = format!(
        "UPDATE {} SET artifact_bundle = ?1, model_manifest = ?2 WHERE {} = ?3",
        owner.table(),
        owner.id_column(),
    );
    conn.execute(
        &update_sql,
        params![
            encoded_artifact.as_deref(),
            encoded_manifest.as_deref(),
            owner_id
        ],
    )?;
    if owner == PayloadOwnerKind::Message {
        let output = conn
            .query_row(
                "SELECT output FROM messages WHERE id = ?1",
                [owner_id],
                |row| row.get::<_, Option<String>>(0),
            )?
            .map(|raw| serde_json::from_str::<DesignOutput>(&raw).map(normalize_design_output))
            .transpose()
            .map_err(sqlite_conversion_error)?;
        let (version_input_digest, runtime_cache_key) =
            version_runtime_binding(owner_id, output.as_ref(), artifact_core.as_ref())?;
        conn.execute(
            "UPDATE messages SET version_input_digest = ?1, runtime_cache_key = ?2 WHERE id = ?3",
            params![version_input_digest, runtime_cache_key, owner_id],
        )?;
    }
    let (decoded_artifact, decoded_manifest, decoded_projection) =
        load_payload_core(conn, owner, owner_id)?;
    if decoded_artifact.is_some() != artifact_core.is_some()
        || decoded_manifest.is_some() != manifest_core.is_some()
        || decoded_projection.edge_count != projection.edge_count
        || decoded_projection.face_count != projection.face_count
        || decoded_projection.selection_count != projection.selection_count
    {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "CAD payload migration verification failed for {} {owner_id}.",
            owner.as_str()
        )));
    }
    Ok(())
}

const BINARY_CAD_PAYLOAD_MIGRATION_KEY: &str = "binary-cad-payload-v1";

fn legacy_cad_payload_counts(conn: &Connection) -> SqlResult<(i64, i64)> {
    conn.query_row(
        "SELECT
           (SELECT COUNT(*) FROM messages
            WHERE typeof(artifact_bundle) = 'text' OR typeof(model_manifest) = 'text'),
           (SELECT COUNT(*) FROM agent_drafts
            WHERE typeof(artifact_bundle) = 'text' OR typeof(model_manifest) = 'text')",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
}

fn binary_cad_payload_migration_completed(conn: &Connection) -> SqlResult<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE key = ?1",
            [BINARY_CAD_PAYLOAD_MIGRATION_KEY],
            |_row| Ok(()),
        )
        .optional()?
        .is_some())
}

fn require_binary_cad_payloads(conn: &Connection) -> SqlResult<()> {
    let completed = binary_cad_payload_migration_completed(conn)?;
    let (message_count, draft_count) = legacy_cad_payload_counts(conn)?;
    let legacy_count = message_count + draft_count;
    if completed {
        if legacy_count > 0 {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "CAD payload migration is marked complete but {legacy_count} legacy JSON rows remain."
            )));
        }
        return Ok(());
    }
    if legacy_count > 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "CAD payload migration required: {message_count} message rows and {draft_count} draft rows still use legacy JSON. Run migrate_history_payloads offline before opening this app."
        )));
    }
    conn.execute(
        "INSERT INTO schema_migrations(key, applied_at)
         VALUES (?1, CAST(strftime('%s','now') AS INTEGER))",
        [BINARY_CAD_PAYLOAD_MIGRATION_KEY],
    )?;
    Ok(())
}

fn migrate_legacy_cad_payloads(conn: &Connection) -> SqlResult<()> {
    let completed = conn
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE key = ?1",
            [BINARY_CAD_PAYLOAD_MIGRATION_KEY],
            |_row| Ok(()),
        )
        .optional()?
        .is_some();
    let (message_count, draft_count) = legacy_cad_payload_counts(conn)?;
    let legacy_count = message_count + draft_count;
    if completed {
        if legacy_count > 0 {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "CAD payload migration is marked complete but {legacy_count} legacy JSON rows remain."
            )));
        }
        return Ok(());
    }

    let message_ids = {
        let mut statement = conn.prepare(
            "SELECT id FROM messages
             WHERE typeof(artifact_bundle) = 'text' OR typeof(model_manifest) = 'text'
             ORDER BY rowid ASC",
        )?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<SqlResult<Vec<_>>>()?;
        ids
    };
    let draft_ids = {
        let mut statement = conn.prepare(
            "SELECT preview_id FROM agent_drafts
             WHERE typeof(artifact_bundle) = 'text' OR typeof(model_manifest) = 'text'
             ORDER BY rowid ASC",
        )?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<SqlResult<Vec<_>>>()?;
        ids
    };

    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        conn.execute("DELETE FROM dense_topology_chunks", [])?;
        conn.execute("DELETE FROM payload_projections", [])?;
        for message_id in &message_ids {
            migrate_legacy_payload_owner(conn, PayloadOwnerKind::Message, message_id).map_err(
                |error| {
                    rusqlite::Error::InvalidParameterName(format!(
                        "CAD payload migration failed for message {message_id}: {error}"
                    ))
                },
            )?;
        }
        for preview_id in &draft_ids {
            migrate_legacy_payload_owner(conn, PayloadOwnerKind::Draft, preview_id).map_err(
                |error| {
                    rusqlite::Error::InvalidParameterName(format!(
                        "CAD payload migration failed for draft {preview_id}: {error}"
                    ))
                },
            )?;
        }
        conn.execute(
            "INSERT INTO schema_migrations(key, applied_at)
             VALUES (?1, CAST(strftime('%s','now') AS INTEGER))",
            [BINARY_CAD_PAYLOAD_MIGRATION_KEY],
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    Ok(())
}

fn ensure_payload_projection(
    conn: &Connection,
    owner: PayloadOwnerKind,
    owner_id: &str,
) -> SqlResult<Option<PayloadProjection>> {
    cached_payload_projection(conn, owner, owner_id)
}

#[derive(Clone, Copy)]
enum PayloadInitializationMode {
    RequireReady,
    MigrateLegacy,
}

fn init_db_with_payload_mode(
    db_path: &std::path::Path,
    payload_mode: PayloadInitializationMode,
) -> SqlResult<Connection> {
    let conn = Connection::open(db_path)?;

    // Enable WAL mode for better concurrency and prevent "database is locked" errors
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS threads (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            summary TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL,
            genie_traits TEXT,
            deleted_at INTEGER
        )",
        [],
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS campaign_runs (
            id TEXT PRIMARY KEY,
            definition_id TEXT NOT NULL,
            definition_version TEXT NOT NULL,
            title TEXT NOT NULL,
            current_step_id TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS campaign_run_steps (
            run_id TEXT NOT NULL REFERENCES campaign_runs(id) ON DELETE CASCADE,
            step_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('completed', 'passed', 'draft')),
            draft_override TEXT,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(run_id, step_id, status),
            CHECK((status = 'draft' AND draft_override IS NOT NULL)
                  OR (status != 'draft' AND draft_override IS NULL))
        );",
    )?;
    migrate_campaign_step_primary_key(&conn)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS active_project_navigation (
            slot INTEGER PRIMARY KEY CHECK(slot = 1),
            kind TEXT NOT NULL CHECK(kind IN ('design', 'campaign')),
            project_id TEXT NOT NULL,
            view TEXT NOT NULL CHECK(view IN ('workbench', 'campaign')),
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS app_window_layouts (
            slot INTEGER PRIMARY KEY CHECK(slot = 1),
            layout_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );",
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'success',
            output TEXT,
            usage TEXT,
            artifact_bundle TEXT,
            model_manifest TEXT,
            structural_verification TEXT,
            agent_origin TEXT,
            timestamp INTEGER NOT NULL,
            image_data TEXT,
            visual_kind TEXT,
            attachment_images TEXT,
            version_input_digest TEXT,
            runtime_cache_key TEXT,
            deleted_at INTEGER,
            trash_hidden_at INTEGER,
            FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS thread_references (
            id TEXT PRIMARY KEY,
            thread_id TEXT NOT NULL,
            source_message_id TEXT,
            ordinal INTEGER NOT NULL DEFAULT 0,
            kind TEXT NOT NULL,
            name TEXT NOT NULL,
            content TEXT NOT NULL DEFAULT '',
            summary TEXT NOT NULL DEFAULT '',
            pinned INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS thread_window_layouts (
            thread_id TEXT PRIMARY KEY,
            layout_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )",
        [],
    )?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_thread_references_source_ordinal_kind
         ON thread_references(source_message_id, ordinal, kind)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_sessions (
            session_id TEXT PRIMARY KEY,
            client_kind TEXT NOT NULL,
            host_label TEXT NOT NULL DEFAULT '',
            agent_label TEXT NOT NULL,
            llm_model_id TEXT,
            llm_model_label TEXT,
            thread_id TEXT,
            message_id TEXT,
            model_id TEXT,
            phase TEXT NOT NULL,
            status_text TEXT NOT NULL DEFAULT '',
            updated_at INTEGER NOT NULL,
            managed_runtime INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;

    if !table_has_column(&conn, "agent_drafts", "preview_id")? {
        let _ = conn.execute("DROP TABLE IF EXISTS agent_drafts", []);
    }
    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_drafts (
            preview_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            base_message_id TEXT,
            design_output TEXT NOT NULL,
            artifact_bundle TEXT NOT NULL,
            model_manifest TEXT NOT NULL,
            draft_feedback TEXT,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;
    let _ = conn.execute(
        "ALTER TABLE agent_drafts ADD COLUMN draft_feedback TEXT",
        [],
    );
    // Draft identity is an authoring actor identity, not a client-session identity.
    // Older installs had a unique session index and silently replaced a draft from
    // another thread in the same client session.
    conn.execute("DROP INDEX IF EXISTS idx_agent_drafts_session", [])?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_drafts_session_thread
         ON agent_drafts(session_id, thread_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_agent_drafts_thread_updated
         ON agent_drafts(thread_id, updated_at DESC)",
        [],
    )?;
    ensure_payload_projection_schema(&conn)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS verification_records (
            snapshot_id TEXT PRIMARY KEY,
            preview_id TEXT NOT NULL,
            artifact_digest TEXT NOT NULL,
            verification_record TEXT NOT NULL,
            verified_at INTEGER NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_verification_records_preview
         ON verification_records(preview_id, verified_at DESC)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS target_leases (
            lease_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            message_id TEXT NOT NULL,
            model_id TEXT,
            acquired_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL,
            host_label TEXT NOT NULL DEFAULT '',
            agent_label TEXT NOT NULL DEFAULT ''
        )",
        [],
    )?;

    // Persistent per-thread source binding (openspec thread-source-binding).
    // Folder + model.ecky + ecky-thread.json live under config.projectsRoot;
    // this row is the authoritative binding lookup. SQLite history stays
    // canonical; the file is the editable working copy.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS thread_source_bindings (
            thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
            folder_path TEXT NOT NULL,
            source_path TEXT NOT NULL UNIQUE,
            source_digest TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
        [],
    )?;
    crate::thread_source_binding::ensure_schema(&conn)?;

    crate::exploration_store::ensure_schema(&conn)?;

    crate::services::codex_takeover::ensure_schema(&conn)?;

    crate::capture_runs::ensure_schema(&conn)?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_thread_visible_timestamp
         ON messages(thread_id, timestamp DESC)
         WHERE deleted_at IS NULL",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_thread_target_candidates
         ON messages(thread_id, role, status, timestamp DESC)
         WHERE deleted_at IS NULL",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_target_leases_target_expires
         ON target_leases(thread_id, message_id, model_id, expires_at DESC)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_target_leases_session
         ON target_leases(session_id, expires_at DESC)",
        [],
    )?;
    // Migrations for existing databases
    let _ = conn.execute(
        "ALTER TABLE threads ADD COLUMN summary TEXT NOT NULL DEFAULT ''",
        [],
    );
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN created_at INTEGER", []);
    conn.execute(
        "UPDATE threads SET created_at = updated_at WHERE created_at IS NULL",
        [],
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            key TEXT PRIMARY KEY,
            applied_at INTEGER NOT NULL
         );
         UPDATE threads
         SET created_at = updated_at - 1
         WHERE created_at = updated_at
           AND NOT EXISTS (
             SELECT 1 FROM schema_migrations
             WHERE key = 'threads-created-at-v1'
           );
         INSERT OR IGNORE INTO schema_migrations (key, applied_at)
         VALUES ('threads-created-at-v1', CAST(strftime('%s','now') AS INTEGER));",
    )?;
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN genie_traits TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN image_data TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN visual_kind TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN attachment_images TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN usage TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN artifact_bundle TEXT", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN model_manifest TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN structural_verification TEXT",
        [],
    );
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN agent_origin TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN status TEXT NOT NULL DEFAULT 'success'",
        [],
    );
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN deleted_at INTEGER", []);
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN deleted_at INTEGER", []);
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN trash_hidden_at INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE messages ADD COLUMN version_input_digest TEXT",
        [],
    );
    let _ = conn.execute("ALTER TABLE messages ADD COLUMN runtime_cache_key TEXT", []);
    match payload_mode {
        PayloadInitializationMode::RequireReady => require_binary_cad_payloads(&conn)?,
        PayloadInitializationMode::MigrateLegacy => migrate_legacy_cad_payloads(&conn)?,
    }
    let _ = conn.execute(
        "ALTER TABLE threads ADD COLUMN status TEXT NOT NULL DEFAULT 'active'",
        [],
    );
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN finalized_at INTEGER", []);
    let _ = conn.execute("ALTER TABLE threads ADD COLUMN pending_confirm TEXT", []);
    migrate_threads_drop_authoring_columns(&conn)?;
    normalize_thread_lifecycle_rows(&conn)?;
    crate::services::codex_takeover::backfill_provider_thread_projection(&conn)?;
    let _ = conn.execute(
        "ALTER TABLE agent_sessions ADD COLUMN host_label TEXT NOT NULL DEFAULT ''",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE agent_sessions ADD COLUMN llm_model_id TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE agent_sessions ADD COLUMN llm_model_label TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE agent_sessions ADD COLUMN managed_runtime INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "DROP INDEX IF EXISTS idx_agent_session_trace_session_trace_id",
        [],
    );
    let _ = conn.execute("DROP TABLE IF EXISTS agent_session_trace", []);
    migrate_thread_genie_traits(&conn)?;

    Ok(conn)
}

pub fn init_db(db_path: &std::path::Path) -> SqlResult<Connection> {
    init_db_with_payload_mode(db_path, PayloadInitializationMode::RequireReady)
}

pub fn migrate_history_payload_storage(db_path: &std::path::Path) -> SqlResult<()> {
    init_db_with_payload_mode(db_path, PayloadInitializationMode::MigrateLegacy)?;
    Ok(())
}

fn migrate_campaign_step_primary_key(conn: &Connection) -> SqlResult<()> {
    let mut statement = conn.prepare("PRAGMA table_info(campaign_run_steps)")?;
    let mut primary_key_columns = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    primary_key_columns.sort_by_key(|(_, position)| *position);
    let primary_key_columns = primary_key_columns
        .into_iter()
        .filter_map(|(name, position)| (position > 0).then_some(name))
        .collect::<Vec<_>>();
    if primary_key_columns == ["run_id", "step_id", "status"] {
        return Ok(());
    }

    conn.execute_batch(
        "BEGIN;
         ALTER TABLE campaign_run_steps RENAME TO campaign_run_steps_legacy;
         CREATE TABLE campaign_run_steps (
            run_id TEXT NOT NULL REFERENCES campaign_runs(id) ON DELETE CASCADE,
            step_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('completed', 'passed', 'draft')),
            draft_override TEXT,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(run_id, step_id, status),
            CHECK((status = 'draft' AND draft_override IS NOT NULL)
                  OR (status != 'draft' AND draft_override IS NULL))
         );
         INSERT INTO campaign_run_steps (run_id, step_id, status, draft_override, updated_at)
         SELECT run_id, step_id, status, draft_override, updated_at FROM campaign_run_steps_legacy;
         DROP TABLE campaign_run_steps_legacy;
         COMMIT;",
    )
}

fn deserialize_thread_genie_traits(thread_id: &str, raw: Option<&str>) -> GenieTraits {
    upgraded_or_default_genie_traits(thread_id, raw)
}

fn deserialize_agent_origin(raw: Option<&str>) -> Option<crate::contracts::AgentOrigin> {
    raw.and_then(|json| serde_json::from_str(json).ok())
}

fn serialize_json<T: serde::Serialize>(value: &T) -> SqlResult<String> {
    serde_json::to_string(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

fn deserialize_json<T: DeserializeOwned>(raw: &str) -> SqlResult<T> {
    serde_json::from_str(raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn deserialize_design_output_json(raw: &str) -> SqlResult<DesignOutput> {
    let parsed: DesignOutput = deserialize_json(raw)?;
    Ok(normalize_design_output(parsed))
}

fn migrate_thread_genie_traits(conn: &Connection) -> SqlResult<()> {
    let mut stmt = conn.prepare("SELECT id, genie_traits FROM threads")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for (thread_id, raw) in rows {
        let traits = deserialize_thread_genie_traits(&thread_id, raw.as_deref());
        let traits_json = serde_json::to_string(&traits).unwrap_or_default();
        conn.execute(
            "UPDATE threads SET genie_traits = ?1 WHERE id = ?2",
            params![traits_json, thread_id],
        )?;
    }

    Ok(())
}

fn table_has_column(conn: &Connection, table_name: &str, column_name: &str) -> SqlResult<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table_name})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column_name {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate_threads_drop_authoring_columns(conn: &Connection) -> SqlResult<()> {
    let has_engine_kind = table_has_column(conn, "threads", "engine_kind")?;
    let has_source_language = table_has_column(conn, "threads", "source_language")?;
    let has_geometry_backend = table_has_column(conn, "threads", "geometry_backend")?;

    if !has_engine_kind && !has_source_language && !has_geometry_backend {
        return Ok(());
    }

    conn.execute_batch(
        "
        PRAGMA foreign_keys = OFF;
        CREATE TABLE IF NOT EXISTS threads_new (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            summary TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            genie_traits TEXT,
            deleted_at INTEGER,
            status TEXT NOT NULL DEFAULT 'active',
            finalized_at INTEGER,
            pending_confirm TEXT
        );
        INSERT OR REPLACE INTO threads_new (
            id,
            title,
            summary,
            created_at,
            updated_at,
            genie_traits,
            deleted_at,
            status,
            finalized_at,
            pending_confirm
        )
        SELECT
            id,
            title,
            COALESCE(summary, ''),
            created_at,
            updated_at,
            genie_traits,
            deleted_at,
            COALESCE(status, 'active'),
            finalized_at,
            pending_confirm
        FROM threads;
        DROP TABLE threads;
        ALTER TABLE threads_new RENAME TO threads;
        PRAGMA foreign_keys = ON;
        ",
    )?;
    Ok(())
}

/// Repair rows written before lifecycle fields were treated as one aggregate.
/// Keep columns for transport compatibility, but make stored combinations canonical.
fn normalize_thread_lifecycle_rows(conn: &Connection) -> SqlResult<()> {
    conn.execute(
        "UPDATE threads SET status = 'active', finalized_at = NULL WHERE finalized_at IS NULL AND COALESCE(status, 'active') = 'finalized'",
        [],
    )?;
    conn.execute(
        "UPDATE threads SET status = 'finalized', pending_confirm = NULL WHERE finalized_at IS NOT NULL",
        [],
    )?;
    Ok(())
}

pub fn get_all_threads(conn: &Connection) -> SqlResult<Vec<Thread>> {
    let mut stmt = conn.prepare("
        SELECT id, title, summary,
        COALESCE(
            (
                SELECT MAX(timestamp)
                FROM messages
                WHERE thread_id = threads.id
                  AND deleted_at IS NULL
                  AND status != 'discarded'
            ),
            updated_at
        ) as last_used_at,
        genie_traits,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status != 'discarded' AND artifact_bundle IS NOT NULL AND deleted_at IS NULL) as v_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'pending' AND deleted_at IS NULL) as p_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'user' AND status = 'pending' AND deleted_at IS NULL) as q_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'error' AND (agent_origin IS NULL OR output IS NOT NULL OR artifact_bundle IS NOT NULL) AND deleted_at IS NULL) as e_count,
        COALESCE(status, 'active') as thread_status,
        finalized_at,
        pending_confirm,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND deleted_at IS NULL AND status != 'discarded') as message_count,
        EXISTS(SELECT 1 FROM agent_provider_messages WHERE ecky_thread_id = threads.id AND status != 'discarded') as has_provider_content
        FROM threads
        WHERE deleted_at IS NULL AND COALESCE(status, 'active') = 'active'
        ORDER BY last_used_at DESC, id DESC
    ")?;
    let thread_iter = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let traits_str: Option<String> = row.get(4)?;
        let status_str: String = row
            .get::<_, String>(9)
            .unwrap_or_else(|_| "active".to_string());
        let message_count = row.get::<_, i64>(12)? as usize;
        let has_provider_content = row.get::<_, bool>(13)?;
        let title: String = row.get(1)?;
        Ok(Thread {
            id: id.clone(),
            title: title.clone(),
            summary: row.get(2)?,
            updated_at: row.get::<_, i64>(3)? as u64,
            messages: vec![],
            genie_traits: Some(deserialize_thread_genie_traits(&id, traits_str.as_deref())),
            version_count: row.get::<_, i64>(5)? as usize,
            pending_count: row.get::<_, i64>(6)? as usize,
            queued_count: row.get::<_, i64>(7)? as usize,
            error_count: row.get::<_, i64>(8)? as usize,
            is_blank: title == "Untitled design" && message_count == 0 && !has_provider_content,
            status: status_str
                .parse()
                .unwrap_or(crate::contracts::ThreadStatus::Active),
            finalized_at: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
            pending_confirm: row.get(11)?,
        })
    })?;

    let mut threads = Vec::new();
    for thread in thread_iter {
        threads.push(thread?);
    }
    Ok(threads)
}

pub fn get_thread_summary_by_id(conn: &Connection, thread_id: &str) -> SqlResult<Option<Thread>> {
    conn.query_row(
        "SELECT
           threads.id,
           threads.title,
           COALESCE(threads.summary, ''),
           COALESCE(
             (SELECT MAX(timestamp) FROM messages
              WHERE thread_id = threads.id AND deleted_at IS NULL AND status != 'discarded'),
             threads.updated_at
           ),
           threads.genie_traits,
           (SELECT COUNT(*) FROM messages
            WHERE thread_id = threads.id AND role = 'assistant'
              AND status != 'discarded' AND deleted_at IS NULL
              AND artifact_bundle IS NOT NULL),
           (SELECT COUNT(*) FROM messages
            WHERE thread_id = threads.id AND role = 'assistant'
              AND status = 'pending' AND deleted_at IS NULL),
           (SELECT COUNT(*) FROM messages
            WHERE thread_id = threads.id AND role = 'user'
              AND status = 'pending' AND deleted_at IS NULL),
           (SELECT COUNT(*) FROM messages
            WHERE thread_id = threads.id AND role = 'assistant'
              AND status = 'error'
              AND (agent_origin IS NULL OR output IS NOT NULL OR artifact_bundle IS NOT NULL)
              AND deleted_at IS NULL),
           COALESCE(threads.status, 'active'),
           threads.finalized_at,
           threads.pending_confirm,
           (SELECT COUNT(*) FROM messages
            WHERE thread_id = threads.id AND deleted_at IS NULL AND status != 'discarded'),
           EXISTS(SELECT 1 FROM agent_provider_messages
            WHERE ecky_thread_id = threads.id AND status != 'discarded')
         FROM threads
         WHERE threads.id = ?1 AND threads.deleted_at IS NULL",
        [thread_id],
        |row| {
            let id: String = row.get(0)?;
            let traits_str: Option<String> = row.get(4)?;
            let status_str: String = row.get(9)?;
            let title: String = row.get(1)?;
            Ok(Thread {
                id: id.clone(),
                title: title.clone(),
                summary: row.get(2)?,
                updated_at: row.get::<_, i64>(3)? as u64,
                messages: Vec::new(),
                genie_traits: Some(deserialize_thread_genie_traits(&id, traits_str.as_deref())),
                version_count: row.get::<_, i64>(5)? as usize,
                pending_count: row.get::<_, i64>(6)? as usize,
                queued_count: row.get::<_, i64>(7)? as usize,
                error_count: row.get::<_, i64>(8)? as usize,
                is_blank: title == "Untitled design"
                    && row.get::<_, i64>(12)? == 0
                    && !row.get::<_, bool>(13)?,
                status: status_str.parse().unwrap_or(ThreadStatus::Active),
                finalized_at: row.get::<_, Option<i64>>(10)?.map(|value| value as u64),
                pending_confirm: row.get(11)?,
            })
        },
    )
    .optional()
}

pub fn get_recent_threads_limited(conn: &Connection, limit: usize) -> SqlResult<Vec<Thread>> {
    let mut stmt = conn.prepare(
        "
        SELECT id, title, summary,
        COALESCE(
            (
                SELECT MAX(timestamp)
                FROM messages
                WHERE thread_id = threads.id
                  AND deleted_at IS NULL
                  AND status != 'discarded'
            ),
            updated_at
        ) as last_used_at,
        genie_traits,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status != 'discarded' AND artifact_bundle IS NOT NULL AND deleted_at IS NULL) as v_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'pending' AND deleted_at IS NULL) as p_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'user' AND status = 'pending' AND deleted_at IS NULL) as q_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'error' AND (agent_origin IS NULL OR output IS NOT NULL OR artifact_bundle IS NOT NULL) AND deleted_at IS NULL) as e_count,
        COALESCE(status, 'active') as thread_status,
        finalized_at,
        pending_confirm,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND deleted_at IS NULL AND status != 'discarded') as message_count,
        EXISTS(SELECT 1 FROM agent_provider_messages WHERE ecky_thread_id = threads.id AND status != 'discarded') as has_provider_content
        FROM threads
        WHERE deleted_at IS NULL AND COALESCE(status, 'active') = 'active'
        ORDER BY last_used_at DESC, id DESC
        LIMIT ?1
    ",
    )?;
    let thread_iter = stmt.query_map([limit as i64], |row| {
        let id: String = row.get(0)?;
        let traits_str: Option<String> = row.get(4)?;
        let status_str: String = row
            .get::<_, String>(9)
            .unwrap_or_else(|_| "active".to_string());
        let message_count = row.get::<_, i64>(12)? as usize;
        let has_provider_content = row.get::<_, bool>(13)?;
        let title: String = row.get(1)?;
        Ok(Thread {
            id: id.clone(),
            title: title.clone(),
            summary: row.get(2)?,
            updated_at: row.get::<_, i64>(3)? as u64,
            messages: vec![],
            genie_traits: Some(deserialize_thread_genie_traits(&id, traits_str.as_deref())),
            version_count: row.get::<_, i64>(5)? as usize,
            pending_count: row.get::<_, i64>(6)? as usize,
            queued_count: row.get::<_, i64>(7)? as usize,
            error_count: row.get::<_, i64>(8)? as usize,
            is_blank: title == "Untitled design" && message_count == 0 && !has_provider_content,
            status: status_str
                .parse()
                .unwrap_or(crate::contracts::ThreadStatus::Active),
            finalized_at: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
            pending_confirm: row.get(11)?,
        })
    })?;

    let mut threads = Vec::new();
    for thread in thread_iter {
        threads.push(thread?);
    }
    Ok(threads)
}

pub fn get_latest_successful_message_id_in_thread(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT m.id
         FROM messages m
         JOIN threads t ON t.id = m.thread_id
         WHERE m.thread_id = ?1
           AND t.deleted_at IS NULL
           AND m.deleted_at IS NULL
           AND m.role = 'assistant'
           AND m.status = 'success'
           AND m.artifact_bundle IS NOT NULL
         ORDER BY m.timestamp DESC, m.id DESC
         LIMIT 1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_latest_successful_target_in_most_recent_thread(
    conn: &Connection,
) -> SqlResult<Option<LatestSuccessfulTarget>> {
    conn.query_row(
        "
        WITH recent_threads AS (
            SELECT id,
                   COALESCE(
                       (
                           SELECT MAX(timestamp)
                           FROM messages
                           WHERE thread_id = threads.id
                             AND deleted_at IS NULL
                             AND status != 'discarded'
                       ),
                       updated_at
                   ) AS last_used_at
            FROM threads
            WHERE deleted_at IS NULL
        )
        SELECT m.thread_id, m.id
        FROM messages m
        INNER JOIN recent_threads rt ON rt.id = m.thread_id
        WHERE m.deleted_at IS NULL
          AND m.role = 'assistant'
          AND m.status = 'success'
          AND m.artifact_bundle IS NOT NULL
        ORDER BY rt.last_used_at DESC, m.timestamp DESC, m.id DESC
        LIMIT 1
        ",
        [],
        |row| {
            Ok(LatestSuccessfulTarget {
                thread_id: row.get(0)?,
                message_id: row.get(1)?,
            })
        },
    )
    .optional()
}

pub fn create_or_update_thread(
    conn: &Connection,
    thread_id: &str,
    title: &str,
    updated_at: u64,
    genie_traits: Option<&GenieTraits>,
) -> SqlResult<()> {
    create_or_update_thread_with_timestamps(
        conn,
        thread_id,
        title,
        updated_at,
        updated_at,
        genie_traits,
    )
}

pub fn create_or_update_thread_with_timestamps(
    conn: &Connection,
    thread_id: &str,
    title: &str,
    created_at: u64,
    updated_at: u64,
    genie_traits: Option<&GenieTraits>,
) -> SqlResult<()> {
    let traits_str = genie_traits.and_then(|t| serde_json::to_string(t).ok());
    conn.execute(
        "INSERT INTO threads (id, title, created_at, updated_at, genie_traits) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
            title=CASE
                WHEN threads.title IS NULL OR trim(threads.title) = '' THEN excluded.title
                ELSE threads.title
            END,
            updated_at=excluded.updated_at,
            genie_traits=COALESCE(excluded.genie_traits, threads.genie_traits)",
        params![
            thread_id,
            title,
            created_at as i64,
            updated_at as i64,
            traits_str
        ],
    )?;
    Ok(())
}

pub fn get_thread_genie_traits(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<GenieTraits>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT genie_traits FROM threads WHERE id = ?1",
            [thread_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();

    Ok(Some(deserialize_thread_genie_traits(
        thread_id,
        raw.as_deref(),
    )))
}

pub fn update_thread_summary(conn: &Connection, thread_id: &str, summary: &str) -> SqlResult<()> {
    conn.execute(
        "UPDATE threads SET summary = ?1 WHERE id = ?2",
        params![summary, thread_id],
    )?;
    Ok(())
}

pub fn update_thread_title(conn: &Connection, thread_id: &str, title: &str) -> SqlResult<bool> {
    let changed = conn.execute(
        "UPDATE threads
         SET title = ?1,
             updated_at = MAX(updated_at + 1, CAST(strftime('%s','now') AS INTEGER))
         WHERE id = ?2 AND deleted_at IS NULL",
        params![title, thread_id],
    )?;
    Ok(changed > 0)
}

pub fn get_thread_title(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT title FROM threads WHERE id = ?1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_visible_thread_title(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT title FROM threads WHERE id = ?1 AND deleted_at IS NULL",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn thread_has_durable_content(conn: &Connection, thread_id: &str) -> SqlResult<bool> {
    conn.query_row(
        "SELECT
            EXISTS (
                SELECT 1 FROM messages
                WHERE thread_id = ?1
                  AND deleted_at IS NULL
                  AND status != 'discarded'
            )
            OR EXISTS (
                SELECT 1 FROM agent_provider_messages
                WHERE ecky_thread_id = ?1
                  AND status != 'discarded'
            )",
        [thread_id],
        |row| row.get(0),
    )
}

pub fn get_thread_summary(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT summary FROM threads WHERE id = ?1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_legacy_user_prompt_rows(
    conn: &Connection,
) -> SqlResult<Vec<(String, String, String, u64)>> {
    let mut stmt = conn.prepare(
        "SELECT messages.thread_id, messages.id, messages.content, messages.timestamp
         FROM messages
         WHERE messages.role = 'user'
           AND messages.deleted_at IS NULL
           AND NOT EXISTS (
             SELECT 1 FROM thread_references
             WHERE thread_references.source_message_id = messages.id
           )
         ORDER BY messages.timestamp ASC, messages.rowid ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get::<_, i64>(3)? as u64,
        ))
    })?;
    rows.collect()
}

pub use crate::thread_lifecycle::ThreadLifecycle;

pub fn get_thread_lifecycle(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<ThreadLifecycle>> {
    conn.query_row(
        "SELECT COALESCE(status, 'active'), finalized_at, pending_confirm FROM threads WHERE id = ?1",
        [thread_id],
        |row| {
            let status_str: String = row.get::<_, String>(0).unwrap_or_else(|_| "active".to_string());
            let lifecycle = crate::thread_lifecycle::ThreadLifecycle::from_legacy(
                status_str.parse().unwrap_or(crate::contracts::ThreadStatus::Active),
                row.get::<_, Option<i64>>(1)?.map(|v| v as u64),
                row.get(2)?,
            );
            Ok(lifecycle)
        },
    )
    .optional()
}

pub fn finalize_thread(conn: &Connection, thread_id: &str, now: i64) -> SqlResult<bool> {
    let changed = conn.execute(
        "UPDATE threads SET status = 'finalized', finalized_at = ?1, pending_confirm = NULL WHERE id = ?2 AND deleted_at IS NULL",
        params![now, thread_id],
    )?;
    Ok(changed > 0)
}

pub fn reopen_thread(conn: &Connection, thread_id: &str) -> SqlResult<bool> {
    let changed = conn.execute(
        "UPDATE threads SET status = 'active', finalized_at = NULL, pending_confirm = NULL WHERE id = ?1 AND deleted_at IS NULL",
        [thread_id],
    )?;
    Ok(changed > 0)
}

pub fn get_inventory_threads(conn: &Connection) -> SqlResult<Vec<Thread>> {
    let mut stmt = conn.prepare("
        SELECT id, title, summary,
        COALESCE(
            (
                SELECT MAX(timestamp)
                FROM messages
                WHERE thread_id = threads.id
                  AND deleted_at IS NULL
                  AND status != 'discarded'
            ),
            updated_at
        ) as last_used_at,
        genie_traits,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status != 'discarded' AND artifact_bundle IS NOT NULL AND deleted_at IS NULL) as v_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'pending' AND deleted_at IS NULL) as p_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'user' AND status = 'pending' AND deleted_at IS NULL) as q_count,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND role = 'assistant' AND status = 'error' AND (agent_origin IS NULL OR output IS NOT NULL OR artifact_bundle IS NOT NULL) AND deleted_at IS NULL) as e_count,
        COALESCE(status, 'active') as thread_status,
        finalized_at,
        pending_confirm,
        (SELECT COUNT(*) FROM messages WHERE thread_id = threads.id AND deleted_at IS NULL AND status != 'discarded') as message_count,
        EXISTS(SELECT 1 FROM agent_provider_messages WHERE ecky_thread_id = threads.id AND status != 'discarded') as has_provider_content
        FROM threads
        WHERE deleted_at IS NULL AND COALESCE(status, 'active') = 'finalized'
        ORDER BY finalized_at DESC, id DESC
    ")?;
    let thread_iter = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let traits_str: Option<String> = row.get(4)?;
        let status_str: String = row
            .get::<_, String>(9)
            .unwrap_or_else(|_| "finalized".to_string());
        let message_count = row.get::<_, i64>(12)? as usize;
        let has_provider_content = row.get::<_, bool>(13)?;
        let title: String = row.get(1)?;
        Ok(Thread {
            id: id.clone(),
            title: title.clone(),
            summary: row.get(2)?,
            updated_at: row.get::<_, i64>(3)? as u64,
            messages: vec![],
            genie_traits: Some(deserialize_thread_genie_traits(&id, traits_str.as_deref())),
            version_count: row.get::<_, i64>(5)? as usize,
            pending_count: row.get::<_, i64>(6)? as usize,
            queued_count: row.get::<_, i64>(7)? as usize,
            error_count: row.get::<_, i64>(8)? as usize,
            is_blank: title == "Untitled design" && message_count == 0 && !has_provider_content,
            status: status_str
                .parse()
                .unwrap_or(crate::contracts::ThreadStatus::Finalized),
            finalized_at: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
            pending_confirm: row.get(11)?,
        })
    })?;

    let mut threads = Vec::new();
    for thread in thread_iter {
        threads.push(thread?);
    }
    Ok(threads)
}

pub fn set_thread_pending_confirm(
    conn: &Connection,
    thread_id: &str,
    pending_confirm: Option<&str>,
) -> SqlResult<()> {
    let Some(raw) = get_thread_lifecycle(conn, thread_id)? else {
        return Ok(());
    };
    let canonical = raw.with_pending_confirm(pending_confirm.map(str::to_string));
    conn.execute(
        "UPDATE threads SET status = ?1, finalized_at = ?2, pending_confirm = ?3 WHERE id = ?4",
        params![
            canonical.status().as_str(),
            canonical.finalized_at().map(|value| value as i64),
            canonical.pending_confirm(),
            thread_id
        ],
    )?;
    Ok(())
}

fn version_runtime_binding(
    message_id: &str,
    output: Option<&DesignOutput>,
    artifact_bundle: Option<&ArtifactBundle>,
) -> SqlResult<(Option<String>, Option<String>)> {
    let Some(output) = output else {
        return Ok((None, None));
    };
    let version_input_digest = crate::services::render_snapshot::canonical_version_input_digest(
        output,
        &output.initial_params,
    )
    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let runtime_cache_key = artifact_bundle
        .map(|bundle| {
            crate::services::render_snapshot::version_runtime_cache_key(
                message_id,
                &version_input_digest,
                bundle,
            )
        })
        .transpose()
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    Ok((Some(version_input_digest), runtime_cache_key))
}

/// Validate the legacy column projection before a new message is persisted.
/// The projection remains nullable for old rows, but new version payloads are
/// assembled as one coherent aggregate by callers.
pub fn validate_message_payload(msg: &Message) -> Result<(), AppError> {
    let has_version_fields = msg.output.is_some()
        || msg.artifact_bundle.is_some()
        || msg.model_manifest.is_some()
        || msg.structural_verification.is_some();
    if msg.role == MessageRole::User && has_version_fields {
        return Err(AppError::validation(
            "User message cannot carry version payload.",
        ));
    }
    if let (Some(bundle), Some(manifest)) = (&msg.artifact_bundle, &msg.model_manifest) {
        if bundle.model_id != manifest.model_id {
            return Err(AppError::validation(format!(
                "Artifact model '{}' does not match manifest model '{}'.",
                bundle.model_id, manifest.model_id
            )));
        }
    }
    if msg.artifact_bundle.is_some() || msg.model_manifest.is_some() {
        if msg.output.is_none() {
            return Err(AppError::validation(
                "Runtime payload requires design output.",
            ));
        }
        if msg.artifact_bundle.is_none() || msg.model_manifest.is_none() {
            return Err(AppError::validation(
                "Runtime payload requires artifact bundle and model manifest.",
            ));
        }
    }
    if msg.structural_verification.is_some() && msg.output.is_none() {
        return Err(AppError::validation(
            "Structural verification requires design output.",
        ));
    }
    Ok(())
}

/// Translate nullable legacy columns into the atomic domain representation.
/// This is the sole adapter new writers should use before persistence.
pub fn message_payload_from_legacy(
    msg: &Message,
) -> Result<crate::contracts::MessagePayload, AppError> {
    validate_message_payload(msg)?;
    match (
        msg.output.clone(),
        msg.artifact_bundle.clone(),
        msg.model_manifest.clone(),
        msg.structural_verification.clone(),
    ) {
        (Some(output), Some(artifact_bundle), Some(model_manifest), structural_verification) => {
            Ok(crate::contracts::MessagePayload::Version {
                output,
                artifact_bundle,
                model_manifest,
                structural_verification,
            })
        }
        (None, None, None, None) if msg.status == MessageStatus::Error => {
            Ok(crate::contracts::MessagePayload::Error)
        }
        (None, None, None, None) => Ok(crate::contracts::MessagePayload::Conversation),
        _ => Err(AppError::validation(
            "Message contains incomplete version payload.",
        )),
    }
}

fn is_managed_runtime_stl(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("stl"))
        && path
            .components()
            .any(|component| component.as_os_str() == "model-runtime")
}

fn prune_non_latest_thread_stls(conn: &Connection, thread_id: &str) -> SqlResult<()> {
    let protected_paths = get_latest_version_artifact_bundles(conn)?
        .into_iter()
        .flat_map(|bundle| {
            std::iter::once(bundle.model_stl_path)
                .chain(bundle.viewer_assets.into_iter().map(|asset| asset.path))
        })
        .map(std::path::PathBuf::from)
        .collect::<std::collections::HashSet<_>>();
    let latest_id = conn
        .query_row(
            "SELECT id FROM messages
             WHERE thread_id = ?1 AND deleted_at IS NULL AND role = 'assistant'
               AND status != 'discarded' AND (output IS NOT NULL OR artifact_bundle IS NOT NULL)
             ORDER BY timestamp DESC, rowid DESC LIMIT 1",
            [thread_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let mut statement = conn.prepare(
        "SELECT messages.id, messages.artifact_bundle
         FROM messages
         WHERE messages.thread_id = ?1
           AND messages.deleted_at IS NULL
           AND messages.role = 'assistant'
           AND messages.status != 'discarded'
           AND messages.artifact_bundle IS NOT NULL",
    )?;
    let rows = statement.query_map([thread_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    for row in rows {
        let (message_id, raw_bundle) = row?;
        if latest_id.as_deref() == Some(message_id.as_str()) {
            continue;
        }
        let Ok(bundle) = decode_payload::<ArtifactBundle>(&raw_bundle) else {
            continue;
        };
        for raw_path in std::iter::once(bundle.model_stl_path)
            .chain(bundle.viewer_assets.into_iter().map(|asset| asset.path))
        {
            let path = std::path::PathBuf::from(raw_path);
            if is_managed_runtime_stl(&path) && !protected_paths.contains(&path) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    Ok(())
}

pub fn add_message(conn: &Connection, thread_id: &str, msg: &Message) -> SqlResult<()> {
    let output_str = msg
        .output
        .as_ref()
        .and_then(|o| serde_json::to_string(o).ok());
    let usage_str = msg
        .usage
        .as_ref()
        .and_then(|usage| serde_json::to_string(usage).ok());
    let encoded_payload =
        encode_cad_payload(msg.artifact_bundle.as_ref(), msg.model_manifest.as_ref())?;
    let structural_verification_str = msg
        .structural_verification
        .as_ref()
        .and_then(|result| serde_json::to_string(result).ok());
    let agent_origin_str = msg
        .agent_origin
        .as_ref()
        .and_then(|origin| serde_json::to_string(origin).ok());
    let attachment_images_str = if msg.attachment_images.is_empty() {
        None
    } else {
        serde_json::to_string(&msg.attachment_images).ok()
    };
    let (version_input_digest, runtime_cache_key) =
        version_runtime_binding(&msg.id, msg.output.as_ref(), msg.artifact_bundle.as_ref())?;
    conn.execute_batch("SAVEPOINT add_message_payload")?;
    let write_result = (|| {
        conn.execute(
        "INSERT INTO messages (id, thread_id, role, content, status, output, usage, artifact_bundle, model_manifest, structural_verification, agent_origin, timestamp, image_data, visual_kind, attachment_images, version_input_digest, runtime_cache_key) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            msg.id,
            thread_id,
            msg.role,
            msg.content,
            msg.status,
            output_str,
            usage_str,
            encoded_payload.artifact_core.as_deref(),
            encoded_payload.model_manifest_core.as_deref(),
            structural_verification_str,
            agent_origin_str,
            msg.timestamp as i64,
            msg.image_data,
            msg.visual_kind,
            attachment_images_str,
            version_input_digest,
            runtime_cache_key,
        ],
    )?;
        store_payload_sidecars_from_structs(
            conn,
            PayloadOwnerKind::Message,
            &msg.id,
            msg.artifact_bundle.as_ref(),
            msg.model_manifest.as_ref(),
            &encoded_payload.projection,
        )?;
        conn.execute(
            "UPDATE threads
         SET updated_at = MAX(updated_at + 1, ?1)
         WHERE id = ?2",
            params![msg.timestamp as i64, thread_id],
        )?;
        if msg.artifact_bundle.is_some() {
            prune_non_latest_thread_stls(conn, thread_id)?;
        }
        Ok(())
    })();
    match write_result {
        Ok(()) => conn.execute_batch("RELEASE add_message_payload")?,
        Err(error) => {
            let _ =
                conn.execute_batch("ROLLBACK TO add_message_payload; RELEASE add_message_payload;");
            return Err(error);
        }
    }
    Ok(())
}

/// Transitional writer for nullable historical projections. Keep new
/// conversation/version writes on `add_message_checked`; use this only where
/// an upstream provider still emits output-only or artifact-only rows.
pub fn add_legacy_message(conn: &Connection, thread_id: &str, msg: &Message) -> SqlResult<()> {
    add_message(conn, thread_id, msg)
}

/// Persist a newly assembled atomic payload. `add_message` remains the
/// compatibility path for historical callers and fixtures that still emit
/// nullable legacy projections.
pub fn add_message_checked(conn: &Connection, thread_id: &str, msg: &Message) -> SqlResult<()> {
    message_payload_from_legacy(msg)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    add_message(conn, thread_id, msg)
}

pub fn add_thread_reference(conn: &Connection, reference: &ThreadReference) -> SqlResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO thread_references
         (id, thread_id, source_message_id, ordinal, kind, name, content, summary, pinned, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            reference.id,
            reference.thread_id,
            reference.source_message_id,
            reference.ordinal,
            reference.kind,
            reference.name,
            reference.content,
            reference.summary,
            if reference.pinned { 1 } else { 0 },
            reference.created_at as i64
        ],
    )?;
    Ok(())
}

pub fn get_thread_messages(conn: &Connection, thread_id: &str) -> SqlResult<Vec<Message>> {
    Ok(load_thread_message_rows(conn, thread_id, false)?
        .into_iter()
        .filter(|row| {
            row.deleted_at.is_none()
                && row.message.status != MessageStatus::Discarded
                && !is_agent_tool_error_message(&row.message)
        })
        .map(|row| row.message)
        .collect())
}

pub fn get_thread_messages_for_thread_view(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Vec<Message>> {
    Ok(load_thread_message_rows(conn, thread_id, true)?
        .into_iter()
        .filter_map(|mut row| {
            if row.deleted_at.is_some() {
                if is_version_message(&row.message) {
                    row.message.status = MessageStatus::Discarded;
                    return Some(row.message);
                }
                return None;
            }

            if row.message.status == MessageStatus::Discarded && !is_version_message(&row.message) {
                return None;
            }

            if is_agent_tool_error_message(&row.message) {
                return None;
            }

            Some(row.message)
        })
        .collect())
}

pub fn get_thread_latest_version(conn: &Connection, thread_id: &str) -> SqlResult<Option<Message>> {
    let rows = load_thread_message_rows_with_clause(
        conn,
        "thread_id = ?1
         AND deleted_at IS NULL
         AND role = 'assistant'
         AND status != 'discarded'
         AND (output IS NOT NULL OR artifact_bundle IS NOT NULL)",
        &[&thread_id],
        "timestamp DESC, rowid DESC",
        Some(1),
    )?;
    Ok(rows.into_iter().next().map(|row| row.message))
}

/// Returns durable authored head identity without selecting or deserializing
/// message payload columns.
pub fn get_thread_head_version_id(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT id
         FROM messages
         WHERE thread_id = ?1
           AND deleted_at IS NULL
           AND role = 'assistant'
           AND status != 'discarded'
           AND output IS NOT NULL
         ORDER BY timestamp DESC, rowid DESC
         LIMIT 1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_latest_version_artifact_bundles(conn: &Connection) -> SqlResult<Vec<ArtifactBundle>> {
    let mut statement = conn.prepare(
        "SELECT m.id
         FROM messages m
         WHERE m.deleted_at IS NULL
           AND m.role = 'assistant'
           AND m.status != 'discarded'
           AND m.artifact_bundle IS NOT NULL
           AND m.rowid = (
             SELECT newer.rowid
             FROM messages newer
             WHERE newer.thread_id = m.thread_id
               AND newer.deleted_at IS NULL
               AND newer.role = 'assistant'
               AND newer.status != 'discarded'
               AND (newer.output IS NOT NULL OR newer.artifact_bundle IS NOT NULL)
             ORDER BY newer.timestamp DESC, newer.rowid DESC
             LIMIT 1
           )",
    )?;
    let message_ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<SqlResult<Vec<_>>>()?;
    drop(statement);
    let mut bundles = Vec::new();
    for message_id in message_ids {
        let (bundle, _, _) = load_payload_core(conn, PayloadOwnerKind::Message, &message_id)?;
        if let Some(bundle) = bundle {
            bundles.push(bundle);
        }
    }
    Ok(bundles)
}

pub fn get_thread_message_version(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
) -> SqlResult<Option<Message>> {
    let rows = load_thread_message_rows_with_clause(
        conn,
        "thread_id = ?1
         AND id = ?2
         AND deleted_at IS NULL
         AND role = 'assistant'
         AND status != 'discarded'
         AND (output IS NOT NULL OR artifact_bundle IS NOT NULL)",
        &[&thread_id, &message_id],
        "timestamp DESC, rowid DESC",
        Some(1),
    )?;
    Ok(rows.into_iter().next().map(|row| row.message))
}

fn core_version_columns() -> &'static str {
    "id, role, content, status, output, usage,
     artifact_bundle,
     model_manifest,
     structural_verification, agent_origin, timestamp, NULL, visual_kind, NULL,
     deleted_at, version_input_digest, runtime_cache_key"
}

fn get_thread_message_version_core_row(
    conn: &Connection,
    thread_id: &str,
    message_id: Option<&str>,
) -> SqlResult<Option<ThreadMessageRow>> {
    let target_id = if let Some(message_id) = message_id {
        conn.query_row(
            "SELECT id FROM messages
             WHERE thread_id = ?1 AND id = ?2 AND deleted_at IS NULL AND role = 'assistant'
               AND status != 'discarded' AND (output IS NOT NULL OR artifact_bundle IS NOT NULL)
             LIMIT 1",
            params![thread_id, message_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    } else {
        conn.query_row(
            "SELECT id FROM messages
             WHERE thread_id = ?1 AND deleted_at IS NULL AND role = 'assistant'
               AND status != 'discarded' AND (output IS NOT NULL OR artifact_bundle IS NOT NULL)
             ORDER BY timestamp DESC, rowid DESC LIMIT 1",
            [thread_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    };
    let Some(target_id) = target_id else {
        return Ok(None);
    };
    ensure_payload_projection(conn, PayloadOwnerKind::Message, &target_id)?;
    let sql = format!(
        "SELECT {} FROM messages
         WHERE thread_id = ?1 AND id = ?2 AND deleted_at IS NULL LIMIT 1",
        core_version_columns()
    );
    let mut statement = conn.prepare(&sql)?;
    Ok(
        load_thread_message_rows_from_stmt(&mut statement, &[&thread_id, &target_id])?
            .into_iter()
            .next(),
    )
}

pub fn get_thread_version_detail(
    conn: &Connection,
    thread_id: &str,
    message_id: Option<&str>,
) -> SqlResult<Option<crate::contracts::VersionDetail>> {
    let Some(mut message) =
        get_thread_message_version_core_row(conn, thread_id, message_id)?.map(|row| row.message)
    else {
        return Ok(None);
    };
    let projection = ensure_payload_projection(conn, PayloadOwnerKind::Message, &message.id)?
        .unwrap_or_default();
    let counts = (
        projection.edge_count as i64,
        projection.face_count as i64,
        projection.selection_count as i64,
    );
    let mut truncated_fields = Vec::new();
    if counts.0 > 0 {
        truncated_fields.push("artifactBundle.edgeTargets".to_string());
    }
    if counts.1 > 0 {
        truncated_fields.push("artifactBundle.faceTargets".to_string());
    }
    if counts.2 > 0 {
        truncated_fields.push("modelManifest.selectionTargets".to_string());
    }
    let initial_bytes = serde_json::to_vec(&message)
        .map(|value| value.len())
        .unwrap_or(0);
    if initial_bytes > crate::transport_budget::VERSION_CORE_MAX_BYTES {
        message.content = crate::transport_budget::bounded_text(&message.content, 64 * 1024);
        if let Some(output) = message.output.as_mut() {
            output.response = crate::transport_budget::bounded_text(&output.response, 64 * 1024);
            output.macro_code =
                crate::transport_budget::bounded_text(&output.macro_code, 512 * 1024);
        }
        truncated_fields.push("content/source".to_string());
    }
    let observed_bytes = serde_json::to_vec(&message)
        .map(|value| value.len())
        .unwrap_or(0);
    Ok(Some(crate::contracts::VersionDetail {
        dense_topology_ref: (counts.0 + counts.1 + counts.2 > 0)
            .then(|| format!("topology:{thread_id}:{}", message.id)),
        edge_count: counts.0.max(0) as usize,
        face_count: counts.1.max(0) as usize,
        selection_target_count: counts.2.max(0) as usize,
        observed_bytes,
        truncated_fields,
        available_sections: vec!["sourceWindow".to_string(), "denseTopologyPage".to_string()],
        message,
    }))
}

pub fn get_version_source(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
) -> SqlResult<Option<(String, Option<String>)>> {
    conn.query_row(
        "SELECT json_extract(output, '$.macroCode'), version_input_digest
         FROM messages WHERE thread_id = ?1 AND id = ?2 AND deleted_at IS NULL
           AND output IS NOT NULL LIMIT 1",
        params![thread_id, message_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
}

pub fn get_dense_topology_json_page(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
    json_column: &str,
    json_path: &str,
    offset: usize,
    limit: usize,
) -> SqlResult<(Vec<String>, usize)> {
    match json_column {
        "artifact_bundle" | "model_manifest" => {}
        _ => {
            return Err(rusqlite::Error::InvalidParameterName(
                "Invalid topology column".into(),
            ))
        }
    }
    let field = match json_path {
        "$.edgeTargets" => DenseField::Edge,
        "$.faceTargets" => DenseField::Face,
        "$.selectionTargets" => DenseField::Selection,
        _ => {
            return Err(rusqlite::Error::InvalidParameterName(
                "Invalid topology path".into(),
            ))
        }
    };
    let valid = conn
        .query_row(
            "SELECT 1 FROM messages
             WHERE thread_id = ?1 AND id = ?2 AND deleted_at IS NULL LIMIT 1",
            params![thread_id, message_id],
            |_row| Ok(()),
        )
        .optional()?
        .is_some();
    if !valid {
        return Ok((Vec::new(), 0));
    }
    let projection =
        ensure_payload_projection(conn, PayloadOwnerKind::Message, message_id)?.unwrap_or_default();
    let total = match field {
        DenseField::Edge => projection.edge_count,
        DenseField::Face => projection.face_count,
        DenseField::Selection => projection.selection_count,
    };
    let items = match field {
        DenseField::Edge => load_topology_json_page::<crate::contracts::ViewerEdgeTarget>(
            conn,
            PayloadOwnerKind::Message,
            message_id,
            "edge",
            offset,
            limit,
        )?,
        DenseField::Face => load_topology_json_page::<crate::contracts::ViewerFaceTarget>(
            conn,
            PayloadOwnerKind::Message,
            message_id,
            "face",
            offset,
            limit,
        )?,
        DenseField::Selection => load_topology_json_page::<crate::contracts::SelectionTarget>(
            conn,
            PayloadOwnerKind::Message,
            message_id,
            "selection",
            offset,
            limit,
        )?,
    };
    Ok((items, total))
}

pub fn get_latest_pending_user_message_id(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT id
         FROM messages
         WHERE thread_id = ?1
           AND deleted_at IS NULL
           AND role = 'user'
           AND status = 'pending'
         ORDER BY timestamp DESC, rowid DESC
         LIMIT 1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_visible_message_role(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
) -> SqlResult<Option<MessageRole>> {
    conn.query_row(
        "SELECT role FROM messages
         WHERE thread_id = ?1 AND id = ?2 AND deleted_at IS NULL AND status != 'discarded'
         LIMIT 1",
        params![thread_id, message_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_pending_user_message_ids(conn: &Connection, thread_id: &str) -> SqlResult<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT id FROM messages
         WHERE thread_id = ?1 AND deleted_at IS NULL AND role = 'user' AND status = 'pending'
         ORDER BY timestamp ASC, rowid ASC",
    )?;
    let ids = statement
        .query_map([thread_id], |row| row.get(0))?
        .collect();
    ids
}

pub fn has_renderable_thread_version(
    conn: &Connection,
    thread_id: &str,
    message_id: Option<&str>,
) -> SqlResult<bool> {
    let found = if let Some(message_id) = message_id {
        conn.query_row(
            "SELECT 1 FROM messages
             WHERE thread_id = ?1 AND id = ?2 AND deleted_at IS NULL
               AND role = 'assistant' AND status = 'success' AND artifact_bundle IS NOT NULL
             LIMIT 1",
            params![thread_id, message_id],
            |_row| Ok(()),
        )
        .optional()?
    } else {
        conn.query_row(
            "SELECT 1 FROM messages
             WHERE thread_id = ?1 AND deleted_at IS NULL
               AND role = 'assistant' AND status = 'success' AND artifact_bundle IS NOT NULL
             ORDER BY timestamp DESC, rowid DESC LIMIT 1",
            [thread_id],
            |_row| Ok(()),
        )
        .optional()?
    };
    Ok(found.is_some())
}

pub fn get_message_attachment_images(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
) -> SqlResult<Option<Vec<String>>> {
    conn.query_row(
        "SELECT attachment_images FROM messages
         WHERE thread_id = ?1 AND id = ?2 AND deleted_at IS NULL LIMIT 1",
        params![thread_id, message_id],
        |row| {
            let raw: Option<String> = row.get(0)?;
            raw.map(|value| serde_json::from_str(&value).unwrap_or_default())
                .map_or(Ok(Vec::new()), Ok)
        },
    )
    .optional()
}

pub fn get_thread_messages_page(
    conn: &Connection,
    thread_id: &str,
    before: Option<&str>,
    limit: usize,
    _include_visual_payloads: bool,
) -> SqlResult<ThreadMessagesPage> {
    get_thread_messages_page_filtered(conn, thread_id, before, limit, None)
}

pub fn get_thread_messages_page_filtered(
    conn: &Connection,
    thread_id: &str,
    before: Option<&str>,
    limit: usize,
    roles: Option<&[MessageRole]>,
) -> SqlResult<ThreadMessagesPage> {
    use crate::contracts::{ThreadTimelineRow, ThreadTimelineVersionSummary};

    ensure_payload_projection_schema(conn)?;
    const TIMELINE_CONTENT_MAX_CHARS: i64 = 2_048;
    const TIMELINE_CONTENT_MAX_BYTES: usize = 8_192;
    const TIMELINE_PAGE_MAX_ROWS: usize = 50;
    let safe_limit = limit.clamp(1, TIMELINE_PAGE_MAX_ROWS);
    let cursor = before
        .map(|value| decode_thread_message_cursor(thread_id, value))
        .transpose()?;
    let cursor_predicate = if cursor.is_some() {
        "AND (timestamp < ?2 OR (timestamp = ?2 AND rowid < ?3))"
    } else {
        ""
    };
    let role_predicate = match roles {
        Some([MessageRole::User]) => "AND role = 'user'",
        Some([MessageRole::Assistant]) => "AND role = 'assistant'",
        Some(roles) if roles.is_empty() => "AND 0",
        _ => "",
    };
    let sql = format!(
        "SELECT
           id,
           role,
           substr(content, 1, {TIMELINE_CONTENT_MAX_CHARS}),
           length(CAST(content AS BLOB)) > {TIMELINE_CONTENT_MAX_BYTES},
           length(CAST(content AS BLOB)),
           status,
           agent_origin,
           timestamp,
           rowid,
           deleted_at,
           output IS NOT NULL,
           artifact_bundle IS NOT NULL,
           model_manifest IS NOT NULL,
           CASE WHEN json_valid(output) THEN substr(json_extract(output, '$.title'), 1, 256) END,
           CASE WHEN json_valid(output) THEN substr(json_extract(output, '$.versionName'), 1, 128) END,
           COALESCE(
             substr(payload_projections.model_id, 1, 256),
             NULL
           ),
           image_data IS NOT NULL,
           CASE WHEN json_valid(attachment_images) THEN json_array_length(attachment_images) ELSE 0 END,
           visual_kind
         FROM messages
         LEFT JOIN payload_projections
           ON payload_projections.owner_kind = 'message'
          AND payload_projections.owner_id = messages.id
         WHERE thread_id = ?1
           AND status != 'discarded'
           AND (
             deleted_at IS NULL
             OR (role = 'assistant' AND (output IS NOT NULL OR artifact_bundle IS NOT NULL))
           )
           AND NOT (
             role = 'assistant' AND status = 'error' AND agent_origin IS NOT NULL
             AND output IS NULL AND artifact_bundle IS NULL
           )
           {role_predicate}
           {cursor_predicate}
         ORDER BY timestamp DESC, rowid DESC
         LIMIT {}",
        safe_limit + 1,
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = if let Some((timestamp, row_id)) = cursor {
        stmt.query(params![thread_id, timestamp, row_id])?
    } else {
        stmt.query(params![thread_id])?
    };
    let mut projected = Vec::with_capacity(safe_limit + 1);
    while let Some(row) = rows.next()? {
        let has_output: bool = row.get(10)?;
        let has_runtime: bool = row.get(11)?;
        let has_manifest: bool = row.get(12)?;
        let deleted_at: Option<i64> = row.get(9)?;
        let mut status: MessageStatus = row.get(5)?;
        if deleted_at.is_some() {
            status = MessageStatus::Discarded;
        }
        let version_summary = if has_output || has_runtime {
            Some(ThreadTimelineVersionSummary {
                title: row.get(13)?,
                version_name: row.get(14)?,
                model_id: row.get(15)?,
                has_output,
                has_runtime,
                has_manifest,
            })
        } else {
            None
        };
        let agent_origin_raw: Option<String> = row.get(6)?;
        projected.push(ThreadTimelineRow {
            id: row.get(0)?,
            role: row.get(1)?,
            content: row.get(2)?,
            content_truncated: row.get(3)?,
            content_observed_bytes: row.get::<_, i64>(4)?.max(0) as usize,
            content_allowed_bytes: TIMELINE_CONTENT_MAX_BYTES,
            status,
            agent_origin: deserialize_agent_origin(agent_origin_raw.as_deref()),
            timestamp: row.get::<_, i64>(7)? as u64,
            timeline_order: row.get(8)?,
            version_summary,
            has_image: row.get(16)?,
            attachment_count: row.get::<_, i64>(17)?.max(0) as usize,
            visual_kind: row.get(18)?,
        });
    }

    let has_more = projected.len() > safe_limit;
    if has_more {
        projected.truncate(safe_limit);
    }
    let next_before = projected.last().map(|row| {
        encode_thread_message_cursor(thread_id, row.timestamp as i64, row.timeline_order)
    });
    projected.reverse();
    let observed_bytes = serde_json::to_vec(&projected)
        .map(|bytes| bytes.len())
        .unwrap_or(0);
    if observed_bytes > crate::transport_budget::TIMELINE_PAGE_MAX_BYTES {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "Timeline page is {observed_bytes} bytes; allowed {} bytes. Use a smaller page limit.",
            crate::transport_budget::TIMELINE_PAGE_MAX_BYTES
        )));
    }
    Ok(ThreadMessagesPage {
        messages: projected,
        next_before,
        has_more,
        observed_bytes,
        truncated_fields: vec![
            "output".to_string(),
            "artifactBundle".to_string(),
            "modelManifest".to_string(),
            "imageData".to_string(),
            "attachmentImages".to_string(),
        ],
    })
}

fn thread_cursor_identity(thread_id: &str) -> u64 {
    thread_id
        .as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

fn encode_thread_message_cursor(thread_id: &str, timestamp: i64, row_id: i64) -> String {
    format!(
        "v2:{:016x}:{timestamp}:{row_id}",
        thread_cursor_identity(thread_id)
    )
}

fn decode_thread_message_cursor(thread_id: &str, cursor: &str) -> SqlResult<(i64, i64)> {
    let mut parts = cursor.split(':');
    let valid_version = parts.next() == Some("v2");
    let valid_thread = parts
        .next()
        .is_some_and(|part| part == format!("{:016x}", thread_cursor_identity(thread_id)));
    let timestamp = parts.next().and_then(|part| part.parse::<i64>().ok());
    let row_id = parts.next().and_then(|part| part.parse::<i64>().ok());
    if valid_version && valid_thread && parts.next().is_none() {
        if let (Some(timestamp), Some(row_id)) = (timestamp, row_id) {
            return Ok((timestamp, row_id));
        }
    }
    Err(rusqlite::Error::InvalidParameterName(
        "Invalid thread timeline cursor.".to_string(),
    ))
}

pub fn get_thread_window_layout(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<crate::contracts::ThreadWindowLayout>> {
    conn.query_row(
        "SELECT layout_json FROM thread_window_layouts WHERE thread_id = ?1",
        [thread_id],
        |row| {
            let raw: String = row.get(0)?;
            serde_json::from_str(&raw).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })
        },
    )
    .optional()
}

pub fn save_thread_window_layout(
    conn: &Connection,
    thread_id: &str,
    layout: &crate::contracts::ThreadWindowLayout,
    updated_at: i64,
) -> SqlResult<bool> {
    let thread_exists = conn
        .query_row(
            "SELECT 1 FROM threads WHERE id = ?1 AND deleted_at IS NULL",
            [thread_id],
            |_row| Ok(()),
        )
        .optional()?
        .is_some();
    if !thread_exists {
        return Ok(false);
    }

    let layout_json = serde_json::to_string(layout)
        .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
    conn.execute(
        "INSERT INTO thread_window_layouts (thread_id, layout_json, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(thread_id) DO UPDATE SET
           layout_json = excluded.layout_json,
           updated_at = excluded.updated_at",
        params![thread_id, layout_json, updated_at],
    )?;
    Ok(true)
}

pub fn get_thread_messages_for_context(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Vec<Message>> {
    const CONTEXT_MESSAGE_QUERY_LIMIT: usize = 10;
    let mut rows = load_thread_message_rows_with_clause_and_projection(
        conn,
        "thread_id = ?1 AND status != 'discarded'",
        &[&thread_id],
        "timestamp DESC, rowid DESC",
        Some(CONTEXT_MESSAGE_QUERY_LIMIT),
        true,
    )?;
    rows.reverse();
    Ok(filter_thread_messages_for_context(&rows))
}

pub fn get_recent_thread_messages_for_summary(
    conn: &Connection,
    thread_id: &str,
    limit: usize,
) -> SqlResult<Vec<Message>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut rows = load_thread_message_rows_with_clause_and_projection(
        conn,
        "thread_id = ?1 AND status != 'discarded'",
        &[&thread_id],
        "timestamp DESC, rowid DESC",
        Some(limit),
        true,
    )?;
    rows.reverse();
    Ok(filter_thread_messages_for_context(&rows))
}

fn filter_thread_messages_for_context(rows: &[ThreadMessageRow]) -> Vec<Message> {
    let mut messages = Vec::new();

    for (index, row) in rows.iter().enumerate() {
        if row.deleted_at.is_some() || row.message.status == MessageStatus::Discarded {
            continue;
        }
        if is_agent_tool_error_message(&row.message) {
            continue;
        }

        let skip_deleted_version_prompt = row.message.role == MessageRole::User
            && rows
                .get(index + 1)
                .map(|next| {
                    next.deleted_at.is_some()
                        && next.message.role == MessageRole::Assistant
                        && is_version_message(&next.message)
                        && next.message.status != MessageStatus::Discarded
                        && next.message.timestamp.saturating_sub(row.message.timestamp) <= 2
                })
                .unwrap_or(false);

        if skip_deleted_version_prompt {
            continue;
        }

        messages.push(row.message.clone());
    }

    messages
}

fn load_thread_message_rows(
    conn: &Connection,
    thread_id: &str,
    include_deleted: bool,
) -> SqlResult<Vec<ThreadMessageRow>> {
    let sql = if include_deleted {
        "SELECT id, role, content, status, output, usage, artifact_bundle, model_manifest, structural_verification, agent_origin, timestamp, image_data, visual_kind, attachment_images, deleted_at, version_input_digest, runtime_cache_key
         FROM messages
         WHERE thread_id = ?1 AND status != 'discarded'
         ORDER BY timestamp ASC, rowid ASC"
    } else {
        "SELECT id, role, content, status, output, usage, artifact_bundle, model_manifest, structural_verification, agent_origin, timestamp, image_data, visual_kind, attachment_images, deleted_at, version_input_digest, runtime_cache_key
         FROM messages
         WHERE thread_id = ?1 AND status != 'discarded' AND deleted_at IS NULL
         ORDER BY timestamp ASC, rowid ASC"
    };

    let mut stmt = conn.prepare(sql)?;
    let mut rows = load_thread_message_rows_from_stmt(&mut stmt, &[&thread_id])?;
    drop(stmt);
    hydrate_message_payloads(conn, &mut rows)?;
    Ok(rows)
}

fn load_thread_message_rows_with_clause(
    conn: &Connection,
    where_clause: &str,
    params: &[&dyn rusqlite::ToSql],
    order_by: &str,
    limit: Option<usize>,
) -> SqlResult<Vec<ThreadMessageRow>> {
    load_thread_message_rows_with_clause_and_projection(
        conn,
        where_clause,
        params,
        order_by,
        limit,
        false,
    )
}

fn load_thread_message_rows_with_clause_and_projection(
    conn: &Connection,
    where_clause: &str,
    params: &[&dyn rusqlite::ToSql],
    order_by: &str,
    limit: Option<usize>,
    compact_visual_payloads: bool,
) -> SqlResult<Vec<ThreadMessageRow>> {
    let columns = if compact_visual_payloads {
        "id, role, substr(content, 1, 8192), status,
         CASE WHEN json_valid(output) THEN json_object(
           'title', COALESCE(json_extract(output, '$.title'), ''),
           'versionName', COALESCE(json_extract(output, '$.versionName'), ''),
           'response', substr(COALESCE(json_extract(output, '$.response'), ''), 1, 1024),
           'macroCode', ''
         ) ELSE NULL END,
         usage,
         NULL,
         NULL,
         NULL,
         agent_origin,
         timestamp,
         NULL,
         visual_kind,
         NULL,
         deleted_at,
         version_input_digest,
         runtime_cache_key"
    } else {
        "id, role, content, status, output, usage, artifact_bundle, model_manifest,
         structural_verification, agent_origin, timestamp, image_data, visual_kind,
         attachment_images, deleted_at, version_input_digest, runtime_cache_key"
    };
    let mut sql = format!(
        "SELECT {}
         FROM messages
         WHERE {}
         ORDER BY {}",
        columns, where_clause, order_by
    );
    if let Some(limit) = limit {
        sql.push_str(" LIMIT ");
        sql.push_str(&limit.to_string());
    }
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = load_thread_message_rows_from_stmt(&mut stmt, params)?;
    drop(stmt);
    if !compact_visual_payloads {
        hydrate_message_payloads(conn, &mut rows)?;
    }
    Ok(rows)
}

fn hydrate_message_payloads(conn: &Connection, rows: &mut [ThreadMessageRow]) -> SqlResult<()> {
    for row in rows {
        if row.message.artifact_bundle.is_none() && row.message.model_manifest.is_none() {
            continue;
        }
        let (artifact, manifest) =
            load_payload_full(conn, PayloadOwnerKind::Message, &row.message.id)?;
        row.message.artifact_bundle = artifact;
        row.message.model_manifest = manifest;
    }
    Ok(())
}

fn load_thread_message_rows_from_stmt(
    stmt: &mut rusqlite::Statement<'_>,
    params: &[&dyn rusqlite::ToSql],
) -> SqlResult<Vec<ThreadMessageRow>> {
    let msg_iter = stmt.query_map(params, |row| {
        let output_str: Option<String> = row.get(4)?;
        let output: Option<DesignOutput> =
            output_str.and_then(|s| serde_json::from_str(&s).ok().map(normalize_design_output));
        let usage_str: Option<String> = row.get(5)?;
        let usage = usage_str.and_then(|s| serde_json::from_str(&s).ok());
        let artifact_bundle_blob: Option<Vec<u8>> = row.get(6)?;
        let mut artifact_bundle = artifact_bundle_blob
            .as_deref()
            .map(decode_payload)
            .transpose()?;
        let model_manifest_blob: Option<Vec<u8>> = row.get(7)?;
        let mut model_manifest = model_manifest_blob
            .as_deref()
            .map(decode_payload)
            .transpose()?;
        let structural_verification_str: Option<String> = row.get(8)?;
        let structural_verification =
            structural_verification_str.and_then(|s| serde_json::from_str(&s).ok());
        let agent_origin_str: Option<String> = row.get(9)?;
        let agent_origin = deserialize_agent_origin(agent_origin_str.as_deref());
        let visual_kind = row.get(12)?;
        let attachment_images_str: Option<String> = row.get(13)?;
        let attachment_images = attachment_images_str
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let message_id: String = row.get(0)?;
        let stored_version_input_digest: Option<String> = row.get(15)?;
        let stored_runtime_cache_key: Option<String> = row.get(16)?;
        if artifact_bundle.is_some() {
            let (expected_version_input_digest, expected_runtime_cache_key) =
                version_runtime_binding(&message_id, output.as_ref(), artifact_bundle.as_ref())?;
            let legacy_unbound =
                stored_version_input_digest.is_none() && stored_runtime_cache_key.is_none();
            if !legacy_unbound
                && (stored_version_input_digest != expected_version_input_digest
                    || stored_runtime_cache_key != expected_runtime_cache_key)
            {
                artifact_bundle = None;
                model_manifest = None;
            }
        }

        Ok(ThreadMessageRow {
            message: Message {
                id: message_id,
                role: row.get(1)?,
                content: row.get(2)?,
                status: row.get(3)?,
                output,
                usage,
                artifact_bundle,
                model_manifest,
                structural_verification,
                agent_origin,
                timestamp: row.get::<_, i64>(10)? as u64,
                image_data: row.get(11)?,
                visual_kind,
                attachment_images,
            },
            deleted_at: row.get(14)?,
        })
    })?;

    let mut messages = Vec::new();
    for msg in msg_iter {
        messages.push(msg?);
    }
    Ok(messages)
}

pub(crate) fn is_version_message(message: &Message) -> bool {
    message.role == MessageRole::Assistant
        && (message.output.is_some() || message.artifact_bundle.is_some())
}

fn is_agent_tool_error_message(message: &Message) -> bool {
    message.role == MessageRole::Assistant
        && message.status == MessageStatus::Error
        && message.agent_origin.is_some()
        && message.output.is_none()
        && message.artifact_bundle.is_none()
}

pub fn get_thread_references(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Vec<ThreadReference>> {
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, source_message_id, ordinal, kind, name, content, summary, pinned, created_at
         FROM thread_references
         WHERE thread_id = ?1 AND pinned = 1
         ORDER BY created_at ASC, ordinal ASC"
    )?;
    let iter = stmt.query_map([thread_id], |row| {
        Ok(ThreadReference {
            id: row.get(0)?,
            thread_id: row.get(1)?,
            source_message_id: row.get(2)?,
            ordinal: row.get(3)?,
            kind: row.get(4)?,
            name: row.get(5)?,
            content: row.get(6)?,
            summary: row.get(7)?,
            pinned: row.get::<_, i64>(8)? != 0,
            created_at: row.get::<_, i64>(9)? as u64,
        })
    })?;
    let mut refs = Vec::new();
    for item in iter {
        refs.push(item?);
    }
    Ok(refs)
}

pub fn get_message_references(
    conn: &Connection,
    message_id: &str,
) -> SqlResult<Vec<ThreadReference>> {
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, source_message_id, ordinal, kind, name, content, summary, pinned, created_at
         FROM thread_references
         WHERE source_message_id = ?1
         ORDER BY created_at ASC, ordinal ASC",
    )?;
    let iter = stmt.query_map([message_id], |row| {
        Ok(ThreadReference {
            id: row.get(0)?,
            thread_id: row.get(1)?,
            source_message_id: row.get(2)?,
            ordinal: row.get(3)?,
            kind: row.get(4)?,
            name: row.get(5)?,
            content: row.get(6)?,
            summary: row.get(7)?,
            pinned: row.get::<_, i64>(8)? != 0,
            created_at: row.get::<_, i64>(9)? as u64,
        })
    })?;
    let mut refs = Vec::new();
    for item in iter {
        refs.push(item?);
    }
    Ok(refs)
}

pub fn clear_history(conn: &Connection) -> SqlResult<()> {
    conn.execute("DELETE FROM threads", [])?;
    Ok(())
}

pub fn mark_interrupted_pending_messages(conn: &Connection) -> SqlResult<usize> {
    conn.execute(
        "UPDATE messages
         SET status = 'error',
             content = 'Request interrupted by app restart before provider response completed. Retry the last prompt.'
         WHERE role = 'assistant'
           AND status = 'pending'
           AND deleted_at IS NULL",
        [],
    )
}

pub fn update_message_status_and_output(
    conn: &Connection,
    message_id: &str,
    update: MessageStatusUpdate<'_>,
) -> SqlResult<()> {
    let MessageStatusUpdate {
        status,
        output,
        usage,
        artifact_bundle,
        model_manifest,
        structural_verification,
        visual_kind,
        content,
    } = update;
    let output_str = output.and_then(|o| serde_json::to_string(o).ok());
    let usage_str = usage.and_then(|value| serde_json::to_string(value).ok());
    let encoded_payload = encode_cad_payload(artifact_bundle, model_manifest)?;
    let structural_verification_str =
        structural_verification.and_then(|value| serde_json::to_string(value).ok());
    let (version_input_digest, runtime_cache_key) =
        version_runtime_binding(message_id, output, artifact_bundle)?;
    conn.execute_batch("SAVEPOINT update_message_payload")?;
    let write_result = (|| {
        if let Some(text) = content {
            conn.execute(
            "UPDATE messages SET status = ?1, output = ?2, usage = ?3, artifact_bundle = ?4, model_manifest = ?5, structural_verification = ?6, visual_kind = COALESCE(?7, visual_kind), content = ?8, version_input_digest = ?9, runtime_cache_key = ?10 WHERE id = ?11",
            params![
                status,
                output_str,
                usage_str,
                encoded_payload.artifact_core.as_deref(),
                encoded_payload.model_manifest_core.as_deref(),
                structural_verification_str,
                visual_kind,
                text,
                version_input_digest,
                runtime_cache_key,
                message_id
            ],
        )?;
        } else {
            conn.execute(
            "UPDATE messages SET status = ?1, output = ?2, usage = ?3, artifact_bundle = ?4, model_manifest = ?5, structural_verification = ?6, visual_kind = COALESCE(?7, visual_kind), version_input_digest = ?8, runtime_cache_key = ?9 WHERE id = ?10",
            params![
                status,
                output_str,
                usage_str,
                encoded_payload.artifact_core.as_deref(),
                encoded_payload.model_manifest_core.as_deref(),
                structural_verification_str,
                visual_kind,
                version_input_digest,
                runtime_cache_key,
                message_id
            ],
        )?;
        }
        store_payload_sidecars_from_structs(
            conn,
            PayloadOwnerKind::Message,
            message_id,
            artifact_bundle,
            model_manifest,
            &encoded_payload.projection,
        )?;
        if artifact_bundle.is_some() {
            if let Some(thread_id) = get_message_thread_id(conn, message_id)? {
                prune_non_latest_thread_stls(conn, &thread_id)?;
            }
        }
        Ok(())
    })();
    match write_result {
        Ok(()) => conn.execute_batch("RELEASE update_message_payload")?,
        Err(error) => {
            let _ = conn.execute_batch(
                "ROLLBACK TO update_message_payload; RELEASE update_message_payload;",
            );
            return Err(error);
        }
    }
    Ok(())
}

pub struct MessageStatusUpdate<'a> {
    pub status: &'a MessageStatus,
    pub output: Option<&'a DesignOutput>,
    pub usage: Option<&'a crate::contracts::UsageSummary>,
    pub artifact_bundle: Option<&'a crate::contracts::ArtifactBundle>,
    pub model_manifest: Option<&'a crate::contracts::ModelManifest>,
    pub structural_verification: Option<&'a crate::contracts::StructuralVerificationResult>,
    pub visual_kind: Option<&'a crate::contracts::MessageVisualKind>,
    pub content: Option<&'a str>,
}

pub fn delete_thread(conn: &Connection, id: &str) -> SqlResult<bool> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let changed = conn.execute(
        "UPDATE threads SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        params![now, id],
    )?;
    Ok(changed > 0)
}

pub fn delete_message(conn: &Connection, id: &str) -> SqlResult<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    conn.execute(
        "UPDATE messages SET deleted_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn restore_message(conn: &Connection, id: &str) -> SqlResult<()> {
    conn.execute("UPDATE messages SET deleted_at = NULL WHERE id = ?", [id])?;
    Ok(())
}

fn unix_now_i64() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[derive(Debug, Clone)]
struct MessageContextInfo {
    thread_id: String,
}

fn get_message_context_info(conn: &Connection, id: &str) -> SqlResult<Option<MessageContextInfo>> {
    conn.query_row(
        "SELECT thread_id
         FROM messages
         WHERE id = ?1",
        [id],
        |row| {
            Ok(MessageContextInfo {
                thread_id: row.get(0)?,
            })
        },
    )
    .optional()
}

fn set_thread_deleted_at(
    conn: &Connection,
    thread_id: &str,
    deleted_at: Option<i64>,
) -> SqlResult<()> {
    conn.execute(
        "UPDATE threads SET deleted_at = ?1 WHERE id = ?2",
        params![deleted_at, thread_id],
    )?;
    Ok(())
}

pub fn has_visible_messages(conn: &Connection, thread_id: &str) -> SqlResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE thread_id = ?1 AND status != 'discarded' AND deleted_at IS NULL",
        [thread_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn delete_version_cluster(conn: &Connection, id: &str) -> SqlResult<Option<String>> {
    let Some(message) = get_message_context_info(conn, id)? else {
        return Ok(None);
    };
    let now = unix_now_i64();

    conn.execute(
        "UPDATE messages SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        params![now, id],
    )?;

    if !has_visible_messages(conn, &message.thread_id)? {
        set_thread_deleted_at(conn, &message.thread_id, Some(now))?;
        update_thread_summary(conn, &message.thread_id, "")?;
    }

    Ok(Some(message.thread_id))
}

pub fn restore_version_cluster(conn: &Connection, id: &str) -> SqlResult<Option<String>> {
    let Some(message) = get_message_context_info(conn, id)? else {
        return Ok(None);
    };
    let now = unix_now_i64();

    conn.execute(
        "UPDATE messages SET deleted_at = NULL, trash_hidden_at = NULL, timestamp = ?2 WHERE id = ?1",
        params![id, now],
    )?;

    set_thread_deleted_at(conn, &message.thread_id, None)?;
    conn.execute(
        "UPDATE threads SET updated_at = ?1 WHERE id = ?2",
        params![now, message.thread_id],
    )?;
    Ok(Some(message.thread_id))
}

pub fn get_deleted_threads_page(
    conn: &Connection,
    before: Option<&str>,
    limit: usize,
) -> SqlResult<DeletedThreadsPage> {
    let safe_limit = limit.clamp(1, 100);
    let (before_deleted_at, before_id) = match before {
        Some(cursor) => {
            let (deleted_at, id) = cursor
                .split_once(':')
                .ok_or_else(|| rusqlite::Error::InvalidParameterName("before".to_string()))?;
            let deleted_at = deleted_at
                .parse::<i64>()
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            (Some(deleted_at), Some(id.to_string()))
        }
        None => (None, None),
    };

    let mut stmt = conn.prepare(
        "
        SELECT t.id, t.title, COALESCE(t.summary, ''), t.updated_at, t.deleted_at,
               (
                   SELECT COUNT(*)
                   FROM messages m
                   WHERE m.thread_id = t.id
                     AND m.role = 'assistant'
                     AND m.status != 'discarded'
                     AND m.artifact_bundle IS NOT NULL
                     AND m.deleted_at IS NULL
               ) AS version_count
        FROM threads t
        WHERE t.deleted_at IS NOT NULL
          AND (
              ?1 IS NULL
              OR t.deleted_at < ?1
              OR (t.deleted_at = ?1 AND t.id < ?2)
          )
        ORDER BY t.deleted_at DESC, t.id DESC
        LIMIT ?3
        ",
    )?;
    let iter = stmt.query_map(
        params![before_deleted_at, before_id, (safe_limit + 1) as i64],
        |row| {
            Ok(DeletedThreadSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                summary: row.get(2)?,
                updated_at: row.get::<_, i64>(3)? as u64,
                deleted_at: row.get::<_, i64>(4)? as u64,
                version_count: row.get::<_, i64>(5)? as usize,
            })
        },
    )?;

    let mut items = iter.collect::<SqlResult<Vec<_>>>()?;
    let has_more = items.len() > safe_limit;
    if has_more {
        items.truncate(safe_limit);
    }
    let next_before = if has_more {
        items
            .last()
            .map(|item| format!("{}:{}", item.deleted_at, item.id))
    } else {
        None
    };

    Ok(DeletedThreadsPage {
        items,
        next_before,
        has_more,
    })
}

pub fn restore_deleted_thread(conn: &Connection, id: &str) -> SqlResult<bool> {
    let now = unix_now_i64();
    let changed = conn.execute(
        "
        UPDATE threads
        SET deleted_at = NULL,
            status = 'active',
            finalized_at = NULL,
            updated_at = ?1
        WHERE id = ?2
          AND deleted_at IS NOT NULL
        ",
        params![now, id],
    )?;
    Ok(changed > 0)
}

fn get_previous_thread_preview_for_deleted_state(
    conn: &Connection,
    thread_id: &str,
    deleted: bool,
) -> SqlResult<Option<String>> {
    conn.query_row(
        "
        SELECT messages.image_data
        FROM messages
        JOIN threads ON threads.id = messages.thread_id
        WHERE messages.thread_id = ?1
          AND ((?2 = 1 AND threads.deleted_at IS NOT NULL)
            OR (?2 = 0 AND threads.deleted_at IS NULL))
          AND messages.role = 'assistant'
          AND messages.status = 'success'
          AND messages.artifact_bundle IS NOT NULL
          AND messages.deleted_at IS NULL
          AND messages.image_data IS NOT NULL
        ORDER BY messages.timestamp DESC, messages.rowid DESC
        LIMIT 1
        ",
        params![thread_id, if deleted { 1 } else { 0 }],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_thread_preview(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    let current = conn
        .query_row(
            "
        SELECT messages.image_data
        FROM messages
        JOIN threads ON threads.id = messages.thread_id
        WHERE messages.thread_id = ?1
          AND threads.deleted_at IS NULL
          AND messages.role = 'assistant'
          AND messages.status != 'discarded'
          AND (messages.output IS NOT NULL OR messages.artifact_bundle IS NOT NULL)
          AND messages.deleted_at IS NULL
        ORDER BY messages.timestamp DESC, messages.rowid DESC
        LIMIT 1
        ",
            [thread_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?;
    Ok(current.flatten())
}

pub fn get_deleted_thread_preview(conn: &Connection, thread_id: &str) -> SqlResult<Option<String>> {
    get_previous_thread_preview_for_deleted_state(conn, thread_id, true)
}

pub fn get_deleted_messages(conn: &Connection) -> SqlResult<Vec<DeletedMessage>> {
    let mut stmt = conn.prepare("
        SELECT m.id, m.thread_id, t.title as thread_title, m.role, m.content, m.output, m.usage, m.artifact_bundle, m.model_manifest, m.structural_verification, m.agent_origin, m.timestamp, m.image_data, m.visual_kind, m.attachment_images, m.deleted_at
        FROM messages m
        JOIN threads t ON m.thread_id = t.id
        WHERE m.deleted_at IS NOT NULL
          AND m.trash_hidden_at IS NULL
          AND m.role = 'assistant'
          AND (m.output IS NOT NULL OR m.artifact_bundle IS NOT NULL)
        ORDER BY m.deleted_at DESC
    ")?;
    let iter = stmt.query_map([], |row| {
        let output_str: Option<String> = row.get(5)?;
        let output: Option<DesignOutput> = if let Some(json_str) = output_str {
            serde_json::from_str(&json_str)
                .ok()
                .map(normalize_design_output)
        } else {
            None
        };
        let usage_str: Option<String> = row.get(6)?;
        let usage = usage_str.and_then(|json_str| serde_json::from_str(&json_str).ok());
        let artifact_bundle_blob: Option<Vec<u8>> = row.get(7)?;
        let artifact_bundle = artifact_bundle_blob
            .as_deref()
            .map(decode_payload)
            .transpose()?;
        let model_manifest_blob: Option<Vec<u8>> = row.get(8)?;
        let model_manifest = model_manifest_blob
            .as_deref()
            .map(decode_payload)
            .transpose()?;
        let structural_verification_str: Option<String> = row.get(9)?;
        let structural_verification =
            structural_verification_str.and_then(|json_str| serde_json::from_str(&json_str).ok());
        let agent_origin_str: Option<String> = row.get(10)?;
        let agent_origin = deserialize_agent_origin(agent_origin_str.as_deref());
        let visual_kind = row.get(13)?;
        let attachment_images_str: Option<String> = row.get(14)?;
        let attachment_images = attachment_images_str
            .and_then(|json_str| serde_json::from_str(&json_str).ok())
            .unwrap_or_default();

        Ok(DeletedMessage {
            id: row.get(0)?,
            thread_id: row.get(1)?,
            thread_title: row.get(2)?,
            role: row.get(3)?,
            content: row.get(4)?,
            output,
            usage,
            artifact_bundle,
            model_manifest,
            structural_verification,
            agent_origin,
            timestamp: row.get::<_, i64>(11)? as u64,
            image_data: row.get(12)?,
            visual_kind,
            attachment_images,
            deleted_at: row.get::<_, i64>(15)? as u64,
        })
    })?;

    let mut results = Vec::new();
    for item in iter {
        results.push(item?);
    }
    Ok(results)
}

pub fn hide_deleted_message(conn: &Connection, id: &str) -> SqlResult<bool> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let changed = conn.execute(
        "UPDATE messages
         SET trash_hidden_at = ?1
         WHERE id = ?2
           AND deleted_at IS NOT NULL
           AND trash_hidden_at IS NULL",
        params![now, id],
    )?;
    Ok(changed > 0)
}

pub fn update_message_ui_spec(
    conn: &Connection,
    message_id: &str,
    ui_spec: &UiSpec,
) -> SqlResult<()> {
    let output_str: Option<String> = conn.query_row(
        "SELECT output FROM messages WHERE id = ?1",
        [message_id],
        |row| row.get(0),
    )?;

    if let Some(json_str) = output_str {
        let parsed: DesignOutput = serde_json::from_str(&json_str)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let mut output: DesignOutput = normalize_design_output(parsed);
        output.ui_spec = ui_spec.clone();
        let updated = serde_json::to_string(&output)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let (version_input_digest, _) = version_runtime_binding(message_id, Some(&output), None)?;
        conn.execute(
            "UPDATE messages SET output = ?1, version_input_digest = ?2, runtime_cache_key = NULL WHERE id = ?3",
            params![updated, version_input_digest, message_id],
        )?;
    }
    Ok(())
}

pub fn update_message_parameters(
    conn: &Connection,
    message_id: &str,
    parameters: &DesignParams,
) -> SqlResult<()> {
    let output_str: Option<String> = conn.query_row(
        "SELECT output FROM messages WHERE id = ?1",
        [message_id],
        |row| row.get(0),
    )?;

    if let Some(json_str) = output_str {
        let parsed: DesignOutput = serde_json::from_str(&json_str)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let mut output: DesignOutput = normalize_design_output(parsed);
        output.initial_params = parameters.clone();
        let updated = serde_json::to_string(&output)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let (version_input_digest, _) = version_runtime_binding(message_id, Some(&output), None)?;
        conn.execute(
            "UPDATE messages SET output = ?1, version_input_digest = ?2, runtime_cache_key = NULL WHERE id = ?3",
            params![updated, version_input_digest, message_id],
        )?;
    }
    Ok(())
}

pub fn update_message_model_manifest(
    conn: &Connection,
    message_id: &str,
    manifest: &crate::contracts::ModelManifest,
) -> SqlResult<()> {
    let mut projection =
        ensure_payload_projection(conn, PayloadOwnerKind::Message, message_id)?.unwrap_or_default();
    let encoded = encode_payload(&model_manifest_core(manifest))?;
    projection.model_id = Some(manifest.model_id.clone());
    projection.selection_count = manifest.selection_targets.len();
    conn.execute_batch("SAVEPOINT update_manifest_payload")?;
    let write_result = (|| {
        conn.execute(
            "UPDATE messages SET model_manifest = ?1 WHERE id = ?2",
            params![encoded, message_id],
        )?;
        store_payload_projection(conn, PayloadOwnerKind::Message, message_id, &projection)?;
        replace_topology_chunks(
            conn,
            PayloadOwnerKind::Message,
            message_id,
            "selection",
            &manifest.selection_targets,
        )?;
        Ok(())
    })();
    match write_result {
        Ok(()) => conn.execute_batch("RELEASE update_manifest_payload")?,
        Err(error) => {
            let _ = conn.execute_batch(
                "ROLLBACK TO update_manifest_payload; RELEASE update_manifest_payload;",
            );
            return Err(error);
        }
    }
    Ok(())
}

pub fn update_message_artifact_bundle(
    conn: &Connection,
    message_id: &str,
    bundle: &crate::contracts::ArtifactBundle,
) -> SqlResult<()> {
    let mut projection =
        ensure_payload_projection(conn, PayloadOwnerKind::Message, message_id)?.unwrap_or_default();
    let encoded = encode_payload(&artifact_bundle_core(bundle))?;
    projection.model_id = Some(bundle.model_id.clone());
    projection.edge_count = bundle.edge_targets.len();
    projection.face_count = bundle.face_targets.len();
    let output = conn
        .query_row(
            "SELECT output FROM messages WHERE id = ?1",
            [message_id],
            |row| row.get::<_, Option<String>>(0),
        )?
        .map(|json| serde_json::from_str::<DesignOutput>(&json).map(normalize_design_output))
        .transpose()
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let (version_input_digest, runtime_cache_key) =
        version_runtime_binding(message_id, output.as_ref(), Some(bundle))?;
    conn.execute_batch("SAVEPOINT update_artifact_payload")?;
    let write_result = (|| {
        conn.execute(
        "UPDATE messages SET artifact_bundle = ?1, version_input_digest = ?2, runtime_cache_key = ?3 WHERE id = ?4",
        params![encoded, version_input_digest, runtime_cache_key, message_id],
    )?;
        store_payload_projection(conn, PayloadOwnerKind::Message, message_id, &projection)?;
        replace_topology_chunks(
            conn,
            PayloadOwnerKind::Message,
            message_id,
            "edge",
            &bundle.edge_targets,
        )?;
        replace_topology_chunks(
            conn,
            PayloadOwnerKind::Message,
            message_id,
            "face",
            &bundle.face_targets,
        )?;
        if let Some(thread_id) = get_message_thread_id(conn, message_id)? {
            prune_non_latest_thread_stls(conn, &thread_id)?;
        }
        Ok(())
    })();
    match write_result {
        Ok(()) => conn.execute_batch("RELEASE update_artifact_payload")?,
        Err(error) => {
            let _ = conn.execute_batch(
                "ROLLBACK TO update_artifact_payload; RELEASE update_artifact_payload;",
            );
            return Err(error);
        }
    }
    Ok(())
}

/// Persisted dependency locks are GC roots. Invalid binary core aborts root
/// collection: retaining too much is safer than deleting historical payloads.
pub fn component_dependency_package_digests(
    conn: &Connection,
) -> SqlResult<std::collections::BTreeSet<String>> {
    let mut statement = conn.prepare(
        "SELECT id
         FROM messages
         WHERE artifact_bundle IS NOT NULL
           AND deleted_at IS NULL
           AND status != 'discarded'",
    )?;
    let message_ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<SqlResult<Vec<_>>>()?;
    drop(statement);
    let mut roots = std::collections::BTreeSet::new();
    for message_id in message_ids {
        let (bundle, _, _) = load_payload_core(conn, PayloadOwnerKind::Message, &message_id)?;
        let Some(bundle) = bundle else {
            continue;
        };
        if let Some(lock) = bundle.component_dependency_lock {
            for dependency in lock.dependencies {
                roots.insert(dependency.package_digest);
            }
        }
    }
    Ok(roots)
}

pub fn update_message_structural_verification(
    conn: &Connection,
    message_id: &str,
    result: Option<&crate::contracts::StructuralVerificationResult>,
) -> SqlResult<()> {
    let serialized = match result {
        Some(result) => Some(
            serde_json::to_string(result)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        ),
        None => None,
    };
    conn.execute(
        "UPDATE messages SET structural_verification = ?1 WHERE id = ?2",
        params![serialized, message_id],
    )?;
    Ok(())
}

pub fn upsert_agent_draft(conn: &Connection, draft: &AgentDraft) -> SqlResult<()> {
    let design_output = serialize_json(&draft.design_output)?;
    let encoded_payload =
        encode_cad_payload(Some(&draft.artifact_bundle), Some(&draft.model_manifest))?;
    let draft_feedback = match &draft.draft_feedback {
        Some(feedback) => Some(serialize_json(feedback)?),
        None => None,
    };
    let replaced_preview_id = conn
        .query_row(
            "SELECT preview_id FROM agent_drafts WHERE session_id = ?1 AND thread_id = ?2",
            params![draft.session_id, draft.thread_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    conn.execute_batch("SAVEPOINT upsert_agent_draft_payload")?;
    let write_result = (|| {
        conn.execute(
            "INSERT INTO agent_drafts (
            preview_id,
            session_id,
            thread_id,
            base_message_id,
            design_output,
            artifact_bundle,
            model_manifest,
            draft_feedback,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(session_id, thread_id) DO UPDATE SET
            preview_id = excluded.preview_id,
            thread_id = excluded.thread_id,
            base_message_id = excluded.base_message_id,
            design_output = excluded.design_output,
            artifact_bundle = excluded.artifact_bundle,
            model_manifest = excluded.model_manifest,
            draft_feedback = excluded.draft_feedback,
            updated_at = excluded.updated_at",
            params![
                draft.preview_id,
                draft.session_id,
                draft.thread_id,
                draft.base_message_id,
                design_output,
                encoded_payload.artifact_core.as_deref(),
                encoded_payload.model_manifest_core.as_deref(),
                draft_feedback,
                draft.updated_at as i64,
            ],
        )?;
        if let Some(old_preview_id) = replaced_preview_id.as_deref() {
            if old_preview_id != draft.preview_id {
                conn.execute(
                    "DELETE FROM payload_projections WHERE owner_kind = 'draft' AND owner_id = ?1",
                    [old_preview_id],
                )?;
                conn.execute(
                "DELETE FROM dense_topology_chunks WHERE owner_kind = 'draft' AND owner_id = ?1",
                [old_preview_id],
            )?;
            }
        }
        store_payload_sidecars_from_structs(
            conn,
            PayloadOwnerKind::Draft,
            &draft.preview_id,
            Some(&draft.artifact_bundle),
            Some(&draft.model_manifest),
            &encoded_payload.projection,
        )?;
        Ok(())
    })();
    match write_result {
        Ok(()) => conn.execute_batch("RELEASE upsert_agent_draft_payload")?,
        Err(error) => {
            let _ = conn.execute_batch(
                "ROLLBACK TO upsert_agent_draft_payload; RELEASE upsert_agent_draft_payload;",
            );
            return Err(error);
        }
    }
    Ok(())
}

pub fn upsert_verification_record(
    conn: &Connection,
    preview_id: &str,
    record: &crate::contracts::VerificationRecord,
    verified_at: u64,
) -> SqlResult<()> {
    let artifact_digest = serde_json::to_string(&record.artifact_digest)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let verification_record = serde_json::to_string(record)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    conn.execute(
        "INSERT INTO verification_records (
            snapshot_id,
            preview_id,
            artifact_digest,
            verification_record,
            verified_at
        ) VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(snapshot_id) DO UPDATE SET
            preview_id = excluded.preview_id,
            artifact_digest = excluded.artifact_digest,
            verification_record = excluded.verification_record,
            verified_at = excluded.verified_at",
        params![
            record.snapshot_id,
            preview_id,
            artifact_digest,
            verification_record,
            verified_at as i64,
        ],
    )?;
    Ok(())
}

pub fn get_verification_record(
    conn: &Connection,
    snapshot_id: &str,
) -> SqlResult<Option<crate::contracts::VerificationRecord>> {
    conn.query_row(
        "SELECT verification_record FROM verification_records WHERE snapshot_id = ?1",
        params![snapshot_id],
        |row| {
            let serialized: String = row.get(0)?;
            serde_json::from_str(&serialized).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        },
    )
    .optional()
}

fn agent_draft_from_row(row: &rusqlite::Row<'_>) -> SqlResult<AgentDraft> {
    let design_output: String = row.get(4)?;
    let artifact_bundle: Vec<u8> = row.get(5)?;
    let model_manifest: Vec<u8> = row.get(6)?;
    let draft_feedback: Option<String> = row.get(7)?;
    Ok(AgentDraft {
        preview_id: row.get(0)?,
        session_id: row.get(1)?,
        thread_id: row.get(2)?,
        base_message_id: row.get(3)?,
        design_output: deserialize_design_output_json(&design_output)?,
        artifact_bundle: decode_payload(&artifact_bundle)?,
        model_manifest: decode_payload(&model_manifest)?,
        draft_feedback: draft_feedback
            .as_deref()
            .map(deserialize_json)
            .transpose()?,
        updated_at: row.get::<_, i64>(8)? as u64,
    })
}

fn hydrate_agent_draft(conn: &Connection, mut draft: AgentDraft) -> SqlResult<AgentDraft> {
    let (artifact, manifest) = load_payload_full(conn, PayloadOwnerKind::Draft, &draft.preview_id)?;
    draft.artifact_bundle = artifact.ok_or_else(|| {
        rusqlite::Error::InvalidParameterName(format!(
            "Draft {} has no binary artifact core.",
            draft.preview_id
        ))
    })?;
    draft.model_manifest = manifest.ok_or_else(|| {
        rusqlite::Error::InvalidParameterName(format!(
            "Draft {} has no binary manifest core.",
            draft.preview_id
        ))
    })?;
    Ok(draft)
}

pub fn get_agent_draft_for_session(
    conn: &Connection,
    session_id: &str,
) -> SqlResult<Option<AgentDraft>> {
    conn.query_row(
        "SELECT preview_id, session_id, thread_id, base_message_id, design_output, artifact_bundle, model_manifest, draft_feedback, updated_at
         FROM agent_drafts
         WHERE session_id = ?1
         ORDER BY updated_at DESC, preview_id DESC
         LIMIT 1",
        params![session_id],
        agent_draft_from_row,
    )
    .optional()?
    .map(|draft| hydrate_agent_draft(conn, draft))
    .transpose()
}

pub fn get_agent_draft_for_session_thread(
    conn: &Connection,
    session_id: &str,
    thread_id: &str,
) -> SqlResult<Option<AgentDraft>> {
    conn.query_row(
        "SELECT preview_id, session_id, thread_id, base_message_id, design_output, artifact_bundle, model_manifest, draft_feedback, updated_at
         FROM agent_drafts
         WHERE session_id = ?1 AND thread_id = ?2",
        params![session_id, thread_id],
        agent_draft_from_row,
    )
    .optional()?
    .map(|draft| hydrate_agent_draft(conn, draft))
    .transpose()
}

pub fn get_agent_draft_for_session_preview_id(
    conn: &Connection,
    session_id: &str,
    preview_id: &str,
) -> SqlResult<Option<AgentDraft>> {
    conn.query_row(
        "SELECT preview_id, session_id, thread_id, base_message_id, design_output, artifact_bundle, model_manifest, draft_feedback, updated_at
         FROM agent_drafts
         WHERE session_id = ?1 AND preview_id = ?2",
        params![session_id, preview_id],
        agent_draft_from_row,
    )
    .optional()?
    .map(|draft| hydrate_agent_draft(conn, draft))
    .transpose()
}

pub fn get_unambiguous_agent_draft_for_session(
    conn: &Connection,
    session_id: &str,
) -> SqlResult<Option<AgentDraft>> {
    let mut statement = conn.prepare(
        "SELECT preview_id, session_id, thread_id, base_message_id, design_output, artifact_bundle, model_manifest, draft_feedback, updated_at
         FROM agent_drafts
         WHERE session_id = ?1
         LIMIT 2",
    )?;
    let mut rows = statement.query(params![session_id])?;
    let Some(first) = rows.next()? else {
        return Ok(None);
    };
    let first = agent_draft_from_row(first)?;
    if rows.next()?.is_some() {
        return Ok(None);
    }
    drop(rows);
    drop(statement);
    hydrate_agent_draft(conn, first).map(Some)
}

pub fn get_agent_draft_for_session_message(
    conn: &Connection,
    session_id: &str,
    message_id: &str,
) -> SqlResult<Option<AgentDraft>> {
    let mut statement = conn.prepare(
        "SELECT preview_id, session_id, thread_id, base_message_id, design_output, artifact_bundle, model_manifest, draft_feedback, updated_at
         FROM agent_drafts
         WHERE session_id = ?1
           AND (preview_id = ?2 OR base_message_id = ?2)
         LIMIT 2",
    )?;
    let mut rows = statement.query(params![session_id, message_id])?;
    let Some(first) = rows.next()? else {
        return Ok(None);
    };
    let first = agent_draft_from_row(first)?;
    if rows.next()?.is_some() {
        return Ok(None);
    }
    drop(rows);
    drop(statement);
    hydrate_agent_draft(conn, first).map(Some)
}

pub fn get_agent_draft_by_preview_id(
    conn: &Connection,
    preview_id: &str,
) -> SqlResult<Option<AgentDraft>> {
    conn.query_row(
        "SELECT preview_id, session_id, thread_id, base_message_id, design_output, artifact_bundle, model_manifest, draft_feedback, updated_at
         FROM agent_drafts
         WHERE preview_id = ?1",
        params![preview_id],
        agent_draft_from_row,
    )
    .optional()?
    .map(|draft| hydrate_agent_draft(conn, draft))
    .transpose()
}

pub fn get_agent_draft_projection_by_preview_id(
    conn: &Connection,
    preview_id: &str,
) -> SqlResult<Option<crate::contracts::AgentDraftProjection>> {
    let Some(projection) = ensure_payload_projection(conn, PayloadOwnerKind::Draft, preview_id)?
    else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT
           preview_id,
           session_id,
           thread_id,
           base_message_id,
           design_output,
           artifact_bundle,
           model_manifest,
           draft_feedback,
           updated_at
         FROM agent_drafts
         WHERE preview_id = ?1",
        params![preview_id],
        |row| {
            let preview_id: String = row.get(0)?;
            let thread_id: String = row.get(2)?;
            let design_output: String = row.get(4)?;
            let artifact_bundle: Vec<u8> = row.get(5)?;
            let model_manifest: Vec<u8> = row.get(6)?;
            let draft_feedback: Option<String> = row.get(7)?;
            let edge_count = projection.edge_count;
            let face_count = projection.face_count;
            let selection_target_count = projection.selection_count;
            let has_dense_topology = edge_count + face_count + selection_target_count > 0;
            Ok(crate::contracts::AgentDraftProjection {
                dense_topology_ref: has_dense_topology
                    .then(|| format!("draft-topology:{thread_id}:{preview_id}")),
                preview_id,
                session_id: row.get(1)?,
                thread_id,
                base_message_id: row.get(3)?,
                design_output: deserialize_design_output_json(&design_output)?,
                artifact_bundle: decode_payload(&artifact_bundle)?,
                model_manifest: decode_payload(&model_manifest)?,
                draft_feedback: draft_feedback
                    .as_deref()
                    .map(deserialize_json)
                    .transpose()?,
                updated_at: row.get::<_, i64>(8)? as u64,
                edge_count,
                face_count,
                selection_target_count,
                observed_bytes: 0,
                truncated_fields: Vec::new(),
            })
        },
    )
    .optional()
}

pub fn get_agent_draft_thread_id(conn: &Connection, preview_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT thread_id FROM agent_drafts WHERE preview_id = ?1",
        params![preview_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_agent_draft_topology_json_page(
    conn: &Connection,
    preview_id: &str,
    json_column: &str,
    json_path: &str,
    offset: usize,
    limit: usize,
) -> SqlResult<(Vec<String>, usize)> {
    match json_column {
        "artifact_bundle" | "model_manifest" => {}
        _ => {
            return Err(rusqlite::Error::InvalidParameterName(
                "Invalid draft topology column".into(),
            ))
        }
    }
    let field = match json_path {
        "$.edgeTargets" => DenseField::Edge,
        "$.faceTargets" => DenseField::Face,
        "$.selectionTargets" => DenseField::Selection,
        _ => {
            return Err(rusqlite::Error::InvalidParameterName(
                "Invalid draft topology path".into(),
            ))
        }
    };
    let projection =
        ensure_payload_projection(conn, PayloadOwnerKind::Draft, preview_id)?.unwrap_or_default();
    let total = match field {
        DenseField::Edge => projection.edge_count,
        DenseField::Face => projection.face_count,
        DenseField::Selection => projection.selection_count,
    };
    let items = match field {
        DenseField::Edge => load_topology_json_page::<crate::contracts::ViewerEdgeTarget>(
            conn,
            PayloadOwnerKind::Draft,
            preview_id,
            "edge",
            offset,
            limit,
        )?,
        DenseField::Face => load_topology_json_page::<crate::contracts::ViewerFaceTarget>(
            conn,
            PayloadOwnerKind::Draft,
            preview_id,
            "face",
            offset,
            limit,
        )?,
        DenseField::Selection => load_topology_json_page::<crate::contracts::SelectionTarget>(
            conn,
            PayloadOwnerKind::Draft,
            preview_id,
            "selection",
            offset,
            limit,
        )?,
    };
    Ok((items, total))
}

pub fn delete_agent_draft_for_session(conn: &Connection, session_id: &str) -> SqlResult<()> {
    conn.execute(
        "DELETE FROM agent_drafts WHERE session_id = ?1",
        params![session_id],
    )?;
    Ok(())
}

pub fn delete_agent_draft_for_session_thread(
    conn: &Connection,
    session_id: &str,
    thread_id: &str,
) -> SqlResult<()> {
    conn.execute(
        "DELETE FROM agent_drafts WHERE session_id = ?1 AND thread_id = ?2",
        params![session_id, thread_id],
    )?;
    Ok(())
}

pub fn update_message_image_data(
    conn: &Connection,
    message_id: &str,
    image_data: &str,
) -> SqlResult<bool> {
    let changed = conn.execute(
        "UPDATE messages SET image_data = ?1 WHERE id = ?2",
        params![image_data, message_id],
    )?;
    Ok(changed > 0)
}

pub fn update_message_output(
    conn: &Connection,
    message_id: &str,
    output: &DesignOutput,
) -> SqlResult<()> {
    let serialized = serde_json::to_string(output)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "UPDATE messages SET output = ?1 WHERE id = ?2",
        params![serialized, message_id],
    )?;
    Ok(())
}

pub fn upsert_agent_session(
    conn: &Connection,
    session: &crate::contracts::AgentSession,
) -> SqlResult<()> {
    upsert_agent_session_with_ownership(conn, session, session.client_kind == "managed-mcp-http")
}

pub fn upsert_agent_session_with_ownership(
    conn: &Connection,
    session: &crate::contracts::AgentSession,
    managed_runtime: bool,
) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO agent_sessions (session_id, client_kind, host_label, agent_label, llm_model_id, llm_model_label, thread_id, message_id, model_id, phase, status_text, updated_at, managed_runtime)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(session_id) DO UPDATE SET
            client_kind = excluded.client_kind,
            host_label = excluded.host_label,
            agent_label = excluded.agent_label,
            llm_model_id = excluded.llm_model_id,
            llm_model_label = excluded.llm_model_label,
            thread_id = excluded.thread_id,
            message_id = excluded.message_id,
            model_id = excluded.model_id,
            phase = excluded.phase,
            status_text = excluded.status_text,
            updated_at = excluded.updated_at,
            managed_runtime = excluded.managed_runtime",
        params![
            session.session_id,
            session.client_kind,
            session.host_label,
            session.agent_label,
            session.llm_model_id,
            session.llm_model_label,
            session.thread_id,
            session.message_id,
            session.model_id,
            session.phase,
            session.status_text,
            session.updated_at as i64,
            i64::from(managed_runtime)
        ],
    )?;
    Ok(())
}

pub fn delete_agent_session(conn: &Connection, session_id: &str) -> SqlResult<()> {
    conn.execute(
        "DELETE FROM agent_sessions WHERE session_id = ?1",
        [session_id],
    )?;
    Ok(())
}

pub fn delete_all_agent_sessions(conn: &Connection) -> SqlResult<()> {
    conn.execute("DELETE FROM agent_sessions", [])?;
    Ok(())
}

pub fn get_active_agent_sessions(
    conn: &Connection,
    stale_threshold_secs: u64,
) -> SqlResult<Vec<crate::contracts::AgentSession>> {
    let now = unix_now_i64();
    let threshold = now - (stale_threshold_secs as i64);

    let mut stmt = conn.prepare(
        "SELECT session_id, client_kind, host_label, agent_label, llm_model_id, llm_model_label, thread_id, message_id, model_id, phase, status_text, updated_at
         FROM agent_sessions
         WHERE updated_at >= ?1
           AND phase NOT IN ('error', 'disconnected')
         ORDER BY updated_at DESC"
    )?;
    let iter = stmt.query_map([threshold], |row| {
        Ok(crate::contracts::AgentSession {
            session_id: row.get(0)?,
            client_kind: row.get(1)?,
            host_label: row.get(2)?,
            agent_label: row.get(3)?,
            llm_model_id: row.get(4)?,
            llm_model_label: row.get(5)?,
            thread_id: row.get(6)?,
            message_id: row.get(7)?,
            model_id: row.get(8)?,
            phase: row.get(9)?,
            status_text: row.get(10)?,
            updated_at: row.get::<_, i64>(11)? as u64,
        })
    })?;

    let mut results = Vec::new();
    for item in iter {
        results.push(item?);
    }
    Ok(results)
}

/// Fetch DB records for a specific set of session IDs (used for live-session push events).
pub fn get_sessions_by_ids(
    conn: &Connection,
    ids: &[String],
) -> SqlResult<Vec<crate::contracts::AgentSession>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT session_id, client_kind, host_label, agent_label, llm_model_id, llm_model_label, thread_id, message_id, model_id, phase, status_text, updated_at
         FROM agent_sessions
         WHERE session_id IN ({})
         ORDER BY updated_at DESC",
        placeholders
    );
    let mut stmt = conn.prepare(&sql)?;
    let iter = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |row| {
        Ok(crate::contracts::AgentSession {
            session_id: row.get(0)?,
            client_kind: row.get(1)?,
            host_label: row.get(2)?,
            agent_label: row.get(3)?,
            llm_model_id: row.get(4)?,
            llm_model_label: row.get(5)?,
            thread_id: row.get(6)?,
            message_id: row.get(7)?,
            model_id: row.get(8)?,
            phase: row.get(9)?,
            status_text: row.get(10)?,
            updated_at: row.get::<_, i64>(11)? as u64,
        })
    })?;
    let mut results = Vec::new();
    for item in iter {
        results.push(item?);
    }
    Ok(results)
}

pub fn get_thread_last_agent_session(
    conn: &Connection,
    thread_id: &str,
) -> SqlResult<Option<crate::contracts::AgentSession>> {
    conn.query_row(
        "SELECT session_id, client_kind, host_label, agent_label, llm_model_id, llm_model_label, thread_id, message_id, model_id, phase, status_text, updated_at
         FROM agent_sessions
         WHERE thread_id = ?1
         ORDER BY updated_at DESC
         LIMIT 1",
        [thread_id],
        |row| {
            Ok(crate::contracts::AgentSession {
                session_id: row.get(0)?,
                client_kind: row.get(1)?,
                host_label: row.get(2)?,
                agent_label: row.get(3)?,
                llm_model_id: row.get(4)?,
                llm_model_label: row.get(5)?,
                thread_id: row.get(6)?,
                message_id: row.get(7)?,
                model_id: row.get(8)?,
                phase: row.get(9)?,
                status_text: row.get(10)?,
                updated_at: row.get::<_, i64>(11)? as u64,
            })
        },
    )
    .optional()
}

pub fn get_thread_last_agent_session_for_agent(
    conn: &Connection,
    agent_label: &str,
) -> SqlResult<Option<crate::contracts::AgentSession>> {
    conn.query_row(
        "SELECT session_id, client_kind, host_label, agent_label, llm_model_id, llm_model_label, thread_id, message_id, model_id, phase, status_text, updated_at
         FROM agent_sessions
         WHERE agent_label = ?1
         ORDER BY updated_at DESC
         LIMIT 1",
        [agent_label],
        |row| {
            Ok(crate::contracts::AgentSession {
                session_id: row.get(0)?,
                client_kind: row.get(1)?,
                host_label: row.get(2)?,
                agent_label: row.get(3)?,
                llm_model_id: row.get(4)?,
                llm_model_label: row.get(5)?,
                thread_id: row.get(6)?,
                message_id: row.get(7)?,
                model_id: row.get(8)?,
                phase: row.get(9)?,
                status_text: row.get(10)?,
                updated_at: row.get::<_, i64>(11)? as u64,
            })
        },
    )
    .optional()
}

pub fn get_managed_agent_session_ids_not_in(
    conn: &Connection,
    live_session_ids: &[String],
) -> SqlResult<Vec<String>> {
    if live_session_ids.is_empty() {
        let mut stmt = conn.prepare(
            "SELECT session_id
             FROM agent_sessions
             WHERE managed_runtime != 0",
        )?;
        let iter = stmt.query_map([], |row| row.get(0))?;
        let mut session_ids = Vec::new();
        for item in iter {
            session_ids.push(item?);
        }
        return Ok(session_ids);
    }

    let placeholders = live_session_ids
        .iter()
        .enumerate()
        .map(|(index, _)| format!("?{}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT session_id
         FROM agent_sessions
         WHERE managed_runtime != 0
           AND session_id NOT IN ({})",
        placeholders
    );
    let mut stmt = conn.prepare(&sql)?;
    let iter = stmt.query_map(rusqlite::params_from_iter(live_session_ids.iter()), |row| {
        row.get(0)
    })?;
    let mut session_ids = Vec::new();
    for item in iter {
        session_ids.push(item?);
    }
    Ok(session_ids)
}

pub fn delete_expired_target_leases(conn: &Connection) -> SqlResult<usize> {
    conn.execute(
        "DELETE FROM target_leases WHERE expires_at < ?1",
        [unix_now_i64()],
    )
}

pub fn get_active_target_lease(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
    model_id: Option<&str>,
) -> SqlResult<Option<TargetLeaseInfo>> {
    let _ = delete_expired_target_leases(conn)?;
    conn.query_row(
        "SELECT session_id, thread_id, message_id, model_id, host_label, agent_label, acquired_at, expires_at
         FROM target_leases
         WHERE thread_id = ?1
           AND message_id = ?2
           AND COALESCE(model_id, '') = COALESCE(?3, '')
         ORDER BY expires_at DESC
         LIMIT 1",
        params![thread_id, message_id, model_id],
        |row| {
            Ok(TargetLeaseInfo {
                session_id: row.get(0)?,
                thread_id: row.get(1)?,
                message_id: row.get(2)?,
                model_id: row.get(3)?,
                host_label: row.get(4)?,
                agent_label: row.get(5)?,
                acquired_at: row.get::<_, i64>(6)? as u64,
                expires_at: row.get::<_, i64>(7)? as u64,
            })
        },
    )
    .optional()
}

pub fn upsert_target_lease(conn: &Connection, lease: &TargetLeaseInfo) -> SqlResult<()> {
    conn.execute(
        "INSERT INTO target_leases (lease_id, session_id, thread_id, message_id, model_id, acquired_at, expires_at, host_label, agent_label)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(lease_id) DO UPDATE SET
            session_id = excluded.session_id,
            thread_id = excluded.thread_id,
            message_id = excluded.message_id,
            model_id = excluded.model_id,
            acquired_at = excluded.acquired_at,
            expires_at = excluded.expires_at,
            host_label = excluded.host_label,
            agent_label = excluded.agent_label",
        params![
            format!(
                "{}:{}:{}",
                lease.session_id,
                lease.message_id,
                lease.model_id.clone().unwrap_or_default()
            ),
            lease.session_id,
            lease.thread_id,
            lease.message_id,
            lease.model_id,
            lease.acquired_at as i64,
            lease.expires_at as i64,
            lease.host_label,
            lease.agent_label
        ],
    )?;
    Ok(())
}

pub fn delete_target_lease(
    conn: &Connection,
    session_id: &str,
    thread_id: &str,
    message_id: &str,
    model_id: Option<&str>,
) -> SqlResult<()> {
    conn.execute(
        "DELETE FROM target_leases
         WHERE session_id = ?1
           AND thread_id = ?2
           AND message_id = ?3
           AND COALESCE(model_id, '') = COALESCE(?4, '')",
        params![session_id, thread_id, message_id, model_id],
    )?;
    Ok(())
}

pub fn delete_target_leases_for_session(conn: &Connection, session_id: &str) -> SqlResult<()> {
    conn.execute(
        "DELETE FROM target_leases WHERE session_id = ?1",
        [session_id],
    )?;
    Ok(())
}

pub fn get_message_output_and_thread(
    conn: &Connection,
    message_id: &str,
) -> SqlResult<Option<(DesignOutput, String)>> {
    let row: Option<(Option<String>, String)> = conn
        .query_row(
            "SELECT m.output, m.thread_id
             FROM messages m
             JOIN threads t ON t.id = m.thread_id
             WHERE m.id = ?1
               AND m.deleted_at IS NULL
               AND m.status != 'discarded'
               AND t.deleted_at IS NULL",
            [message_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    let Some((output_str, thread_id)) = row else {
        return Ok(None);
    };

    let Some(json_str) = output_str else {
        return Ok(None);
    };

    let Ok(output) = serde_json::from_str::<DesignOutput>(&json_str).map(normalize_design_output)
    else {
        return Ok(None);
    };

    Ok(Some((output, thread_id)))
}

pub fn get_message_thread_id(conn: &Connection, message_id: &str) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT thread_id FROM messages WHERE id = ?1",
        [message_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_visible_message_thread_id(
    conn: &Connection,
    message_id: &str,
) -> SqlResult<Option<String>> {
    conn.query_row(
        "SELECT m.thread_id
         FROM messages m
         JOIN threads t ON t.id = m.thread_id
         WHERE m.id = ?1
           AND m.deleted_at IS NULL
           AND m.status != 'discarded'
           AND t.deleted_at IS NULL",
        [message_id],
        |row| row.get(0),
    )
    .optional()
}

pub type MessageRuntimeAndThread = (Option<ArtifactBundle>, Option<ModelManifest>, String);

pub fn get_message_runtime_and_thread(
    conn: &Connection,
    message_id: &str,
) -> SqlResult<Option<MessageRuntimeAndThread>> {
    let row: Option<(String, Option<String>, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT m.thread_id, m.output,
                    m.version_input_digest, m.runtime_cache_key
             FROM messages m
             JOIN threads t ON t.id = m.thread_id
             WHERE m.id = ?1
               AND m.deleted_at IS NULL
               AND m.status != 'discarded'
               AND t.deleted_at IS NULL",
            [message_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    let Some((thread_id, output_str, stored_version_input_digest, stored_runtime_cache_key)) = row
    else {
        return Ok(None);
    };

    let (artifact_bundle, model_manifest) =
        load_payload_full(conn, PayloadOwnerKind::Message, message_id)?;
    let output = output_str.and_then(|json_str| {
        serde_json::from_str::<DesignOutput>(&json_str)
            .ok()
            .map(normalize_design_output)
    });
    if artifact_bundle.is_some() {
        let (expected_version_input_digest, expected_runtime_cache_key) =
            version_runtime_binding(message_id, output.as_ref(), artifact_bundle.as_ref())?;
        if stored_version_input_digest != expected_version_input_digest
            || stored_runtime_cache_key != expected_runtime_cache_key
        {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "Binary runtime binding mismatch for message {message_id}."
            )));
        }
    }

    Ok(Some((artifact_bundle, model_manifest, thread_id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        ComponentDependencyLock, ComponentDependencyLockComponent, ComponentDependencyLockEntry,
        ComponentPayloadKind, DesignParams, InteractionMode, MessageRole, MessageStatus,
        ParamValue, UiField, UiSpec,
    };
    use std::fs;
    use std::path::PathBuf;

    fn init_db_internal(conn: &Connection) -> SqlResult<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS threads (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                summary TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL,
                genie_traits TEXT,
                deleted_at INTEGER
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'success',
                output TEXT,
                usage TEXT,
                artifact_bundle TEXT,
                model_manifest TEXT,
                structural_verification TEXT,
                agent_origin TEXT,
                timestamp INTEGER NOT NULL,
                image_data TEXT,
                visual_kind TEXT,
                attachment_images TEXT,
                version_input_digest TEXT,
                runtime_cache_key TEXT,
                deleted_at INTEGER,
                trash_hidden_at INTEGER,
                FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_thread_visible_timestamp
             ON messages(thread_id, timestamp DESC)
             WHERE deleted_at IS NULL",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_thread_target_candidates
             ON messages(thread_id, role, status, timestamp DESC)
             WHERE deleted_at IS NULL",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_sessions (
                session_id TEXT PRIMARY KEY,
                client_kind TEXT NOT NULL,
                host_label TEXT NOT NULL DEFAULT '',
                agent_label TEXT NOT NULL,
                llm_model_id TEXT,
                llm_model_label TEXT,
                thread_id TEXT,
                message_id TEXT,
                model_id TEXT,
                phase TEXT NOT NULL,
                status_text TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL,
                managed_runtime INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_drafts (
                preview_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                thread_id TEXT NOT NULL,
                base_message_id TEXT,
                design_output TEXT NOT NULL,
                artifact_bundle TEXT NOT NULL,
                model_manifest TEXT NOT NULL,
                draft_feedback TEXT,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        conn.execute("DROP INDEX IF EXISTS idx_agent_drafts_session", [])?;
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_drafts_session_thread
             ON agent_drafts(session_id, thread_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_agent_drafts_thread_updated
             ON agent_drafts(thread_id, updated_at DESC)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS verification_records (
                snapshot_id TEXT PRIMARY KEY,
                preview_id TEXT NOT NULL,
                artifact_digest TEXT NOT NULL,
                verification_record TEXT NOT NULL,
                verified_at INTEGER NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_verification_records_preview
             ON verification_records(preview_id, verified_at DESC)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS thread_references (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                source_message_id TEXT,
                ordinal INTEGER NOT NULL DEFAULT 0,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                summary TEXT NOT NULL DEFAULT '',
                pinned INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS thread_window_layouts (
                thread_id TEXT PRIMARY KEY,
                layout_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY(thread_id) REFERENCES threads(id) ON DELETE CASCADE
            )",
            [],
        )?;
        // Migrations: keep in sync with init_db
        let _ = conn.execute(
            "ALTER TABLE threads ADD COLUMN status TEXT NOT NULL DEFAULT 'active'",
            [],
        );
        let _ = conn.execute("ALTER TABLE threads ADD COLUMN created_at INTEGER", []);
        let _ = conn.execute(
            "UPDATE threads SET created_at = updated_at WHERE created_at IS NULL",
            [],
        );
        let _ = conn.execute("ALTER TABLE threads ADD COLUMN finalized_at INTEGER", []);
        let _ = conn.execute("ALTER TABLE threads ADD COLUMN pending_confirm TEXT", []);
        normalize_thread_lifecycle_rows(&conn)?;
        crate::services::codex_takeover::ensure_schema(conn)?;
        Ok(())
    }

    fn sample_output() -> DesignOutput {
        DesignOutput {
            title: "Test".to_string(),
            version_name: "V1".to_string(),
            response: "".to_string(),
            interaction_mode: InteractionMode::Design,
            macro_code: "print('hi')".to_string(),
            macro_dialect: crate::contracts::MacroDialect::Legacy,
            engine_kind: crate::contracts::EngineKind::Freecad,
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            geometry_backend: crate::contracts::GeometryBackend::Freecad,
            ui_spec: UiSpec { fields: Vec::new() },
            initial_params: DesignParams::from([("x".to_string(), ParamValue::Number(10.0))]),
            post_processing: None,
        }
    }

    fn sample_artifact_bundle(model_id: &str) -> ArtifactBundle {
        ArtifactBundle {
            geometry_provenance: None,
            component_dependency_lock: None,
            component_dependency_lock_digest: None,
            component_import_origins: Vec::new(),
            component_placement_evidence: Vec::new(),
            schema_version: 1,
            model_id: model_id.to_string(),
            source_kind: crate::contracts::ModelSourceKind::Generated,
            engine_kind: crate::contracts::EngineKind::Freecad,
            source_language: crate::contracts::SourceLanguage::LegacyPython,
            geometry_backend: crate::contracts::GeometryBackend::Freecad,
            content_hash: format!("hash-{model_id}"),
            artifact_version: 1,
            fcstd_path: format!("/tmp/{model_id}.FCStd"),
            manifest_path: format!("/tmp/{model_id}.json"),
            macro_path: None,
            model_stl_path: format!("/tmp/{model_id}.stl"),
            viewer_assets: Vec::new(),
            edge_targets: Vec::new(),
            face_targets: Vec::new(),
            callout_anchors: Vec::new(),
            measurement_guides: Vec::new(),
            export_artifacts: Vec::new(),
        }
    }

    fn sample_manifest(model_id: &str) -> ModelManifest {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "modelId": model_id,
            "sourceKind": "generated",
            "engineKind": "freecad",
            "sourceLanguage": "legacyPython",
            "geometryBackend": "freecad",
            "document": { "documentName": "test", "documentLabel": "Test" }
        }))
        .expect("minimal manifest")
    }

    #[test]
    fn new_message_payload_rejects_artifact_manifest_model_mismatch() {
        let mut message = sample_version_message("mismatch", 1, sample_artifact_bundle("artifact"));
        message.model_manifest = Some(sample_manifest("manifest"));
        assert!(validate_message_payload(&message).is_err());
    }

    #[test]
    fn new_message_payload_rejects_runtime_without_output_or_manifest() {
        let mut message = sample_version_message("runtime", 1, sample_artifact_bundle("model"));
        message.output = None;
        assert!(validate_message_payload(&message).is_err());

        message.output = Some(sample_output());
        message.model_manifest = None;
        assert!(validate_message_payload(&message).is_err());
    }

    #[test]
    fn new_message_payload_rejects_user_version_payload() {
        let mut message =
            sample_version_message("user-version", 1, sample_artifact_bundle("model"));
        message.role = MessageRole::User;
        assert!(validate_message_payload(&message).is_err());
    }

    #[test]
    fn legacy_error_row_remains_valid_without_version_payload() {
        let message = Message {
            id: "legacy-error".to_string(),
            role: MessageRole::Assistant,
            content: "provider failed".to_string(),
            status: MessageStatus::Error,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 1,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        assert!(validate_message_payload(&message).is_ok());
    }

    #[test]
    fn atomic_payload_adapter_accepts_conversation_and_complete_version() {
        let conversation = Message {
            id: "conversation".to_string(),
            role: MessageRole::User,
            content: "hello".to_string(),
            status: MessageStatus::Success,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 1,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        assert!(matches!(
            message_payload_from_legacy(&conversation),
            Ok(crate::contracts::MessagePayload::Conversation)
        ));

        let mut version = sample_version_message("complete", 1, sample_artifact_bundle("model"));
        version.model_manifest = Some(sample_manifest("model"));
        assert!(matches!(
            message_payload_from_legacy(&version),
            Ok(crate::contracts::MessagePayload::Version { .. })
        ));
    }

    #[test]
    fn atomic_payload_adapter_rejects_incomplete_version() {
        let message = sample_version_message("incomplete", 1, sample_artifact_bundle("model"));
        assert!(message_payload_from_legacy(&message).is_err());
    }

    fn sample_version_message(id: &str, timestamp: u64, bundle: ArtifactBundle) -> Message {
        Message {
            id: id.to_string(),
            role: MessageRole::Assistant,
            content: String::new(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(bundle),
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        }
    }

    #[test]
    fn committing_new_thread_version_keeps_latest_stl_and_prunes_previous_stl() {
        let conn = Connection::open_in_memory().expect("memory db");
        init_db_internal(&conn).expect("schema");
        create_or_update_thread(&conn, "stl-thread", "STL", 1, None).expect("thread");
        let root = std::env::temp_dir()
            .join(format!("ecky-db-stl-{}", uuid::Uuid::new_v4()))
            .join("model-runtime");
        fs::create_dir_all(&root).expect("runtime root");
        let old_path = root.join("old").join("model.stl");
        let latest_path = root.join("latest").join("model.stl");
        fs::create_dir_all(old_path.parent().unwrap()).expect("old bundle");
        fs::create_dir_all(latest_path.parent().unwrap()).expect("latest bundle");
        fs::write(&old_path, b"old").expect("old stl");
        fs::write(&latest_path, b"latest").expect("latest stl");

        let mut old_bundle = sample_artifact_bundle("old");
        old_bundle.model_stl_path = old_path.to_string_lossy().to_string();
        add_message(
            &conn,
            "stl-thread",
            &sample_version_message("old-version", 1, old_bundle),
        )
        .expect("old version");
        let mut latest_bundle = sample_artifact_bundle("latest");
        latest_bundle.model_stl_path = latest_path.to_string_lossy().to_string();
        add_message(
            &conn,
            "stl-thread",
            &sample_version_message("latest-version", 2, latest_bundle),
        )
        .expect("latest version");

        assert!(!old_path.exists(), "previous STL must become ephemeral");
        assert!(latest_path.exists(), "latest STL must stay durable");
        fs::remove_dir_all(root.parent().unwrap()).expect("cleanup");
    }

    fn bundle_with_package_lock(
        model_id: &str,
        version: &str,
        package_digest: &str,
    ) -> ArtifactBundle {
        let mut bundle = sample_artifact_bundle(model_id);
        let lock = ComponentDependencyLock {
            schema_version: crate::contracts::COMPONENT_DEPENDENCY_LOCK_SCHEMA_VERSION,
            dependencies: vec![ComponentDependencyLockEntry {
                package_id: "fixture.live".to_string(),
                version: version.to_string(),
                package_digest: package_digest.to_string(),
                components: vec![ComponentDependencyLockComponent {
                    component_id: "cage".to_string(),
                    entry_symbol: Some("cage".to_string()),
                    payload_digest: package_digest.to_string(),
                    payload_kind: Some(ComponentPayloadKind::Source),
                    geometry_representation: None,
                }],
            }],
        }
        .canonical();
        bundle.component_dependency_lock_digest = Some(
            crate::services::render_snapshot::component_dependency_lock_digest(&lock)
                .expect("lock digest"),
        );
        bundle.component_dependency_lock = Some(lock);
        bundle
    }

    #[test]
    fn committed_upgrade_versions_keep_distinct_locks_and_both_root_gc() {
        let conn = Connection::open_in_memory().expect("memory db");
        init_db_internal(&conn).expect("schema");
        create_or_update_thread(&conn, "upgrade-thread", "Upgrade", 1, None).expect("thread");

        let first_digest = format!("sha256:{}", "a".repeat(64));
        let second_digest = format!("sha256:{}", "b".repeat(64));
        for (id, timestamp, bundle) in [
            (
                "version-1",
                1,
                bundle_with_package_lock("generated-v1", "1.0.0", &first_digest),
            ),
            (
                "version-2",
                2,
                bundle_with_package_lock("generated-v2", "2.0.0", &second_digest),
            ),
        ] {
            add_message(
                &conn,
                "upgrade-thread",
                &Message {
                    id: id.to_string(),
                    role: MessageRole::Assistant,
                    content: String::new(),
                    status: MessageStatus::Success,
                    output: Some(sample_output()),
                    usage: None,
                    artifact_bundle: Some(bundle),
                    model_manifest: None,
                    structural_verification: None,
                    agent_origin: None,
                    timestamp,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                },
            )
            .expect("commit version");
        }

        let first = get_thread_message_version(&conn, "upgrade-thread", "version-1")
            .expect("read v1")
            .expect("v1")
            .artifact_bundle
            .expect("v1 bundle");
        let second = get_thread_message_version(&conn, "upgrade-thread", "version-2")
            .expect("read v2")
            .expect("v2")
            .artifact_bundle
            .expect("v2 bundle");
        assert_eq!(
            first
                .component_dependency_lock
                .expect("v1 lock")
                .dependencies[0]
                .package_digest,
            first_digest
        );
        assert_eq!(
            second
                .component_dependency_lock
                .expect("v2 lock")
                .dependencies[0]
                .package_digest,
            second_digest
        );
        assert_eq!(
            component_dependency_package_digests(&conn).expect("gc roots"),
            [first_digest, second_digest].into_iter().collect()
        );
    }

    #[test]
    fn test_update_ui_spec_and_params() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "test-thread";
        let msg_id = "test-msg";
        let now = 123456789;

        create_or_update_thread(&conn, thread_id, "Test Thread", now, None).unwrap();

        let msg = Message {
            id: msg_id.to_string(),
            role: MessageRole::Assistant,
            content: "Hello".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: now,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &msg).unwrap();

        // Update UI Spec
        let new_spec = UiSpec {
            fields: vec![UiField::Number {
                key: "y".to_string(),
                label: "Y".to_string(),
                min: None,
                max: None,
                step: None,
                min_from: None,
                max_from: None,
                frozen: false,
            }],
        };
        update_message_ui_spec(&conn, msg_id, &new_spec).unwrap();

        // Update Params
        let new_params = DesignParams::from([
            ("x".to_string(), ParamValue::Number(20.0)),
            ("y".to_string(), ParamValue::Number(5.0)),
        ]);
        update_message_parameters(&conn, msg_id, &new_params).unwrap();

        // Verify
        let (output, tid) = get_message_output_and_thread(&conn, msg_id)
            .unwrap()
            .unwrap();
        assert_eq!(tid, thread_id);
        assert_eq!(output.ui_spec, new_spec);
        assert_eq!(output.initial_params, new_params);
    }

    #[test]
    fn durable_version_runtime_binding_rejects_digest_mismatch() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();
        create_or_update_thread(&conn, "runtime-thread", "Runtime", 1, None).unwrap();
        let message = Message {
            id: "runtime-version".to_string(),
            role: MessageRole::Assistant,
            content: "Rendered".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("generated-runtime")),
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 1,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        add_message(&conn, "runtime-thread", &message).unwrap();

        let (version_digest, cache_key): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT version_input_digest, runtime_cache_key FROM messages WHERE id = ?1",
                [&message.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(version_digest.is_some());
        assert!(cache_key.is_some());

        conn.execute(
            "UPDATE messages SET runtime_cache_key = 'sha256:stale' WHERE id = ?1",
            [&message.id],
        )
        .unwrap();
        let loaded = get_thread_message_version(&conn, "runtime-thread", &message.id)
            .unwrap()
            .unwrap();
        assert!(loaded.artifact_bundle.is_none());
        assert!(loaded.model_manifest.is_none());
    }

    #[test]
    fn legacy_unbound_version_keeps_runtime_until_explicitly_rewritten() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();
        create_or_update_thread(&conn, "legacy-thread", "Legacy", 1, None).unwrap();
        let message = Message {
            id: "legacy-version".to_string(),
            role: MessageRole::Assistant,
            content: "Rendered".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("legacy-runtime")),
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 1,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        add_message(&conn, "legacy-thread", &message).unwrap();
        conn.execute(
            "UPDATE messages SET version_input_digest = NULL, runtime_cache_key = NULL WHERE id = ?1",
            [&message.id],
        )
        .unwrap();

        let loaded = get_thread_message_version(&conn, "legacy-thread", &message.id)
            .unwrap()
            .unwrap();
        assert!(loaded.artifact_bundle.is_some());
    }

    #[test]
    fn test_delete_version_keeps_prompt_visible_and_only_surfaces_deleted_models() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-1";
        create_or_update_thread(&conn, thread_id, "Thread", 100, None).unwrap();

        let user_msg = Message {
            id: "user-1".to_string(),
            role: MessageRole::User,
            content: "Make a box".to_string(),
            status: MessageStatus::Success,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 100,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        let assistant_msg = Message {
            id: "assistant-1".to_string(),
            role: MessageRole::Assistant,
            content: "Box created".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-1")),
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 101,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &user_msg).unwrap();
        add_message(&conn, thread_id, &assistant_msg).unwrap();
        add_thread_reference(
            &conn,
            &ThreadReference {
                id: "ref-1".to_string(),
                thread_id: thread_id.to_string(),
                source_message_id: Some(user_msg.id.clone()),
                ordinal: 0,
                kind: "python_macro".to_string(),
                name: "prompt_macro_1".to_string(),
                content: "box()".to_string(),
                summary: "Prompt macro".to_string(),
                pinned: true,
                created_at: 100,
            },
        )
        .unwrap();

        delete_version_cluster(&conn, &assistant_msg.id).unwrap();

        let visible_messages = get_thread_messages(&conn, thread_id).unwrap();
        assert_eq!(visible_messages.len(), 1);
        assert_eq!(visible_messages[0].id, user_msg.id);
        assert!(has_visible_messages(&conn, thread_id).unwrap());

        let context_messages = get_thread_messages_for_context(&conn, thread_id).unwrap();
        assert!(context_messages.is_empty());

        let deleted = get_deleted_messages(&conn).unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].id, assistant_msg.id);

        let refs = get_thread_references(&conn, thread_id).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(
            refs[0].source_message_id.as_deref(),
            Some(user_msg.id.as_str())
        );
    }

    #[test]
    fn test_delete_and_restore_manual_version_hides_and_restores_empty_thread() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-2";
        create_or_update_thread(&conn, thread_id, "Manual", 200, None).unwrap();

        let assistant_msg = Message {
            id: "assistant-manual".to_string(),
            role: MessageRole::Assistant,
            content: "Manual version".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-manual")),
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 200,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &assistant_msg).unwrap();
        delete_version_cluster(&conn, &assistant_msg.id).unwrap();
        assert!(!has_visible_messages(&conn, thread_id).unwrap());
        assert!(get_all_threads(&conn).unwrap().is_empty());

        restore_version_cluster(&conn, &assistant_msg.id).unwrap();
        assert!(has_visible_messages(&conn, thread_id).unwrap());
        assert_eq!(get_all_threads(&conn).unwrap().len(), 1);
    }

    #[test]
    fn test_restored_version_becomes_latest_version() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-restore-latest";
        create_or_update_thread(&conn, thread_id, "Restore Latest", 100, None).unwrap();

        let older_msg = Message {
            id: "assistant-older".to_string(),
            role: MessageRole::Assistant,
            content: "Older version".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-older")),
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 100,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        let newer_msg = Message {
            id: "assistant-newer".to_string(),
            role: MessageRole::Assistant,
            content: "Newer version".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-newer")),
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 200,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &older_msg).unwrap();
        add_message(&conn, thread_id, &newer_msg).unwrap();
        delete_version_cluster(&conn, &older_msg.id).unwrap();
        assert_eq!(
            get_thread_latest_version(&conn, thread_id)
                .unwrap()
                .unwrap()
                .id,
            "assistant-newer"
        );

        restore_version_cluster(&conn, &older_msg.id).unwrap();
        assert_eq!(
            get_thread_latest_version(&conn, thread_id)
                .unwrap()
                .unwrap()
                .id,
            "assistant-older"
        );
    }

    #[test]
    fn test_hide_deleted_message_removes_it_from_trash_listing() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-trash";
        create_or_update_thread(&conn, thread_id, "Trash", 250, None).unwrap();

        let assistant_msg = Message {
            id: "assistant-trash".to_string(),
            role: MessageRole::Assistant,
            content: "Trash candidate".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-trash")),
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 250,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &assistant_msg).unwrap();
        delete_version_cluster(&conn, &assistant_msg.id).unwrap();
        assert_eq!(get_deleted_messages(&conn).unwrap().len(), 1);

        assert!(hide_deleted_message(&conn, &assistant_msg.id).unwrap());
        assert!(get_deleted_messages(&conn).unwrap().is_empty());

        restore_version_cluster(&conn, &assistant_msg.id).unwrap();
        assert!(get_deleted_messages(&conn).unwrap().is_empty());
    }

    #[test]
    fn thread_preview_returns_only_newest_visible_preview_payload() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-preview";
        create_or_update_thread(&conn, thread_id, "Preview", 100, None).unwrap();
        for (id, timestamp, image_data) in [
            (
                "preview-older",
                100,
                Some("data:image/png;base64,preview".to_string()),
            ),
            ("preview-newer-without-image", 200, None),
        ] {
            add_message(
                &conn,
                thread_id,
                &Message {
                    id: id.to_string(),
                    role: MessageRole::Assistant,
                    content: "Version".to_string(),
                    status: MessageStatus::Success,
                    output: Some(sample_output()),
                    usage: None,
                    artifact_bundle: Some(sample_artifact_bundle(id)),
                    model_manifest: None,
                    structural_verification: None,
                    agent_origin: None,
                    timestamp,
                    image_data,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                },
            )
            .unwrap();
        }

        assert_eq!(get_thread_preview(&conn, thread_id).unwrap(), None);

        assert!(delete_thread(&conn, thread_id).unwrap());
        assert_eq!(get_thread_preview(&conn, thread_id).unwrap(), None);
        assert_eq!(
            get_deleted_thread_preview(&conn, thread_id)
                .unwrap()
                .as_deref(),
            Some("data:image/png;base64,preview")
        );
    }

    #[test]
    fn deleted_thread_page_is_cursor_paginated_and_restore_keeps_identity() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        for (id, title, updated_at, deleted_at) in [
            ("deleted-newer", "Newer project", 300_i64, 500_i64),
            ("deleted-older", "Older project", 200_i64, 400_i64),
        ] {
            create_or_update_thread(&conn, id, title, updated_at as u64, None).unwrap();
            add_message(
                &conn,
                id,
                &Message {
                    id: format!("{id}-version"),
                    role: MessageRole::Assistant,
                    content: title.to_string(),
                    status: MessageStatus::Success,
                    output: Some(sample_output()),
                    usage: None,
                    artifact_bundle: Some(sample_artifact_bundle(id)),
                    model_manifest: None,
                    structural_verification: None,
                    agent_origin: None,
                    timestamp: updated_at as u64,
                    image_data: Some(format!("data:image/png;base64,{id}")),
                    visual_kind: None,
                    attachment_images: Vec::new(),
                },
            )
            .unwrap();
            assert!(delete_thread(&conn, id).unwrap());
            conn.execute(
                "UPDATE threads SET deleted_at = ?1 WHERE id = ?2",
                params![deleted_at, id],
            )
            .unwrap();
        }

        let first = get_deleted_threads_page(&conn, None, 1).unwrap();
        assert_eq!(first.items.len(), 1);
        assert_eq!(first.items[0].id, "deleted-newer");
        assert_eq!(first.items[0].version_count, 1);
        assert!(first.has_more);
        assert!(first.next_before.is_some());

        let second = get_deleted_threads_page(&conn, first.next_before.as_deref(), 1).unwrap();
        assert_eq!(second.items.len(), 1);
        assert_eq!(second.items[0].id, "deleted-older");
        assert!(!second.has_more);

        assert!(restore_deleted_thread(&conn, "deleted-newer").unwrap());
        let restored = get_all_threads(&conn).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].id, "deleted-newer");
        assert_eq!(
            get_thread_messages(&conn, "deleted-newer").unwrap().len(),
            1
        );
    }

    #[test]
    fn test_message_attachment_images_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-images";
        create_or_update_thread(&conn, thread_id, "Images", 300, None).unwrap();

        let msg = Message {
            id: "user-images".to_string(),
            role: MessageRole::User,
            content: "See references".to_string(),
            status: MessageStatus::Success,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 300,
            image_data: Some("data:image/png;base64,viewport".to_string()),
            visual_kind: Some(crate::contracts::MessageVisualKind::ConceptPreview),
            attachment_images: vec![
                "data:image/png;base64,ref-1".to_string(),
                "data:image/png;base64,ref-2".to_string(),
            ],
        };

        add_message(&conn, thread_id, &msg).unwrap();

        let messages = get_thread_messages(&conn, thread_id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].image_data.as_deref(),
            Some("data:image/png;base64,viewport")
        );
        assert_eq!(
            messages[0].visual_kind,
            Some(crate::contracts::MessageVisualKind::ConceptPreview)
        );
        assert_eq!(
            messages[0].attachment_images,
            vec![
                "data:image/png;base64,ref-1".to_string(),
                "data:image/png;base64,ref-2".to_string(),
            ]
        );
    }

    #[test]
    fn test_update_message_image_data_updates_version_preview() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-preview";
        create_or_update_thread(&conn, thread_id, "Preview", 400, None).unwrap();

        let msg = Message {
            id: "assistant-preview".to_string(),
            role: MessageRole::Assistant,
            content: "Rendered".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-preview")),
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 400,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &msg).unwrap();

        let changed =
            update_message_image_data(&conn, &msg.id, "data:image/jpeg;base64,render-preview")
                .unwrap();
        assert!(changed);

        let latest = get_thread_latest_version(&conn, thread_id)
            .unwrap()
            .expect("latest version");
        assert_eq!(
            latest.image_data.as_deref(),
            Some("data:image/jpeg;base64,render-preview")
        );
    }

    #[test]
    fn test_thread_version_count_ignores_output_only_success_messages() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-renderable-count";
        create_or_update_thread(&conn, thread_id, "Renderable", 500, None).unwrap();

        let output_only = Message {
            id: "assistant-output-only".to_string(),
            role: MessageRole::Assistant,
            content: "Draft only".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 500,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        let rendered = Message {
            id: "assistant-rendered".to_string(),
            role: MessageRole::Assistant,
            content: "Rendered".to_string(),
            status: MessageStatus::Success,
            output: Some(sample_output()),
            usage: None,
            artifact_bundle: Some(sample_artifact_bundle("assistant-rendered")),
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 501,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };

        add_message(&conn, thread_id, &output_only).unwrap();
        add_message(&conn, thread_id, &rendered).unwrap();

        let threads = get_all_threads(&conn).unwrap();
        assert_eq!(threads[0].version_count, 1);
        assert_eq!(
            get_thread_latest_version(&conn, thread_id)
                .unwrap()
                .unwrap()
                .id,
            rendered.id
        );
    }

    #[test]
    fn test_get_all_threads_orders_by_latest_visible_message_timestamp() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "older-thread", "Older", 100, None).unwrap();
        create_or_update_thread(&conn, "newer-thread", "Newer", 50, None).unwrap();

        add_message(
            &conn,
            "older-thread",
            &Message {
                id: "older-msg".to_string(),
                role: MessageRole::User,
                content: "older".to_string(),
                status: MessageStatus::Success,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                timestamp: 200,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
            },
        )
        .unwrap();

        add_message(
            &conn,
            "newer-thread",
            &Message {
                id: "newer-msg".to_string(),
                role: MessageRole::User,
                content: "newer".to_string(),
                status: MessageStatus::Success,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                timestamp: 300,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
            },
        )
        .unwrap();

        let threads = get_all_threads(&conn).unwrap();
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].id, "newer-thread");
        assert_eq!(threads[0].updated_at, 300);
        assert_eq!(threads[1].id, "older-thread");
        assert_eq!(threads[1].updated_at, 200);
    }

    #[test]
    fn test_mark_interrupted_pending_messages_promotes_pending_to_error() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "pending-thread", "Pending", 100, None).unwrap();

        add_message(
            &conn,
            "pending-thread",
            &Message {
                id: "pending-assistant".to_string(),
                role: MessageRole::Assistant,
                content: "Generating...".to_string(),
                status: MessageStatus::Pending,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                timestamp: 100,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
            },
        )
        .unwrap();

        let changed = mark_interrupted_pending_messages(&conn).unwrap();
        assert_eq!(changed, 1);

        let messages = get_thread_messages(&conn, "pending-thread").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].status, MessageStatus::Error);
        assert!(messages[0]
            .content
            .contains("Request interrupted by app restart before provider response completed"));
    }

    #[test]
    fn get_thread_messages_preserves_insertion_order_for_equal_timestamps() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "thread-order", "Thread", 100, None).unwrap();

        add_message(
            &conn,
            "thread-order",
            &Message {
                id: "msg-a".to_string(),
                role: MessageRole::User,
                content: "first".to_string(),
                status: MessageStatus::Success,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                timestamp: 100,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
            },
        )
        .unwrap();

        add_message(
            &conn,
            "thread-order",
            &Message {
                id: "msg-b".to_string(),
                role: MessageRole::Assistant,
                content: "second".to_string(),
                status: MessageStatus::Success,
                output: None,
                usage: None,
                artifact_bundle: None,
                model_manifest: None,
                structural_verification: None,
                agent_origin: None,
                timestamp: 100,
                image_data: None,
                visual_kind: None,
                attachment_images: Vec::new(),
            },
        )
        .unwrap();

        let messages = get_thread_messages(&conn, "thread-order").unwrap();
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["msg-a", "msg-b"]
        );
    }

    #[test]
    fn thread_message_reads_hide_agent_tool_errors_from_history() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "thread-agent-errors", "Thread", 100, None).unwrap();
        let visible_user = Message {
            id: "user-visible".to_string(),
            role: MessageRole::User,
            content: "make roof pins".to_string(),
            status: MessageStatus::Success,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 100,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        let agent_error = Message {
            id: "agent-error".to_string(),
            role: MessageRole::Assistant,
            content: "Expected a symbolic head for runtime list expression.".to_string(),
            status: MessageStatus::Error,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: Some(crate::contracts::AgentOrigin {
                host_label: "Codex MCP Client".to_string(),
                client_kind: "mcp-http".to_string(),
                agent_label: "Ecky".to_string(),
                llm_model_id: None,
                llm_model_label: None,
                session_id: "session-1".to_string(),
                created_at: 101,
            }),
            timestamp: 101,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        let generation_error = Message {
            id: "generation-error".to_string(),
            role: MessageRole::Assistant,
            content: "Generation failed.".to_string(),
            status: MessageStatus::Error,
            output: None,
            usage: None,
            artifact_bundle: None,
            model_manifest: None,
            structural_verification: None,
            agent_origin: None,
            timestamp: 102,
            image_data: None,
            visual_kind: None,
            attachment_images: Vec::new(),
        };
        let mut authored_error = agent_error.clone();
        authored_error.id = "authored-error".to_string();
        authored_error.content = "Draft failed validation.".to_string();
        authored_error.output = Some(sample_output());
        authored_error.timestamp = 103;
        add_message(&conn, "thread-agent-errors", &visible_user).unwrap();
        add_message(&conn, "thread-agent-errors", &agent_error).unwrap();
        add_message(&conn, "thread-agent-errors", &generation_error).unwrap();
        add_message(&conn, "thread-agent-errors", &authored_error).unwrap();

        let full = get_thread_messages(&conn, "thread-agent-errors").unwrap();
        assert_eq!(
            full.iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["user-visible", "generation-error", "authored-error"]
        );

        let page = get_thread_messages_page(&conn, "thread-agent-errors", None, 50, true).unwrap();
        assert_eq!(
            page.messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["user-visible", "generation-error", "authored-error"]
        );

        let context = get_thread_messages_for_context(&conn, "thread-agent-errors").unwrap();
        assert_eq!(
            context
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["user-visible", "generation-error", "authored-error"]
        );

        let threads = get_all_threads(&conn).unwrap();
        assert_eq!(threads[0].error_count, 2);
    }

    #[test]
    fn create_or_update_thread_preserves_existing_title_on_conflict() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "thread-keep-title", "Original Thread", 100, None).unwrap();
        create_or_update_thread(&conn, "thread-keep-title", "Version Name Noise", 200, None)
            .unwrap();

        let thread = get_all_threads(&conn)
            .unwrap()
            .into_iter()
            .find(|thread| thread.id == "thread-keep-title")
            .expect("thread exists");
        assert_eq!(thread.title, "Original Thread");
        assert_eq!(thread.updated_at, 200);
    }

    #[test]
    fn create_or_update_thread_inserts_thread_without_authoring_context() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        create_or_update_thread(&conn, "thread-no-context", "No Context", 100, None).unwrap();

        let thread = get_all_threads(&conn)
            .unwrap()
            .into_iter()
            .find(|thread| thread.id == "thread-no-context")
            .expect("thread exists");
        assert_eq!(thread.title, "No Context");
    }

    #[test]
    fn recent_thread_messages_for_summary_returns_visible_tail_in_order() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-summary-tail";
        create_or_update_thread(&conn, thread_id, "Summary Tail", 100, None).unwrap();

        for index in 0..6 {
            add_message(
                &conn,
                thread_id,
                &Message {
                    id: format!("msg-{}", index),
                    role: if index % 2 == 0 {
                        MessageRole::User
                    } else {
                        MessageRole::Assistant
                    },
                    content: format!("message {}", index),
                    status: MessageStatus::Success,
                    output: None,
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    structural_verification: None,
                    agent_origin: None,
                    timestamp: 100 + index as u64,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                },
            )
            .unwrap();
        }

        let tail = get_recent_thread_messages_for_summary(&conn, thread_id, 3).unwrap();
        assert_eq!(
            tail.iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>(),
            vec!["msg-3", "msg-4", "msg-5"]
        );
    }

    #[test]
    fn migrate_threads_drop_authoring_columns_removes_legacy_thread_context() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();
        conn.execute(
            "ALTER TABLE threads ADD COLUMN engine_kind TEXT NOT NULL DEFAULT 'freecad'",
            [],
        )
        .unwrap();
        conn.execute(
            "ALTER TABLE threads ADD COLUMN source_language TEXT NOT NULL DEFAULT 'legacyPython'",
            [],
        )
        .unwrap();
        conn.execute(
            "ALTER TABLE threads ADD COLUMN geometry_backend TEXT NOT NULL DEFAULT 'freecad'",
            [],
        )
        .unwrap();
        create_or_update_thread(&conn, "thread-context", "Context", 100, None).unwrap();
        conn.execute(
            "UPDATE threads SET status = 'finalized', finalized_at = 123, pending_confirm = 'review' WHERE id = 'thread-context'",
            [],
        )
        .unwrap();

        migrate_threads_drop_authoring_columns(&conn).unwrap();

        assert!(!table_has_column(&conn, "threads", "engine_kind").unwrap());
        assert!(!table_has_column(&conn, "threads", "source_language").unwrap());
        assert!(!table_has_column(&conn, "threads", "geometry_backend").unwrap());
        let thread = get_inventory_threads(&conn)
            .unwrap()
            .into_iter()
            .find(|thread| thread.id == "thread-context")
            .expect("thread survives migration");
        assert_eq!(thread.finalized_at, Some(123));
        assert_eq!(thread.pending_confirm.as_deref(), Some("review"));
    }

    #[test]
    fn test_migrate_thread_genie_traits_upgrades_legacy_and_missing_rows() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        conn.execute(
            "INSERT INTO threads (id, title, updated_at, genie_traits) VALUES (?1, ?2, ?3, ?4)",
            params![
                "legacy-thread",
                "Legacy",
                100i64,
                r#"{"seed":77,"colorHue":150.0,"vertexCount":18,"jitterScale":1.1,"pulseScale":0.9}"#
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO threads (id, title, updated_at, genie_traits) VALUES (?1, ?2, ?3, NULL)",
            params!["missing-thread", "Missing", 101i64],
        )
        .unwrap();

        migrate_thread_genie_traits(&conn).unwrap();

        let legacy_traits = get_thread_genie_traits(&conn, "legacy-thread")
            .unwrap()
            .expect("legacy thread should have traits after migration");
        assert_eq!(
            legacy_traits.version,
            crate::contracts::GENIE_TRAITS_VERSION
        );
        assert_eq!(legacy_traits.seed, 77);
        assert_eq!(legacy_traits.color_hue, 150.0);
        assert_eq!(legacy_traits.vertex_count, 18);
        assert_eq!(legacy_traits.jitter_scale, 1.1);
        assert_eq!(legacy_traits.pulse_scale, 0.9);

        let missing_traits = get_thread_genie_traits(&conn, "missing-thread")
            .unwrap()
            .expect("missing thread should get synthesized traits");
        assert_eq!(
            missing_traits.version,
            crate::contracts::GENIE_TRAITS_VERSION
        );
        assert_eq!(
            missing_traits.seed,
            crate::contracts::derive_thread_seed("missing-thread")
        );

        let raw: String = conn
            .query_row(
                "SELECT genie_traits FROM threads WHERE id = ?1",
                ["missing-thread"],
                |row| row.get(0),
            )
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            parsed.get("version").and_then(serde_json::Value::as_u64),
            Some(crate::contracts::GENIE_TRAITS_VERSION as u64)
        );
    }

    #[test]
    fn trimmed_mcp_fixture_keeps_thread_bound_agent_sessions() {
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mcp_regression_fixture.sqlite");
        assert!(
            fixture_path.exists(),
            "missing fixture at {}",
            fixture_path.display()
        );

        let temp_db =
            std::env::temp_dir().join(format!("ecky-mcp-fixture-{}.sqlite", uuid::Uuid::new_v4()));
        fs::copy(&fixture_path, &temp_db).expect("copy fixture");
        let conn = init_db(&temp_db).expect("open fixture copy");

        let raw_thread_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
            .expect("thread count");
        assert_eq!(raw_thread_count, 3);

        let visible_threads = get_all_threads(&conn).expect("threads");
        assert!(
            visible_threads
                .iter()
                .any(|thread| thread.id == "29c64fc4-803b-4d75-bac0-e0f656304881"),
            "fixture should keep the Panelka thread visible"
        );

        let last_panelka_session =
            get_thread_last_agent_session(&conn, "29c64fc4-803b-4d75-bac0-e0f656304881")
                .expect("last session")
                .expect("panelka session");
        assert_eq!(
            last_panelka_session.thread_id.as_deref(),
            Some("29c64fc4-803b-4d75-bac0-e0f656304881")
        );
        assert!(!last_panelka_session.session_id.is_empty());
    }

    #[test]
    fn init_db_enforces_foreign_keys_for_returned_connection() {
        let db_path =
            std::env::temp_dir().join(format!("ecky-fk-enforced-{}", uuid::Uuid::new_v4()));
        let conn = init_db(&db_path).unwrap();

        let foreign_keys_enabled: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys_enabled, 1);

        let err = conn
            .execute(
                "INSERT INTO messages (id, thread_id, role, content, status, timestamp)
                 VALUES ('orphan-message', 'missing-thread', 'assistant', '', 'success', 1)",
                [],
            )
            .unwrap_err();
        assert!(
            err.to_string().contains("FOREIGN KEY"),
            "orphan message insert must fail by FK, got: {err}"
        );
    }

    #[test]
    fn init_db_drops_legacy_agent_session_trace_table_and_index() {
        let db_path =
            std::env::temp_dir().join(format!("ecky-agent-trace-drop-{}", uuid::Uuid::new_v4()));
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TABLE agent_session_trace (
                    trace_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    summary TEXT NOT NULL
                )",
                [],
            )
            .unwrap();
            conn.execute(
                "CREATE INDEX idx_agent_session_trace_session_trace_id
                 ON agent_session_trace(session_id, trace_id DESC)",
                [],
            )
            .unwrap();
        }

        let conn = init_db(&db_path).unwrap();
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'agent_session_trace'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_agent_session_trace_session_trace_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);
        assert_eq!(index_count, 0);
    }

    #[test]
    fn init_db_preserves_agent_drafts_table() {
        let db_path =
            std::env::temp_dir().join(format!("ecky-agent-drafts-legacy-{}", uuid::Uuid::new_v4()));
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TABLE agent_drafts (
                    session_id TEXT NOT NULL,
                    thread_id TEXT NOT NULL,
                    base_message_id TEXT NOT NULL,
                    design_output TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                )",
                [],
            )
            .unwrap();
        }

        let conn = init_db(&db_path).unwrap();
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'agent_drafts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 1);
        assert!(table_has_column(&conn, "agent_drafts", "draft_feedback").unwrap());
    }

    #[test]
    fn init_db_creates_snapshot_verification_records_table() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-verification-records-{}",
            uuid::Uuid::new_v4()
        ));

        let conn = init_db(&db_path).unwrap();
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'verification_records'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(table_count, 1);
        assert!(table_has_column(&conn, "verification_records", "snapshot_id").unwrap());
        assert!(table_has_column(&conn, "verification_records", "artifact_digest").unwrap());
        assert!(table_has_column(&conn, "verification_records", "verification_record").unwrap());
    }

    #[test]
    fn verification_record_roundtrip_is_keyed_by_snapshot_and_preserves_artifact_digest() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();
        let record = crate::contracts::VerificationRecord {
            verification_id: "verification-1".to_string(),
            snapshot_id: "snapshot-1".to_string(),
            artifact_digest: "sha256:artifact-1".to_string(),
            passed: true,
            verifier_status: crate::contracts::VerifierStatus::Ok,
            verifier_source: Some(crate::contracts::VerifierSource::RustStructural),
        };

        upsert_verification_record(&conn, "preview-1", &record, 123).unwrap();

        assert_eq!(
            get_verification_record(&conn, "snapshot-1").unwrap(),
            Some(record)
        );
    }

    #[test]
    fn agent_draft_roundtrip_preserves_draft_feedback() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let mut draft = AgentDraft {
            preview_id: "preview-1".to_string(),
            session_id: "session-1".to_string(),
            thread_id: "thread-1".to_string(),
            base_message_id: Some("msg-1".to_string()),
            design_output: crate::contracts::DesignOutput {
                title: "Draft".to_string(),
                version_name: String::new(),
                response: "ok".to_string(),
                interaction_mode: InteractionMode::Design,
                macro_code: "draft_macro()".to_string(),
                macro_dialect: crate::contracts::MacroDialect::Legacy,
                engine_kind: crate::contracts::EngineKind::Freecad,
                source_language: crate::contracts::SourceLanguage::LegacyPython,
                geometry_backend: crate::contracts::GeometryBackend::Freecad,
                ui_spec: UiSpec { fields: Vec::new() },
                initial_params: DesignParams::from([(
                    "diameter".to_string(),
                    ParamValue::Number(12.0),
                )]),
                post_processing: None,
            },
            artifact_bundle: ArtifactBundle {
                geometry_provenance: None,
                component_dependency_lock: None,
                component_dependency_lock_digest: None,
                component_import_origins: Vec::new(),
                component_placement_evidence: Vec::new(),
                schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
                model_id: "model-1".to_string(),
                source_kind: crate::contracts::ModelSourceKind::Generated,
                engine_kind: crate::contracts::EngineKind::Freecad,
                geometry_backend: crate::contracts::GeometryBackend::Freecad,
                source_language: crate::contracts::SourceLanguage::LegacyPython,
                content_hash: "hash-1".to_string(),
                artifact_version: 1,
                fcstd_path: "/tmp/model-1.FCStd".to_string(),
                manifest_path: "/tmp/model-1.json".to_string(),
                macro_path: Some("/tmp/model-1.py".to_string()),
                model_stl_path: "/tmp/model-1.stl".to_string(),
                viewer_assets: Vec::new(),
                edge_targets: Vec::new(),
                face_targets: Vec::new(),
                callout_anchors: Vec::new(),
                measurement_guides: Vec::new(),
                export_artifacts: Vec::new(),
            },
            model_manifest: crate::contracts::ModelManifest {
                geometry_provenance: None,
                component_import_origins: Vec::new(),
                component_placement_evidence: Vec::new(),
                schema_version: crate::contracts::MODEL_RUNTIME_SCHEMA_VERSION,
                model_id: "model-1".to_string(),
                source_kind: crate::contracts::ModelSourceKind::Generated,
                source_digest: None,
                core_digest: None,
                ast_schema_version: None,
                engine_kind: crate::contracts::EngineKind::Freecad,
                source_language: crate::contracts::SourceLanguage::LegacyPython,
                geometry_backend: crate::contracts::GeometryBackend::Freecad,
                document: crate::contracts::DocumentMetadata {
                    document_name: "Doc".to_string(),
                    document_label: "Doc".to_string(),
                    source_path: None,
                    object_count: 1,
                    warnings: Vec::new(),
                },
                parts: vec![crate::contracts::PartBinding {
                    part_id: "body".to_string(),
                    freecad_object_name: "Body".to_string(),
                    label: "Body".to_string(),
                    kind: "solid".to_string(),
                    semantic_role: None,
                    viewer_asset_path: None,
                    viewer_node_ids: vec!["body".to_string()],
                    parameter_keys: Vec::new(),
                    editable: true,
                    bounds: None,
                    volume: None,
                    area: None,
                }],
                parameter_groups: Vec::new(),
                control_primitives: Vec::new(),
                control_relations: Vec::new(),
                control_views: Vec::new(),
                preview_views: Vec::new(),
                advisories: Vec::new(),
                selection_targets: Vec::new(),
                measurement_annotations: Vec::new(),
                tagged_anchors: std::collections::BTreeMap::new(),
                feature_graph: None,
                correspondence_graph: None,
                analysis_declarations: Vec::new(),
                warnings: Vec::new(),
                enrichment_state: crate::contracts::ManifestEnrichmentState {
                    status: crate::contracts::EnrichmentStatus::None,
                    proposals: Vec::new(),
                },
            },
            draft_feedback: Some(crate::contracts::AgentDraftFeedback {
                session_id: "session-1".to_string(),
                thread_id: "thread-1".to_string(),
                preview_id: "preview-1".to_string(),
                status: crate::contracts::AgentDraftFeedbackStatus::Failed,
                summary: "Model STL file not found.".to_string(),
                items: vec![crate::contracts::AgentDraftFeedbackItem {
                    code: "PREVIEW_STL_MISSING".to_string(),
                    message: "Model STL file not found.".to_string(),
                }],
                authoring_lints: Vec::new(),
                source: crate::contracts::AgentDraftFeedbackSource::StructuralVerification,
            }),
            updated_at: 123,
        };

        draft.artifact_bundle.edge_targets = vec![crate::contracts::ViewerEdgeTarget {
            target_id: "edge-1".to_string(),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Edge 1".to_string(),
            editable: true,
            start: crate::contracts::ViewerEdgePoint {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            end: crate::contracts::ViewerEdgePoint {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        }];
        draft.model_manifest.selection_targets = vec![crate::contracts::SelectionTarget {
            target_id: Some("selection-1".to_string()),
            durable_target_id: None,
            canonical_target_id: None,
            alias_ids: Vec::new(),
            part_id: "body".to_string(),
            viewer_node_id: "body".to_string(),
            label: "Body".to_string(),
            kind: crate::contracts::SelectionTargetKind::Part,
            editable: true,
            parameter_keys: Vec::new(),
            primitive_ids: Vec::new(),
            view_ids: Vec::new(),
        }];

        upsert_agent_draft(&conn, &draft).unwrap();
        let loaded = get_agent_draft_for_session(&conn, "session-1")
            .unwrap()
            .expect("draft");

        assert_eq!(loaded.draft_feedback, draft.draft_feedback);

        let projection = get_agent_draft_projection_by_preview_id(&conn, "preview-1")
            .unwrap()
            .expect("projected draft");
        assert_eq!(projection.edge_count, 1);
        assert_eq!(projection.face_count, 0);
        assert_eq!(projection.selection_target_count, 1);
        assert!(projection.artifact_bundle.edge_targets.is_empty());
        assert!(projection.model_manifest.selection_targets.is_empty());
        assert_eq!(
            projection.dense_topology_ref.as_deref(),
            Some("draft-topology:thread-1:preview-1")
        );

        let (edge_page, edge_total) = get_agent_draft_topology_json_page(
            &conn,
            "preview-1",
            "artifact_bundle",
            "$.edgeTargets",
            0,
            1,
        )
        .unwrap();
        assert_eq!(edge_total, 1);
        assert_eq!(edge_page.len(), 1);
        let edge: crate::contracts::ViewerEdgeTarget = serde_json::from_str(&edge_page[0]).unwrap();
        assert_eq!(edge.target_id, "edge-1");

        let other_thread = AgentDraft {
            preview_id: "preview-2".to_string(),
            thread_id: "thread-2".to_string(),
            ..draft.clone()
        };
        upsert_agent_draft(&conn, &other_thread).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_drafts WHERE session_id = ?1",
                ["session-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            get_agent_draft_for_session_thread(&conn, "session-1", "thread-1")
                .unwrap()
                .expect("first thread draft")
                .preview_id,
            "preview-1"
        );
        assert_eq!(
            get_agent_draft_for_session_thread(&conn, "session-1", "thread-2")
                .unwrap()
                .expect("second thread draft")
                .preview_id,
            "preview-2"
        );
        assert!(
            get_unambiguous_agent_draft_for_session(&conn, "session-1")
                .unwrap()
                .is_none(),
            "two thread-scoped drafts make no-thread resolution ambiguous"
        );
    }

    #[test]
    fn thread_window_layout_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-layout-1";
        create_or_update_thread(&conn, thread_id, "Layout Thread", 100, None).unwrap();

        let mut windows = std::collections::HashMap::new();
        windows.insert(
            "projects".to_string(),
            crate::contracts::ThreadWindowState {
                visible: true,
                minimized: false,
                x: 50.0,
                y: 60.0,
                width: 400.0,
                height: 300.0,
                z: 1,
            },
        );
        let layout = crate::contracts::ThreadWindowLayout {
            schema_version: 1,
            remember_layout: true,
            windows,
        };

        let saved = save_thread_window_layout(&conn, thread_id, &layout, 200).unwrap();
        assert!(saved);

        let loaded = get_thread_window_layout(&conn, thread_id).unwrap();
        assert_eq!(loaded, Some(layout));
    }

    #[test]
    fn thread_window_layout_returns_none_when_missing() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let thread_id = "thread-no-layout";
        create_or_update_thread(&conn, thread_id, "No Layout", 100, None).unwrap();

        let loaded = get_thread_window_layout(&conn, thread_id).unwrap();
        assert_eq!(loaded, None);
    }

    #[test]
    fn thread_window_layout_save_fails_for_missing_thread() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let layout = crate::contracts::ThreadWindowLayout {
            schema_version: 1,
            remember_layout: true,
            windows: std::collections::HashMap::new(),
        };

        let saved = save_thread_window_layout(&conn, "nonexistent", &layout, 200).unwrap();
        assert!(!saved);
    }

    #[test]
    fn thread_window_layout_delete_thread_does_not_break_others() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();

        let t1 = "thread-a";
        let t2 = "thread-b";
        create_or_update_thread(&conn, t1, "A", 100, None).unwrap();
        create_or_update_thread(&conn, t2, "B", 100, None).unwrap();

        let layout1 = crate::contracts::ThreadWindowLayout {
            schema_version: 1,
            remember_layout: true,
            windows: std::collections::HashMap::new(),
        };
        let mut windows2 = std::collections::HashMap::new();
        windows2.insert(
            "params".to_string(),
            crate::contracts::ThreadWindowState {
                visible: false,
                minimized: false,
                x: 10.0,
                y: 20.0,
                width: 300.0,
                height: 200.0,
                z: 0,
            },
        );
        let layout2 = crate::contracts::ThreadWindowLayout {
            schema_version: 1,
            remember_layout: true,
            windows: windows2,
        };

        save_thread_window_layout(&conn, t1, &layout1, 200).unwrap();
        save_thread_window_layout(&conn, t2, &layout2, 200).unwrap();

        // Soft-delete thread A
        delete_thread(&conn, t1).unwrap();

        // Thread B layout should still work
        let loaded = get_thread_window_layout(&conn, t2).unwrap();
        assert_eq!(loaded, Some(layout2));
    }

    #[test]
    fn thread_head_version_id_does_not_deserialize_message_payloads() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();
        create_or_update_thread(&conn, "dense-thread", "Dense", 100, None).unwrap();
        conn.execute(
            "INSERT INTO messages (
                id, thread_id, role, content, status, output, artifact_bundle,
                model_manifest, timestamp
             ) VALUES (?1, ?2, 'assistant', '', 'success', ?3, ?4, ?5, ?6)",
            params![
                "older-version",
                "dense-thread",
                "{not-valid-design-json",
                "{not-valid-artifact-json",
                "{not-valid-manifest-json",
                100_i64,
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (
                id, thread_id, role, content, status, output, artifact_bundle,
                model_manifest, timestamp
             ) VALUES (?1, ?2, 'assistant', '', 'error', ?3, ?4, ?5, ?6)",
            params![
                "latest-version",
                "dense-thread",
                "{also-not-valid-design-json",
                "{also-not-valid-artifact-json",
                "{also-not-valid-manifest-json",
                101_i64,
            ],
        )
        .unwrap();

        assert_eq!(
            get_thread_head_version_id(&conn, "dense-thread").unwrap(),
            Some("latest-version".to_string()),
        );
        let query_plan: String = conn
            .query_row(
                "EXPLAIN QUERY PLAN
                 SELECT id FROM messages
                 WHERE thread_id = ?1
                   AND deleted_at IS NULL
                   AND role = 'assistant'
                   AND status != 'discarded'
                   AND output IS NOT NULL
                 ORDER BY timestamp DESC, rowid DESC LIMIT 1",
                ["dense-thread"],
                |row| row.get(3),
            )
            .unwrap();
        assert!(
            query_plan.contains("idx_messages_thread_visible_timestamp")
                || query_plan.contains("idx_messages_thread_target_candidates"),
            "head lookup must use a thread/timestamp index: {query_plan}",
        );
    }

    #[test]
    fn context_messages_are_bounded_before_payload_materialization() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_internal(&conn).unwrap();
        create_or_update_thread(&conn, "context-thread", "Context", 1, None).unwrap();
        for index in 0..40_u64 {
            add_message(
                &conn,
                "context-thread",
                &Message {
                    id: format!("message-{index}"),
                    role: if index % 2 == 0 {
                        MessageRole::User
                    } else {
                        MessageRole::Assistant
                    },
                    content: format!("dialogue {index}"),
                    status: MessageStatus::Success,
                    output: None,
                    usage: None,
                    artifact_bundle: None,
                    model_manifest: None,
                    structural_verification: None,
                    agent_origin: None,
                    timestamp: index,
                    image_data: None,
                    visual_kind: None,
                    attachment_images: Vec::new(),
                },
            )
            .unwrap();
        }

        let messages = get_thread_messages_for_context(&conn, "context-thread").unwrap();
        assert_eq!(messages.len(), 10);
        assert_eq!(
            messages.first().map(|message| message.id.as_str()),
            Some("message-30")
        );
        assert_eq!(
            messages.last().map(|message| message.id.as_str()),
            Some("message-39")
        );
    }

    #[test]
    fn legacy_json_payloads_migrate_once_to_binary_without_losing_dense_topology() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-binary-payload-migration-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = init_db(&db_path).unwrap();
        create_or_update_thread(&conn, "legacy-thread", "Legacy", 1, None).unwrap();

        let mut artifact = sample_artifact_bundle("legacy-model");
        artifact
            .edge_targets
            .push(crate::contracts::ViewerEdgeTarget {
                target_id: "edge-1".to_string(),
                durable_target_id: None,
                canonical_target_id: None,
                alias_ids: Vec::new(),
                part_id: "body".to_string(),
                viewer_node_id: "Body".to_string(),
                label: "Edge 1".to_string(),
                editable: true,
                start: crate::contracts::ViewerEdgePoint {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                end: crate::contracts::ViewerEdgePoint {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            });
        let manifest = serde_json::json!({
            "modelId": "legacy-model",
            "sourceKind": "generated",
            "document": {
                "documentName": "Legacy",
                "documentLabel": "Legacy"
            },
            "selectionTargets": [{
                "targetId": "selection-1",
                "partId": "body",
                "viewerNodeId": "Body",
                "label": "Body",
                "kind": "part",
                "editable": true
            }],
            "enrichmentState": { "status": "none", "proposals": [] }
        });
        conn.execute(
            "INSERT INTO messages (
                id, thread_id, role, content, status, artifact_bundle,
                model_manifest, timestamp
             ) VALUES (?1, ?2, 'assistant', '', 'success', ?3, ?4, 2)",
            params![
                "legacy-message",
                "legacy-thread",
                serde_json::to_string(&artifact).unwrap(),
                manifest.to_string()
            ],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM schema_migrations WHERE key = 'binary-cad-payload-v1'",
            [],
        )
        .unwrap();
        drop(conn);

        migrate_history_payload_storage(&db_path).unwrap();
        let conn = init_db(&db_path).unwrap();
        let storage_types: (String, String) = conn
            .query_row(
                "SELECT typeof(artifact_bundle), typeof(model_manifest)
                 FROM messages WHERE id = 'legacy-message'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(storage_types, ("blob".to_string(), "blob".to_string()));
        let (artifact, manifest) =
            load_payload_full(&conn, PayloadOwnerKind::Message, "legacy-message").unwrap();
        assert_eq!(artifact.unwrap().edge_targets[0].target_id, "edge-1");
        assert_eq!(
            manifest.unwrap().selection_targets[0].target_id.as_deref(),
            Some("selection-1")
        );
        let projection =
            cached_payload_projection(&conn, PayloadOwnerKind::Message, "legacy-message")
                .unwrap()
                .unwrap();
        assert_eq!(projection.edge_count, 1);
        assert_eq!(projection.selection_count, 1);
        drop(conn);
        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_file(format!("{}-wal", db_path.display()));
        let _ = fs::remove_file(format!("{}-shm", db_path.display()));
    }

    #[test]
    fn startup_rejects_unmigrated_legacy_payload_without_rewriting_history() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-binary-payload-startup-gate-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = init_db(&db_path).unwrap();
        create_or_update_thread(&conn, "legacy-thread", "Legacy", 1, None).unwrap();
        let legacy_artifact =
            serde_json::to_string(&sample_artifact_bundle("legacy-model")).unwrap();
        conn.execute(
            "INSERT INTO messages (
                id, thread_id, role, content, status, artifact_bundle, timestamp
             ) VALUES ('legacy-message', 'legacy-thread', 'assistant', '', 'success', ?1, 2)",
            [legacy_artifact],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM schema_migrations WHERE key = 'binary-cad-payload-v1'",
            [],
        )
        .unwrap();
        drop(conn);

        let error = init_db(&db_path).unwrap_err().to_string();
        assert!(error.contains("CAD payload migration required"), "{error}");
        assert!(error.contains("1 message rows"), "{error}");
        assert!(error.contains("0 draft rows"), "{error}");

        let conn = Connection::open(&db_path).unwrap();
        let payload_type: String = conn
            .query_row(
                "SELECT typeof(artifact_bundle) FROM messages WHERE id = 'legacy-message'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let marker_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations
                 WHERE key = 'binary-cad-payload-v1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(payload_type, "text");
        assert_eq!(marker_count, 0);
        drop(conn);
        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_file(format!("{}-wal", db_path.display()));
        let _ = fs::remove_file(format!("{}-shm", db_path.display()));
    }

    #[test]
    fn completed_binary_migration_rejects_late_legacy_text_instead_of_falling_back() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-binary-payload-no-fallback-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = init_db(&db_path).unwrap();
        create_or_update_thread(&conn, "strict-thread", "Strict", 1, None).unwrap();
        conn.execute(
            "INSERT INTO messages (
                id, thread_id, role, content, status, artifact_bundle, timestamp
             ) VALUES ('late-text', 'strict-thread', 'assistant', '', 'success', '{}', 2)",
            [],
        )
        .unwrap();
        drop(conn);

        let error = init_db(&db_path).unwrap_err().to_string();
        assert!(error.contains("migration is marked complete"), "{error}");
        assert!(error.contains("legacy JSON rows remain"), "{error}");
        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_file(format!("{}-wal", db_path.display()));
        let _ = fs::remove_file(format!("{}-shm", db_path.display()));
    }

    #[test]
    fn malformed_legacy_payload_rolls_back_binary_migration_and_names_owner() {
        let db_path = std::env::temp_dir().join(format!(
            "ecky-binary-payload-rollback-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let conn = init_db(&db_path).unwrap();
        create_or_update_thread(&conn, "broken-thread", "Broken", 1, None).unwrap();
        conn.execute(
            "INSERT INTO messages (
                id, thread_id, role, content, status, artifact_bundle, timestamp
             ) VALUES ('broken-message', 'broken-thread', 'assistant', '', 'success', '{broken', 2)",
            [],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM schema_migrations WHERE key = 'binary-cad-payload-v1'",
            [],
        )
        .unwrap();
        drop(conn);

        let error = migrate_history_payload_storage(&db_path)
            .unwrap_err()
            .to_string();
        assert!(error.contains("message broken-message"), "{error}");
        let conn = Connection::open(&db_path).unwrap();
        let payload_type: String = conn
            .query_row(
                "SELECT typeof(artifact_bundle) FROM messages WHERE id = 'broken-message'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let marker_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations
                 WHERE key = 'binary-cad-payload-v1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(payload_type, "text");
        assert_eq!(marker_count, 0);
        drop(conn);
        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_file(format!("{}-wal", db_path.display()));
        let _ = fs::remove_file(format!("{}-shm", db_path.display()));
    }
}
