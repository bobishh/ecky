use std::fs;
use std::path::Path;

use crate::models::{AppError, AppResult};
use fontdb::{Family, Query, Source, Stretch, Style, Weight};
use rustybuzz::UnicodeBuffer;
use ttf_parser::{Face, GlyphId, OutlineBuilder};

const CURVE_SAMPLES: usize = 12;
const QUAD_SAMPLES: usize = 8;
const EPS: f64 = 1.0e-9;
const DEFAULT_FONT_PATHS: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Black.ttf",
    "/System/Library/Fonts/Supplemental/Impact.ttf",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
    "C:/Windows/Fonts/arial.ttf",
];
const DEFAULT_FONT_FAMILIES: &[&str] = &[
    "Arial Black",
    "Impact",
    "Arial",
    "Arial Unicode MS",
    "DejaVu Sans",
    "Liberation Sans",
];

#[derive(Debug, Clone, PartialEq)]
pub struct TextProfileComponent {
    pub outer_loop: Vec<[f64; 2]>,
    pub hole_loops: Vec<Vec<[f64; 2]>>,
}

#[derive(Debug, Clone)]
struct ResolvedFont {
    bytes: Vec<u8>,
    face_index: u32,
}

#[derive(Debug, Clone)]
struct GlyphLoopBuilder {
    loops: Vec<Vec<[f64; 2]>>,
    current: Vec<[f64; 2]>,
    start: Option<[f64; 2]>,
    last: Option<[f64; 2]>,
    scale: f64,
    offset: [f64; 2],
}

#[derive(Debug, Clone)]
struct LoopEntry {
    points: Vec<[f64; 2]>,
    area: f64,
}

impl GlyphLoopBuilder {
    fn new(scale: f64, offset: [f64; 2]) -> Self {
        Self {
            loops: Vec::new(),
            current: Vec::new(),
            start: None,
            last: None,
            scale,
            offset,
        }
    }

    fn finish(mut self) -> AppResult<Vec<Vec<[f64; 2]>>> {
        self.flush_current(true)?;
        Ok(self.loops)
    }

    fn map_point(&self, x: f32, y: f32) -> [f64; 2] {
        [
            self.offset[0] + f64::from(x) * self.scale,
            self.offset[1] + f64::from(y) * self.scale,
        ]
    }

    fn push_point(&mut self, point: [f64; 2]) {
        if self
            .current
            .last()
            .is_some_and(|last| distance2(*last, point) <= EPS)
        {
            self.last = Some(point);
            return;
        }
        self.current.push(point);
        self.last = Some(point);
    }

    fn flush_current(&mut self, close: bool) -> AppResult<()> {
        if self.current.is_empty() {
            self.start = None;
            self.last = None;
            return Ok(());
        }
        if close {
            if let Some(start) = self.start {
                if !self
                    .current
                    .last()
                    .is_some_and(|last| distance2(*last, start) <= EPS)
                {
                    self.current.push(start);
                }
            }
        }
        let normalized = normalize_loop(std::mem::take(&mut self.current))?;
        if normalized.len() >= 3 {
            self.loops.push(normalized);
        }
        self.start = None;
        self.last = None;
        Ok(())
    }
}

