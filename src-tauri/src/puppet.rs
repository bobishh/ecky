//! Rigid-part game assets. Scans stay mesh-native; no CAD reconstruction or skin weights.
mod smoothing;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use specta::Type;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, String>;
const MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PuppetDocument {
    pub schema_version: u32,
    pub name: String,
    pub duration_secs: f64,
    pub stepped: bool,
    pub parts: Vec<PuppetPart>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PuppetPart {
    pub id: String,
    pub name: String,
    pub asset_id: String,
    pub parent_id: Option<String>,
    /// Joint position in its parent's joint frame; millimetres, Y up.
    pub position_mm: [f64; 3],
    /// Joint point in the original imported mesh; millimetres, Y up.
    pub pivot_mm: [f64; 3],
    pub rotation_deg: [f64; 3],
    pub scale: f64,
    /// Maximum mesh displacement in millimetres at the current part scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smoothing_mm: Option<f64>,
    pub axis: String,
    pub amplitude_deg: f64,
    pub phase_deg: f64,
    pub triangle_count: u32,
    pub textured: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MeshAsset {
    primitives: Vec<Primitive>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Primitive {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    material: Material,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Material {
    color: [f32; 4],
    texture: Option<Texture>,
}
impl Default for Material {
    fn default() -> Self {
        Self {
            color: [0.65, 0.43, 0.27, 1.0],
            texture: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Texture {
    mime: String,
    base64: String,
}
impl Primitive {
    fn new(material: Material) -> Self {
        Self {
            positions: vec![],
            normals: vec![],
            uv: vec![],
            material,
        }
    }
    fn triangle(
        &mut self,
        positions: [[f32; 3]; 3],
        uv: [[f32; 2]; 3],
        normals: Option<[[f32; 3]; 3]>,
    ) {
        let a = sub(positions[1], positions[0]);
        let b = sub(positions[2], positions[0]);
        let n = normalize([
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]);
        self.positions.extend(positions);
        self.normals.extend(normals.unwrap_or([n; 3]));
        self.uv.extend(uv);
    }
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 1e-12 {
        v.map(|x| x / n)
    } else {
        [0.0, 1.0, 0.0]
    }
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read_limited(path: &Path) -> Result<Vec<u8>> {
    let file =
        fs::File::open(path).map_err(|e| format!("Cannot read '{}': {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Part exceeds 256 MB. Reduce the scan first.".into());
    }
    Ok(bytes)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or("Choose an output folder.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".puppet-{}-{serial}-{}.tmp",
        std::process::id(),
        &digest(bytes)[..16]
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&temporary, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
fn asset_path(store: &Path, id: &str) -> Result<PathBuf> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid puppet asset identity.".into());
    }
    Ok(store.join("assets").join(format!("{id}.json")))
}
fn load_asset(store: &Path, id: &str) -> Result<MeshAsset> {
    let bytes = read_limited(&asset_path(store, id)?)?;
    if digest(&bytes) != id {
        return Err("Puppet asset has changed on disk; import it again.".into());
    }
    let asset: MeshAsset = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate_asset(&asset)?;
    Ok(asset)
}
fn validate_asset(asset: &MeshAsset) -> Result<()> {
    if asset.primitives.is_empty() {
        return Err("Part has no mesh faces.".into());
    }
    for p in &asset.primitives {
        if p.positions.is_empty()
            || p.positions.len() % 3 != 0
            || p.positions.len() != p.normals.len()
            || p.positions.len() != p.uv.len()
            || p.positions
                .iter()
                .flatten()
                .chain(p.normals.iter().flatten())
                .chain(p.uv.iter().flatten())
                .any(|x| !x.is_finite())
            || p.material
                .color
                .iter()
                .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
        {
            return Err("Part contains invalid geometry or material values.".into());
        }
        if let Some(t) = &p.material.texture {
            if !["image/png", "image/jpeg"].contains(&t.mime.as_str()) {
                return Err("Use PNG or JPEG scan textures.".into());
            }
            let bytes = STANDARD.decode(&t.base64).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_BYTES
                || !(bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.starts_with(&[255, 216, 255]))
            {
                return Err("Scan texture is not a valid PNG/JPEG image.".into());
            }
        }
    }
    Ok(())
}

/// Copy imported geometry and texture bytes into an immutable, self-contained asset.
/// STL defaults to mm/Z-up; Object Capture OBJ is metres/Y-up. Caller chooses explicitly.
pub fn import_part(
    store: &Path,
    path: &Path,
    id: String,
    metres: bool,
    z_up: bool,
) -> Result<PuppetPart> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = read_limited(path)?;
    let mut asset = match ext.as_str() {
        "stl" => parse_stl(&bytes)?,
        "obj" => parse_obj(
            path,
            std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
        )?,
        _ => {
            return Err(
                "Import an STL or OBJ file. Keep OBJ materials and textures beside it.".into(),
            )
        }
    };
    let factor = if metres { 1000.0 } else { 1.0 };
    for p in &mut asset.primitives {
        for v in &mut p.positions {
            *v = v.map(|x| x * factor);
            if z_up {
                *v = [v[0], v[2], -v[1]];
            }
        }
        if z_up {
            for n in &mut p.normals {
                *n = [n[0], n[2], -n[1]];
            }
        }
    }
    validate_asset(&asset)?;
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for v in asset.primitives.iter().flat_map(|p| &p.positions) {
        for i in 0..3 {
            min[i] = min[i].min(v[i]);
            max[i] = max[i].max(v[i]);
        }
    }
    let encoded = serde_json::to_vec(&asset).map_err(|e| e.to_string())?;
    if encoded.len() as u64 > MAX_BYTES {
        return Err("Prepared part exceeds 256 MB. Reduce the scan first.".into());
    }
    let asset_id = digest(&encoded);
    let destination = asset_path(store, &asset_id)?;
    if !destination.exists() {
        atomic_write(&destination, &encoded)?;
    }
    Ok(PuppetPart {
        id,
        name: path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        asset_id,
        parent_id: None,
        position_mm: [0.0; 3],
        pivot_mm: std::array::from_fn(|i| ((min[i] + max[i]) / 2.0) as f64),
        rotation_deg: [0.0; 3],
        scale: 1.0,
        smoothing_mm: None,
        axis: "y".into(),
        amplitude_deg: 0.0,
        phase_deg: 0.0,
        triangle_count: asset
            .primitives
            .iter()
            .map(|p| p.positions.len() / 3)
            .sum::<usize>() as u32,
        textured: asset
            .primitives
            .iter()
            .any(|p| p.material.texture.is_some()),
    })
}
fn parse_stl(bytes: &[u8]) -> Result<MeshAsset> {
    let mut p = Primitive::new(Material::default());
    let count = if bytes.len() >= 84 {
        u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize
    } else {
        0
    };
    if count > 0 && bytes.len() == 84 + count * 50 {
        for chunk in bytes[84..].as_chunks::<50>().0 {
            let vertices = std::array::from_fn(|i| {
                std::array::from_fn(|axis| {
                    let offset = 12 + i * 12 + axis * 4;
                    f32::from_le_bytes(chunk[offset..offset + 4].try_into().unwrap())
                })
            });
            p.triangle(vertices, [[0.0; 2]; 3], None);
        }
    } else {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| "Invalid STL: binary triangle count does not match file length.")?;
        let mut vertices = vec![];
        for line in text.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.first() == Some(&"vertex") {
                vertices.push(vector3(&fields[1..])?);
            }
        }
        if vertices.len() % 3 != 0 {
            return Err("Invalid STL triangle.".into());
        }
        for &vertices in vertices.as_chunks::<3>().0 {
            p.triangle(vertices, [[0.0; 2]; 3], None);
        }
    }
    Ok(MeshAsset {
        primitives: vec![p],
    })
}
fn vector3(fields: &[&str]) -> Result<[f32; 3]> {
    if fields.len() < 3 {
        return Err("Expected three geometry coordinates.".into());
    }
    Ok([number(fields[0])?, number(fields[1])?, number(fields[2])?])
}
fn number(value: &str) -> Result<f32> {
    let x = value
        .parse::<f32>()
        .map_err(|_| format!("Invalid geometry number: {value}"))?;
    if !x.is_finite() {
        return Err("Geometry must have finite coordinates.".into());
    }
    Ok(x)
}
fn relative_file(root: &Path, name: &str) -> Result<PathBuf> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let path = root.join(name).canonicalize().map_err(|_| {
        format!("Missing texture or material: {name}. Keep the OBJ, MTL and images together.")
    })?;
    if !path.starts_with(&root) {
        return Err("OBJ materials and textures must stay inside the object's folder.".into());
    }
    Ok(path)
}
fn materials(path: &Path) -> Result<BTreeMap<String, Material>> {
    let bytes = read_limited(path)?;
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let mut output = BTreeMap::new();
    let mut current = String::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let value = value.trim();
        if key == "newmtl" {
            current = value.into();
            output.insert(
                current.clone(),
                Material {
                    color: [1.0; 4],
                    texture: None,
                },
            );
            continue;
        }
        let Some(material) = output.get_mut(&current) else {
            continue;
        };
        match key {
            "Kd" => {
                let v = vector3(&value.split_whitespace().collect::<Vec<_>>())?;
                material.color[..3].copy_from_slice(&v);
            }
            "d" => material.color[3] = number(value)?,
            "Tr" => material.color[3] = 1.0 - number(value)?,
            "map_Kd" => {
                if value.starts_with('-') {
                    return Err("OBJ texture transforms are not supported yet. Bake the texture transform before importing.".into());
                }
                let image = relative_file(path.parent().unwrap(), value)?;
                let bytes = read_limited(&image)?;
                let mime = if bytes.starts_with(b"\x89PNG") {
                    "image/png"
                } else if bytes.starts_with(&[255, 216, 255]) {
                    "image/jpeg"
                } else {
                    return Err("Use PNG or JPEG scan textures.".into());
                };
                material.texture = Some(Texture {
                    mime: mime.into(),
                    base64: STANDARD.encode(bytes),
                });
            }
            _ => {}
        }
    }
    Ok(output)
}
fn obj_index(raw: &str, count: usize) -> Result<usize> {
    let index = raw
        .parse::<isize>()
        .map_err(|_| "Invalid OBJ face index.")?;
    let index = if index > 0 {
        index - 1
    } else {
        count as isize + index
    };
    if index < 0 || index as usize >= count {
        return Err("OBJ face index is outside its geometry.".into());
    }
    Ok(index as usize)
}
fn parse_obj(path: &Path, text: &str) -> Result<MeshAsset> {
    let mut material_map = BTreeMap::new();
    for line in text.lines() {
        if let Some(names) = line.trim().strip_prefix("mtllib ") {
            for name in names.split_whitespace() {
                material_map.extend(materials(&relative_file(path.parent().unwrap(), name)?)?);
            }
        }
    }
    let mut vertices = vec![];
    let mut normals = vec![];
    let mut uvs = vec![];
    let mut parts: BTreeMap<String, Primitive> = BTreeMap::new();
    let mut current = String::new();
    for line in text.lines() {
        let fields: Vec<_> = line
            .split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        let Some(key) = fields.first() else { continue };
        match *key {
            "v" => vertices.push(vector3(&fields[1..])?),
            "vn" => normals.push(normalize(vector3(&fields[1..])?)),
            "vt" if fields.len() >= 3 => uvs.push([number(fields[1])?, number(fields[2])?]),
            "usemtl" => current = fields[1..].join(" "),
            "f" => {
                if !(4..=5).contains(&fields.len()) {
                    return Err("OBJ faces must be triangles or convex quads. Triangulate this mesh before import.".into());
                }
                let mut points = vec![];
                let mut tex = vec![];
                let mut ns = vec![];
                for face in &fields[1..] {
                    let indices: Vec<_> = face.split('/').collect();
                    if material_map
                        .get(&current)
                        .is_some_and(|m| m.texture.is_some())
                        && !indices.get(1).is_some_and(|uv| !uv.is_empty())
                    {
                        return Err("Textured OBJ faces require UV coordinates.".into());
                    }
                    points.push(vertices[obj_index(indices[0], vertices.len())?]);
                    tex.push(if indices.get(1).is_some_and(|x| !x.is_empty()) {
                        uvs[obj_index(indices[1], uvs.len())?]
                    } else {
                        [0.0; 2]
                    });
                    if indices.get(2).is_some_and(|x| !x.is_empty()) {
                        ns.push(normals[obj_index(indices[2], normals.len())?]);
                    }
                }
                if !current.is_empty() && !material_map.contains_key(&current) {
                    return Err(format!(
                        "Missing OBJ material '{current}'. Keep the MTL beside the OBJ."
                    ));
                }
                let p = parts.entry(current.clone()).or_insert_with(|| {
                    Primitive::new(material_map.get(&current).cloned().unwrap_or_default())
                });
                for i in 1..points.len() - 1 {
                    p.triangle(
                        [points[0], points[i], points[i + 1]],
                        [tex[0], tex[i], tex[i + 1]],
                        if ns.len() == points.len() {
                            Some([ns[0], ns[i], ns[i + 1]])
                        } else {
                            None
                        },
                    );
                }
            }
            _ => {}
        }
    }
    Ok(MeshAsset {
        primitives: parts.into_values().collect(),
    })
}

pub fn validate(document: &PuppetDocument) -> Result<()> {
    if document.schema_version != 1 {
        return Err("Unsupported puppet file version.".into());
    }
    if document.name.trim().is_empty() || document.name.len() > 200 {
        return Err("Give the puppet a name (up to 200 characters).".into());
    }
    if document.parts.is_empty() || document.parts.len() > 64 {
        return Err("A puppet needs between 1 and 64 separate parts.".into());
    }
    if !document.duration_secs.is_finite() || !(0.2..=60.0).contains(&document.duration_secs) {
        return Err("Motion duration must be between 0.2 and 60 seconds.".into());
    }
    let ids: BTreeSet<_> = document.parts.iter().map(|p| p.id.as_str()).collect();
    if ids.len() != document.parts.len() {
        return Err("Each part needs a unique identity.".into());
    }
    for part in &document.parts {
        asset_path(Path::new(""), &part.asset_id)?;
        if part.id.is_empty()
            || !part
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || part.name.trim().is_empty()
        {
            return Err("Each part needs a valid identity and a name.".into());
        }
        if !["x", "y", "z"].contains(&part.axis.as_str())
            || !part.scale.is_finite()
            || !(0.0001..=10000.0).contains(&part.scale)
            || !part.amplitude_deg.is_finite()
            || !(0.0..=180.0).contains(&part.amplitude_deg)
            || !part.phase_deg.is_finite()
            || part
                .position_mm
                .iter()
                .chain(&part.pivot_mm)
                .chain(&part.rotation_deg)
                .any(|x| !x.is_finite() || x.abs() > 1e8)
        {
            return Err(format!("Invalid joint settings for '{}'.", part.name));
        }
        if part
            .smoothing_mm
            .is_some_and(|limit| !(0.0..=2.0).contains(&limit))
        {
            return Err(format!(
                "Smoothing limit for '{}' must be between 0 and 2 mm.",
                part.name
            ));
        }
        let mut seen = BTreeSet::new();
        seen.insert(part.id.as_str());
        let mut parent = part.parent_id.as_deref();
        while let Some(id) = parent {
            if !seen.insert(id) {
                return Err("Part parents form a cycle. Choose a different parent.".into());
            }
            parent = document
                .parts
                .iter()
                .find(|p| p.id == id)
                .ok_or("A parent part is missing.")?
                .parent_id
                .as_deref();
        }
    }
    Ok(())
}

struct Glb {
    binary: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
    images: Vec<Value>,
    textures: Vec<Value>,
    materials: Vec<Value>,
}
impl Glb {
    fn new() -> Self {
        Self {
            binary: vec![],
            views: vec![],
            accessors: vec![],
            images: vec![],
            textures: vec![],
            materials: vec![],
        }
    }
    fn bytes(&mut self, bytes: &[u8]) -> usize {
        while self.binary.len() % 4 != 0 {
            self.binary.push(0)
        }
        let i = self.views.len();
        self.views
            .push(json!({"buffer":0,"byteOffset":self.binary.len(),"byteLength":bytes.len()}));
        self.binary.extend_from_slice(bytes);
        i
    }
    fn floats(&mut self, values: &[f32], size: usize, bounds: bool) -> usize {
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.bytes(&data);
        let mut accessor = json!({"bufferView":view,"componentType":5126,"count":values.len()/size,"type":match size {1=>"SCALAR",2=>"VEC2",3=>"VEC3",_=>"VEC4"}});
        if bounds {
            let min: Vec<f32> = (0..size)
                .map(|i| {
                    values
                        .iter()
                        .skip(i)
                        .step_by(size)
                        .copied()
                        .fold(f32::INFINITY, f32::min)
                })
                .collect();
            let max: Vec<f32> = (0..size)
                .map(|i| {
                    values
                        .iter()
                        .skip(i)
                        .step_by(size)
                        .copied()
                        .fold(f32::NEG_INFINITY, f32::max)
                })
                .collect();
            accessor["min"] = json!(min);
            accessor["max"] = json!(max);
        }
        let i = self.accessors.len();
        self.accessors.push(accessor);
        i
    }
    fn material(&mut self, m: &Material) -> Result<usize> {
        let mut pbr = json!({"baseColorFactor":m.color,"metallicFactor":0,"roughnessFactor":0.85});
        if let Some(t) = &m.texture {
            let bytes = STANDARD.decode(&t.base64).map_err(|e| e.to_string())?;
            let view = self.bytes(&bytes);
            let source = self.images.len();
            self.images
                .push(json!({"bufferView":view,"mimeType":t.mime}));
            let index = self.textures.len();
            self.textures.push(json!({"source":source}));
            pbr["baseColorTexture"] = json!({"index":index});
        }
        let index = self.materials.len();
        self.materials.push(json!({"pbrMetallicRoughness":pbr,"doubleSided":true,"alphaMode":if m.color[3]<1.0 {"BLEND"} else {"OPAQUE"}}));
        Ok(index)
    }
}
fn quaternion(axis: usize, degrees: f64) -> [f64; 4] {
    let half = degrees.to_radians() / 2.0;
    let mut q = [0.0, 0.0, 0.0, half.cos()];
    q[axis] = half.sin();
    q
}
fn multiply(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
fn rest_rotation(part: &PuppetPart) -> [f64; 4] {
    multiply(
        multiply(
            quaternion(0, part.rotation_deg[0]),
            quaternion(1, part.rotation_deg[1]),
        ),
        quaternion(2, part.rotation_deg[2]),
    )
}
fn build_glb(document: &PuppetDocument, assets: &[MeshAsset]) -> Result<(Value, Vec<u8>)> {
    validate(document)?;
    let mut glb = Glb::new();
    let mut nodes = vec![];
    let mut meshes = vec![];
    let mut roots = vec![];
    let mut channels = vec![];
    let mut samplers = vec![];
    for (i, (part, asset)) in document.parts.iter().zip(assets).enumerate() {
        validate_asset(asset)?;
        let smoothed = smoothing::smooth(asset, part.smoothing_mm.unwrap_or_default() / part.scale);
        let mut corner_offset = 0;
        let mut primitives = vec![];
        for p in &asset.primitives {
            let mut positions = Vec::with_capacity(p.positions.len() * 3);
            let mut normals = Vec::with_capacity(p.normals.len() * 3);
            for (corner, (&position, &normal)) in p.positions.iter().zip(&p.normals).enumerate() {
                let (position, normal) = smoothed.as_ref().map_or((position, normal), |mesh| {
                    let vertex = &mesh.vertices[mesh.corners[corner_offset + corner]];
                    (vertex.position, vertex.normal.unwrap_or(normal))
                });
                positions.extend(position.map(|value| value * 0.001));
                normals.extend(normal);
            }
            corner_offset += p.positions.len();
            let position = glb.floats(&positions, 3, true);
            let normal = glb.floats(&normals, 3, false);
            let uv = glb.floats(
                &p.uv
                    .iter()
                    .flat_map(|uv| [uv[0], 1.0 - uv[1]])
                    .collect::<Vec<_>>(),
                2,
                false,
            );
            let material = glb.material(&p.material)?;
            primitives.push(json!({"attributes":{"POSITION":position,"NORMAL":normal,"TEXCOORD_0":uv},"material":material,"mode":4}));
        }
        meshes.push(json!({"name":format!("geometry_{}",part.id),"primitives":primitives}));
        let mut children = vec![i * 2 + 1];
        children.extend(
            document
                .parts
                .iter()
                .enumerate()
                .filter(|(_, p)| p.parent_id.as_deref() == Some(&part.id))
                .map(|(j, _)| j * 2),
        );
        if part.parent_id.is_none() {
            roots.push(i * 2)
        }
        nodes.push(json!({"name":format!("joint_{}",part.id),"translation":part.position_mm.map(|x|x*0.001),"rotation":rest_rotation(part),"children":children,
            "extras":{"puppetPartId":part.id,"label":part.name}}));
        nodes.push(json!({"name":format!("mesh_{}",part.id),"mesh":i,"translation":part.pivot_mm.map(|x|-x*part.scale*0.001),"scale":([part.scale;3])}));
        if part.amplitude_deg > 0.0 {
            let samples = if document.stepped {
                (document.duration_secs * 12.0).ceil() as usize
            } else {
                (document.duration_secs * 30.0).ceil() as usize
            };
            let times: Vec<_> = (0..=samples)
                .map(|s| (s as f64 * document.duration_secs / samples as f64) as f32)
                .collect();
            let input = glb.floats(&times, 1, true);
            let axis = match part.axis.as_str() {
                "x" => 0,
                "z" => 2,
                _ => 1,
            };
            let rotations: Vec<_> = (0..=samples)
                .flat_map(|s| {
                    let angle = part.amplitude_deg
                        * (std::f64::consts::TAU * s as f64 / samples as f64
                            + part.phase_deg.to_radians())
                        .sin();
                    multiply(rest_rotation(part), quaternion(axis, angle)).map(|x| x as f32)
                })
                .collect();
            let output = glb.floats(&rotations, 4, false);
            let sampler = samplers.len();
            samplers.push(json!({"input":input,"output":output,"interpolation":if document.stepped {"STEP"} else {"LINEAR"}}));
            channels.push(json!({"sampler":sampler,"target":{"node":i*2,"path":"rotation"}}));
        }
    }
    while glb.binary.len() % 4 != 0 {
        glb.binary.push(0)
    }
    let mut json = json!({"asset":{"version":"2.0","generator":"Ecky rigid puppet"},"scene":0,"scenes":[{"name":document.name,"nodes":roots}],
        "nodes":nodes,"meshes":meshes,"materials":glb.materials,"buffers":[{"byteLength":glb.binary.len()}],"bufferViews":glb.views,"accessors":glb.accessors});
    if !glb.images.is_empty() {
        json["images"] = json!(glb.images);
        json["textures"] = json!(glb.textures);
    }
    if !channels.is_empty() {
        json["animations"] =
            json!([{"name":"puppet_motion","channels":channels,"samplers":samplers}]);
    }
    Ok((json, glb.binary))
}
fn encode_glb(json: &Value, binary: &[u8]) -> Result<Vec<u8>> {
    let mut encoded = serde_json::to_vec(json).map_err(|e| e.to_string())?;
    while encoded.len() % 4 != 0 {
        encoded.push(b' ')
    }
    let length = 12 + 8 + encoded.len() + 8 + binary.len();
    if length as u64 > MAX_BYTES {
        return Err("Export exceeds 256 MB. Reduce scan detail before exporting.".into());
    }
    let mut out = vec![];
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend((length as u32).to_le_bytes());
    out.extend((encoded.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(encoded);
    out.extend((binary.len() as u32).to_le_bytes());
    out.extend(b"BIN\0");
    out.extend(binary);
    Ok(out)
}
pub fn export_glb(store: &Path, document: &PuppetDocument) -> Result<Vec<u8>> {
    validate(document)?;
    let assets = document
        .parts
        .iter()
        .map(|p| load_asset(store, &p.asset_id))
        .collect::<Result<Vec<_>>>()?;
    let (json, binary) = build_glb(document, &assets)?;
    encode_glb(&json, &binary)
}
pub fn preview(store: &Path, document: &PuppetDocument) -> Result<PathBuf> {
    let bytes = export_glb(store, document)?;
    let path = store
        .join("previews")
        .join(format!("{}.glb", digest(&bytes)));
    if !path.exists() {
        atomic_write(&path, &bytes)?
    }
    // These are disposable previews, not saved puppet versions. Keep the current
    // file and the most recent previous file while the viewport swaps scenes.
    if let Ok(entries) = fs::read_dir(store.join("previews")) {
        let mut previous: Vec<_> = entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let candidate = entry.path();
                if candidate == path
                    || candidate.extension().and_then(|s| s.to_str()) != Some("glb")
                {
                    return None;
                }
                Some((entry.metadata().ok()?.modified().ok()?, candidate))
            })
            .collect();
        previous.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        for (_, obsolete) in previous.into_iter().skip(1) {
            let _ = fs::remove_file(obsolete);
        }
    }
    Ok(path)
}
pub fn save(store: &Path, document: &PuppetDocument, path: &Path) -> Result<()> {
    validate(document)?;
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive
        .start_file("document.json", options)
        .map_err(|e| e.to_string())?;
    archive
        .write_all(&serde_json::to_vec(document).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let ids: BTreeSet<_> = document.parts.iter().map(|p| &p.asset_id).collect();
    let mut total = 0u64;
    for id in ids {
        load_asset(store, id)?;
        let bytes = read_limited(&asset_path(store, id)?)?;
        total += bytes.len() as u64;
        if total > MAX_BYTES {
            return Err("Puppet exceeds 256 MB. Reduce scan detail.".into());
        }
        archive
            .start_file(format!("assets/{id}.json"), options)
            .map_err(|e| e.to_string())?;
        archive.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    atomic_write(
        path,
        &archive.finish().map_err(|e| e.to_string())?.into_inner(),
    )
}
pub fn open(store: &Path, path: &Path) -> Result<PuppetDocument> {
    let bytes = read_limited(path)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    fn entry(
        archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
        name: &str,
        total: &mut u64,
    ) -> Result<Vec<u8>> {
        let file = archive
            .by_name(name)
            .map_err(|e| format!("Puppet archive is missing {name}: {e}"))?;
        *total += file.size();
        if *total > MAX_BYTES {
            return Err("Puppet archive exceeds 256 MB when unpacked.".into());
        }
        let mut bytes = vec![];
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Puppet archive entry is too large.".into());
        }
        Ok(bytes)
    }
    let mut total = 0;
    let document: PuppetDocument =
        serde_json::from_slice(&entry(&mut archive, "document.json", &mut total)?)
            .map_err(|e| e.to_string())?;
    validate(&document)?;
    let ids: BTreeSet<_> = document.parts.iter().map(|p| &p.asset_id).collect();
    let mut prepared = vec![];
    for id in ids {
        let bytes = entry(&mut archive, &format!("assets/{id}.json"), &mut total)?;
        if digest(&bytes) != *id {
            return Err("Puppet archive asset checksum mismatch.".into());
        }
        let asset: MeshAsset = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        validate_asset(&asset)?;
        prepared.push((asset_path(store, id)?, bytes));
    }
    for (path, bytes) in prepared {
        if !path.exists() {
            atomic_write(&path, &bytes)?
        }
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PuppetDocument, MeshAsset) {
        let mut primitive = Primitive::new(Material::default());
        primitive.triangle(
            [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            [[0.0; 2]; 3],
            None,
        );
        let asset = MeshAsset {
            primitives: vec![primitive],
        };
        let part = PuppetPart {
            id: "body".into(),
            name: "Body".into(),
            asset_id: digest(&serde_json::to_vec(&asset).unwrap()),
            parent_id: None,
            position_mm: [0.0; 3],
            pivot_mm: [0.0; 3],
            rotation_deg: [0.0; 3],
            scale: 1.0,
            smoothing_mm: None,
            axis: "y".into(),
            amplitude_deg: 0.0,
            phase_deg: 0.0,
            triangle_count: 1,
            textured: false,
        };
        let head = PuppetPart {
            id: "head".into(),
            name: "Head".into(),
            parent_id: Some("body".into()),
            position_mm: [0.0, 100.0, 0.0],
            pivot_mm: [5.0, 0.0, 0.0],
            amplitude_deg: 30.0,
            ..part.clone()
        };
        (
            PuppetDocument {
                schema_version: 1,
                name: "Unicorn".into(),
                duration_secs: 2.0,
                stepped: false,
                parts: vec![part, head],
            },
            asset,
        )
    }
    fn temporary() -> PathBuf {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ecky-puppet-{}-{now}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }
    #[test]
    fn given_two_rigid_parts_when_exported_then_keep_hierarchy_pivots_and_motion() {
        let (document, asset) = fixture();
        let (json, binary) = build_glb(&document, &[asset.clone(), asset]).unwrap();
        assert_eq!(json["asset"]["version"], "2.0");
        assert_eq!(json["nodes"][0]["children"], json!([1, 2]));
        assert_eq!(json["nodes"][2]["translation"], json!([0.0, 0.1, 0.0]));
        assert_eq!(json["nodes"][3]["translation"], json!([-0.005, 0.0, 0.0]));
        assert_eq!(json["animations"][0]["name"], "puppet_motion");
        assert_eq!(json["animations"][0]["channels"][0]["target"]["node"], 2);
        let positions = json["accessors"][0]["max"].as_array().unwrap();
        assert!((positions[0].as_f64().unwrap() - 0.01).abs() < 1e-7);
        let bytes = encode_glb(&json, &binary).unwrap();
        assert_eq!(&bytes[..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
            bytes.len()
        );
        assert!(!serde_json::to_string(&json).unwrap().contains("assetId"));
    }
    #[test]
    fn given_scan_ripple_when_exporting_then_limit_displacement_and_keep_the_open_rim() {
        let center = [0.0, 0.0, 0.8];
        let rim = [
            [-5.0, -5.0, 0.0],
            [5.0, -5.0, 0.0],
            [5.0, 5.0, 0.0],
            [-5.0, 5.0, 0.0],
        ];
        let mut primitive = Primitive::new(Material::default());
        for i in 0..rim.len() {
            primitive.triangle(
                [center, rim[i], rim[(i + 1) % rim.len()]],
                [[0.0; 2]; 3],
                None,
            );
        }
        let mut document = fixture().0;
        document.parts.truncate(1);
        document.parts[0].scale = 2.0;
        let mut options = serde_json::to_value(&document).unwrap();
        options["parts"][0]["smoothingMm"] = json!(0.2);
        let smoothed: PuppetDocument = serde_json::from_value(options).unwrap();
        let assets = [MeshAsset {
            primitives: vec![primitive],
        }];
        let (_, original) = build_glb(&document, &assets).unwrap();
        let (_, finished) = build_glb(&smoothed, &assets).unwrap();

        for (before, after) in original[..144]
            .as_chunks::<36>()
            .0
            .iter()
            .zip(finished[..144].as_chunks::<36>().0)
        {
            let z = |triangle: &[u8; 36]| f32::from_le_bytes(triangle[8..12].try_into().unwrap());
            let move_mm = (z(before) - z(after)) * 2_000.0;
            assert!((0.19..=0.2001).contains(&move_mm));
            assert_eq!(before[12..], after[12..]);
        }
    }
    #[test]
    fn given_a_cyclic_parent_or_nonfinite_joint_when_exporting_then_reject() {
        let (mut document, _) = fixture();
        document.parts[0].parent_id = Some("head".into());
        assert!(validate(&document).unwrap_err().contains("cycle"));
        document.parts[0].parent_id = None;
        document.parts[1].pivot_mm[0] = f64::NAN;
        assert!(validate(&document).is_err());
    }
    #[test]
    fn given_textured_obj_when_imported_and_exported_then_embed_color_uv_and_image() {
        let dir = temporary();
        fs::write(dir.join("part.obj"),"mtllib part.mtl\nv 0 0 0\nv 0.01 0 0\nv 0 0.01 0\nvt 0 0\nvt 1 0\nvt 0 1\nusemtl clay\nf 1/1 2/2 3/3\n").unwrap();
        fs::write(
            dir.join("part.mtl"),
            "newmtl clay\nKd 0.8 0.2 0.5\nmap_Kd color.png\n",
        )
        .unwrap();
        let png=STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=").unwrap();
        fs::write(dir.join("color.png"), &png).unwrap();
        let part = import_part(&dir, &dir.join("part.obj"), "head".into(), true, false).unwrap();
        assert!(part.textured);
        let asset = load_asset(&dir, &part.asset_id).unwrap();
        assert_eq!(asset.primitives[0].uv[1], [1.0, 0.0]);
        let document = PuppetDocument {
            parts: vec![part],
            ..fixture().0
        };
        let (json, binary) = build_glb(&document, &[asset]).unwrap();
        let view = json["images"][0]["bufferView"].as_u64().unwrap() as usize;
        let offset = json["bufferViews"][view]["byteOffset"].as_u64().unwrap() as usize;
        assert_eq!(&binary[offset..offset + png.len()], &png);
        assert_eq!(json["images"][0]["mimeType"], "image/png");
        let archive = dir.join("unicorn.eckypuppet");
        save(&dir, &document, &archive).unwrap();
        let restored = dir.join("restored");
        let reopened = open(&restored, &archive).unwrap();
        assert_eq!(
            export_glb(&dir, &document).unwrap(),
            export_glb(&restored, &reopened).unwrap()
        );
        fs::remove_file(dir.join("color.png")).unwrap();
        assert!(
            import_part(&dir, &dir.join("part.obj"), "other".into(), true, false)
                .unwrap_err()
                .contains("Missing texture")
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn given_stl_in_millimetres_and_z_up_when_imported_then_keep_scale_and_rotate_up_axis() {
        let dir = temporary();
        let path = dir.join("part.stl");
        fs::write(&path,"solid test\nfacet normal 0 1 0\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 0 20\nendloop\nendfacet\nendsolid").unwrap();
        let part = import_part(&dir, &path, "body".into(), false, true).unwrap();
        let asset = load_asset(&dir, &part.asset_id).unwrap();
        assert_eq!(asset.primitives[0].positions[2], [0.0, 20.0, 0.0]);
        assert_eq!(part.triangle_count, 1);
        assert!(!part.textured);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn given_stepped_motion_when_exported_then_use_step_interpolation_and_unit_quaternions() {
        let (mut document, asset) = fixture();
        document.stepped = true;
        document.parts[1].rotation_deg = [20.0, 30.0, 40.0];
        let (json, binary) = build_glb(&document, &[asset.clone(), asset]).unwrap();
        let sampler = &json["animations"][0]["samplers"][0];
        assert_eq!(sampler["interpolation"], "STEP");
        let accessor = &json["accessors"][sampler["output"].as_u64().unwrap() as usize];
        assert_eq!(accessor["count"], 25);
        let view = &json["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
        let offset = view["byteOffset"].as_u64().unwrap() as usize;
        let values: Vec<_> = binary[offset..offset + 25 * 16]
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(f32::from_le_bytes)
            .collect();
        for q in values.as_chunks::<4>().0 {
            assert!((q.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-6);
        }
        for i in 0..4 {
            assert!((values[i] - values[24 * 4 + i]).abs() < 1e-6);
        }
    }
}
