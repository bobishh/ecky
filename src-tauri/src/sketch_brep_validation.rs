use crate::models::{
    BrepHiddenLineProjectionResponse, BrepProjectedEdge2d, SketchBrepProjectionValidation,
    SketchDefinition, SketchDocument, SketchPrimitive, SketchPrimitiveKind, SketchValidationIssue,
    SketchValidationSeverity, SketchView,
};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Bounds2d {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BoundsDelta {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

#[derive(Debug, Clone)]
struct ClosedProfile<'a> {
    sketch: &'a SketchDefinition,
    primitive: &'a SketchPrimitive,
    bounds: Bounds2d,
}

pub fn validate_sketch_brep_hidden_line_projection(
    document: &SketchDocument,
    projection: &BrepHiddenLineProjectionResponse,
    tolerance: f64,
) -> SketchBrepProjectionValidation {
    let tolerance = if tolerance.is_finite() && tolerance >= 0.0 {
        tolerance
    } else {
        0.0
    };
    let mut issues = Vec::new();
    let mut evidence = Vec::new();

    for view in [SketchView::Front, SketchView::Top, SketchView::Side] {
        let label = view_label(&view);
        let Some(profile) = find_closed_profile(document, &view) else {
            issues.push(SketchValidationIssue {
                sketch_id: view_key(&view).to_string(),
                primitive_id: None,
                severity: SketchValidationSeverity::Error,
                message: format!("{label} sketch view has no active closed profile."),
            });
            continue;
        };

        let Some(brep_bounds) = projected_bounds(projection, &view) else {
            issues.push(SketchValidationIssue {
                sketch_id: profile.sketch.sketch_id.clone(),
                primitive_id: Some(profile.primitive.primitive_id.clone()),
                severity: SketchValidationSeverity::Error,
                message: format!("{label} BRep projection has no visible or hidden edge points."),
            });
            continue;
        };

        let delta = profile.bounds.delta(brep_bounds);
        let max_delta = delta.max_component();
        if max_delta > tolerance {
            issues.push(SketchValidationIssue {
                sketch_id: profile.sketch.sketch_id.clone(),
                primitive_id: Some(profile.primitive.primitive_id.clone()),
                severity: SketchValidationSeverity::Error,
                message: format!(
                    "{label} bounds mismatch: sketch {}, brep {}, maxDelta={:.6}, tolerance={:.6}.",
                    profile.bounds.format_values(),
                    brep_bounds.format_values(),
                    max_delta,
                    tolerance
                ),
            });
        } else {
            evidence.push(format!(
                "{label} bounds match within tolerance {:.6}: sketch {}, brep {}, maxDelta={:.6}.",
                tolerance,
                profile.bounds.format_values(),
                brep_bounds.format_values(),
                max_delta
            ));
        }
    }

    SketchBrepProjectionValidation {
        passed: issues.is_empty(),
        issues,
        evidence,
    }
}

fn find_closed_profile<'a>(
    document: &'a SketchDocument,
    view: &SketchView,
) -> Option<ClosedProfile<'a>> {
    let active_sketch_id = document.active_sketch_id.as_deref();
    let active_profile = active_sketch_id.and_then(|active_id| {
        document
            .sketches
            .iter()
            .filter(|sketch| sketch.sketch_id == active_id && sketch.view == *view)
            .find_map(closed_profile_for_sketch)
    });

    active_profile.or_else(|| {
        document
            .sketches
            .iter()
            .filter(|sketch| sketch.view == *view)
            .find_map(closed_profile_for_sketch)
    })
}

fn closed_profile_for_sketch(sketch: &SketchDefinition) -> Option<ClosedProfile<'_>> {
    sketch.primitives.iter().find_map(|primitive| {
        if !primitive.closed {
            return None;
        }
        primitive_bounds(primitive).map(|bounds| ClosedProfile {
            sketch,
            primitive,
            bounds,
        })
    })
}

fn primitive_bounds(primitive: &SketchPrimitive) -> Option<Bounds2d> {
    match primitive.kind {
        SketchPrimitiveKind::Circle => {
            let radius = primitive.radius?;
            if !radius.is_finite() || radius <= 0.0 {
                return None;
            }
            let center = primitive.points.first().copied().unwrap_or([0.0, 0.0]);
            if !center[0].is_finite() || !center[1].is_finite() {
                return None;
            }
            Some(Bounds2d {
                min_x: center[0] - radius,
                max_x: center[0] + radius,
                min_y: center[1] - radius,
                max_y: center[1] + radius,
            })
        }
        _ => Bounds2d::from_points(&primitive.points),
    }
}

fn projected_bounds(
    projection: &BrepHiddenLineProjectionResponse,
    view: &SketchView,
) -> Option<Bounds2d> {
    projection
        .views
        .iter()
        .filter(|candidate| candidate.view == *view)
        .flat_map(|candidate| {
            candidate
                .visible_edges
                .iter()
                .chain(candidate.hidden_edges.iter())
        })
        .filter_map(edge_bounds)
        .reduce(Bounds2d::union)
}

fn edge_bounds(edge: &BrepProjectedEdge2d) -> Option<Bounds2d> {
    Bounds2d::from_points(&edge.points)
}

fn view_label(view: &SketchView) -> &'static str {
    match view {
        SketchView::Front => "Front",
        SketchView::Top => "Top",
        SketchView::Side => "Side",
        SketchView::Custom => "Custom",
    }
}

fn view_key(view: &SketchView) -> &'static str {
    match view {
        SketchView::Front => "front",
        SketchView::Top => "top",
        SketchView::Side => "side",
        SketchView::Custom => "custom",
    }
}

impl Bounds2d {
    fn from_points(points: &[[f64; 2]]) -> Option<Self> {
        let first = points.first()?;
        if !first[0].is_finite() || !first[1].is_finite() {
            return None;
        }
        let mut bounds = Self {
            min_x: first[0],
            max_x: first[0],
            min_y: first[1],
            max_y: first[1],
        };
        for point in points.iter().skip(1) {
            if !point[0].is_finite() || !point[1].is_finite() {
                return None;
            }
            bounds.min_x = bounds.min_x.min(point[0]);
            bounds.max_x = bounds.max_x.max(point[0]);
            bounds.min_y = bounds.min_y.min(point[1]);
            bounds.max_y = bounds.max_y.max(point[1]);
        }
        Some(bounds)
    }

    fn union(self, other: Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            max_x: self.max_x.max(other.max_x),
            min_y: self.min_y.min(other.min_y),
            max_y: self.max_y.max(other.max_y),
        }
    }

    fn delta(self, other: Self) -> BoundsDelta {
        BoundsDelta {
            min_x: (self.min_x - other.min_x).abs(),
            max_x: (self.max_x - other.max_x).abs(),
            min_y: (self.min_y - other.min_y).abs(),
            max_y: (self.max_y - other.max_y).abs(),
        }
    }

    fn format_values(self) -> String {
        format!(
            "minX={:.6} maxX={:.6} minY={:.6} maxY={:.6}",
            self.min_x, self.max_x, self.min_y, self.max_y
        )
    }
}

impl BoundsDelta {
    fn max_component(self) -> f64 {
        self.min_x.max(self.max_x).max(self.min_y).max(self.max_y)
    }
}