impl OutlineBuilder for GlyphLoopBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = self.flush_current(true);
        let point = self.map_point(x, y);
        self.current.push(point);
        self.start = Some(point);
        self.last = Some(point);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push_point(self.map_point(x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let Some(start) = self.last else {
            return;
        };
        let control = self.map_point(x1, y1);
        let end = self.map_point(x, y);
        for step in 1..=QUAD_SAMPLES {
            let t = step as f64 / QUAD_SAMPLES as f64;
            let mt = 1.0 - t;
            self.push_point([
                mt * mt * start[0] + 2.0 * mt * t * control[0] + t * t * end[0],
                mt * mt * start[1] + 2.0 * mt * t * control[1] + t * t * end[1],
            ]);
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let Some(start) = self.last else {
            return;
        };
        let control1 = self.map_point(x1, y1);
        let control2 = self.map_point(x2, y2);
        let end = self.map_point(x, y);
        for step in 1..=CURVE_SAMPLES {
            let t = step as f64 / CURVE_SAMPLES as f64;
            let mt = 1.0 - t;
            self.push_point([
                mt * mt * mt * start[0]
                    + 3.0 * mt * mt * t * control1[0]
                    + 3.0 * mt * t * t * control2[0]
                    + t * t * t * end[0],
                mt * mt * mt * start[1]
                    + 3.0 * mt * mt * t * control1[1]
                    + 3.0 * mt * t * t * control2[1]
                    + t * t * t * end[1],
            ]);
        }
    }

    fn close(&mut self) {
        let _ = self.flush_current(true);
    }
}

pub fn parse_text_profile(
    text: &str,
    size: f64,
    explicit_font_selector: Option<&str>,
) -> AppResult<Vec<TextProfileComponent>> {
    let value = text.trim_end_matches('\0');
    if value.is_empty() {
        return Err(AppError::validation("`text` requires a non-empty string."));
    }
    if !size.is_finite() || size <= 0.0 {
        return Err(AppError::validation(
            "`text` size must be positive and finite.",
        ));
    }

    let resolved_font = resolve_font(explicit_font_selector)?;
    let face = Face::parse(&resolved_font.bytes, resolved_font.face_index)
        .map_err(|_| AppError::validation("Failed to parse font face for `text`."))?;
    let shape_face = rustybuzz::Face::from_slice(&resolved_font.bytes, resolved_font.face_index)
        .ok_or_else(|| AppError::validation("Failed to shape text with selected font."))?;
    let units_per_em = f64::from(face.units_per_em());
    if units_per_em <= 0.0 {
        return Err(AppError::validation(
            "Selected font reported invalid units-per-em for `text`.",
        ));
    }
    let scale = size / units_per_em;
    let glyphs = rustybuzz::shape(&shape_face, &[], {
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(value);
        buffer
    });

    let mut components = Vec::new();
    let mut pen_x = 0.0f64;
    let mut pen_y = 0.0f64;
    for (info, position) in glyphs
        .glyph_infos()
        .iter()
        .zip(glyphs.glyph_positions().iter())
    {
        let offset = [
            pen_x + f64::from(position.x_offset) * scale,
            pen_y + f64::from(position.y_offset) * scale,
        ];
        let mut builder = GlyphLoopBuilder::new(scale, offset);
        if face
            .outline_glyph(GlyphId(info.glyph_id as u16), &mut builder)
            .is_some()
        {
            components.extend(classify_text_loops(builder.finish()?)?);
        }
        pen_x += f64::from(position.x_advance) * scale;
        pen_y += f64::from(position.y_advance) * scale;
    }

    if components.is_empty() {
        return Err(AppError::validation(
            "`text` produced no outline geometry with the selected font.",
        ));
    }

    Ok(components)
}

fn resolve_font(explicit_font_selector: Option<&str>) -> AppResult<ResolvedFont> {
    if let Some(selector) = explicit_font_selector
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return load_font_selector(selector).ok_or_else(|| {
            AppError::validation(format!(
                "No usable font found for `text` selector `{selector}`."
            ))
        });
    }

    if let Ok(selector) = std::env::var("ECKYCAD_FONT_PATH") {
        let selector = selector.trim().to_string();
        if !selector.is_empty() {
            if let Some(font) = load_font_selector(&selector) {
                return Ok(font);
            }
        }
    }

    for candidate in DEFAULT_FONT_PATHS {
        if let Some(font) = load_font_selector(candidate) {
            return Ok(font);
        }
    }
    for family in DEFAULT_FONT_FAMILIES {
        if let Some(font) = load_font_selector(family) {
            return Ok(font);
        }
    }

    Err(AppError::validation(
        "No usable font found for `text`. Set ECKYCAD_FONT_PATH or pass an explicit font selector.",
    ))
}

fn load_font_selector(selector: &str) -> Option<ResolvedFont> {
    if Path::new(selector).is_file() {
        let bytes = fs::read(selector).ok()?;
        return Some(ResolvedFont {
            bytes,
            face_index: 0,
        });
    }

    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let id = db.query(&Query {
        families: &[Family::Name(selector), Family::SansSerif],
        weight: Weight::NORMAL,
        stretch: Stretch::Normal,
        style: Style::Normal,
    })?;
    let face = db.face(id)?;
    match &face.source {
        Source::Binary(data) => Some(ResolvedFont {
            bytes: data.as_ref().as_ref().to_vec(),
            face_index: face.index,
        }),
        Source::File(path) => Some(ResolvedFont {
            bytes: fs::read(path).ok()?,
            face_index: face.index,
        }),
        Source::SharedFile(_, data) => Some(ResolvedFont {
            bytes: data.as_ref().as_ref().to_vec(),
            face_index: face.index,
        }),
    }
}

fn classify_text_loops(loops: Vec<Vec<[f64; 2]>>) -> AppResult<Vec<TextProfileComponent>> {
    let mut entries = loops
        .into_iter()
        .map(|points| {
            let area = signed_area(&points);
            if area.abs() <= EPS {
                Err(AppError::validation(
                    "Text glyph contour collapsed to zero area.",
                ))
            } else {
                Ok(LoopEntry { points, area })
            }
        })
        .collect::<AppResult<Vec<_>>>()?;

    entries.sort_by(|left, right| {
        right
            .area
            .abs()
            .partial_cmp(&left.area.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut parents = vec![None; entries.len()];
    for child in 0..entries.len() {
        let sample = entries[child].points[0];
        let mut parent = None;
        for candidate in 0..child {
            if point_in_polygon(sample, &entries[candidate].points) {
                parent = Some(candidate);
                break;
            }
        }
        parents[child] = parent;
    }

    let mut depths = vec![0usize; entries.len()];
    for index in 0..entries.len() {
        let mut depth = 0usize;
        let mut cursor = parents[index];
        while let Some(parent) = cursor {
            depth += 1;
            cursor = parents[parent];
        }
        depths[index] = depth;
    }

    let mut faces = Vec::new();
    for index in 0..entries.len() {
        if depths[index] % 2 != 0 {
            continue;
        }
        let mut holes = Vec::new();
        for child in 0..entries.len() {
            if parents[child] == Some(index) && depths[child] == depths[index] + 1 {
                holes.push(entries[child].points.clone());
            }
        }
        faces.push(TextProfileComponent {
            outer_loop: entries[index].points.clone(),
            hole_loops: holes,
        });
    }
    Ok(faces)
}

fn normalize_loop(mut points: Vec<[f64; 2]>) -> AppResult<Vec<[f64; 2]>> {
    if points.len() < 3 {
        return Err(AppError::validation(
            "Text glyph contour has fewer than three points.",
        ));
    }
    if points
        .first()
        .zip(points.last())
        .is_some_and(|(first, last)| distance2(*first, *last) <= EPS)
    {
        points.pop();
    }

    let mut normalized = Vec::with_capacity(points.len());
    for point in points {
        if normalized
            .last()
            .is_some_and(|last| distance2(*last, point) <= EPS)
        {
            continue;
        }
        normalized.push(point);
    }
    if normalized.len() < 3 {
        return Err(AppError::validation(
            "Text glyph contour collapsed below three points.",
        ));
    }
    if signed_area(&normalized).abs() <= EPS {
        return Err(AppError::validation(
            "Text glyph contour collapsed to zero area.",
        ));
    }
    Ok(normalized)
}

fn signed_area(points: &[[f64; 2]]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        area += points[index][0] * points[next][1] - points[next][0] * points[index][1];
    }
    area * 0.5
}

fn point_in_polygon(point: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let mut previous = polygon.len().saturating_sub(1);
    for current in 0..polygon.len() {
        let [xi, yi] = polygon[current];
        let [xj, yj] = polygon[previous];
        let intersects = ((yi > point[1]) != (yj > point[1]))
            && (point[0] < (xj - xi) * (point[1] - yi) / ((yj - yi).abs().max(EPS)) + xi);
        if intersects {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn distance2(left: [f64; 2], right: [f64; 2]) -> f64 {
    let dx = left[0] - right[0];
    let dy = left[1] - right[1];
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_profile_rejects_empty_string() {
        let err = parse_text_profile("", 12.0, None).expect_err("empty text");
        assert!(err.to_string().contains("non-empty"), "{err}");
    }

    #[test]
    fn parse_text_profile_builds_components_for_basic_glyphs() {
        let profiles = parse_text_profile("A", 12.0, None).expect("text profile");
        assert!(!profiles.is_empty());
        assert!(profiles
            .iter()
            .all(|component| !component.outer_loop.is_empty()));
    }
}
