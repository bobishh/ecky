use std::collections::BTreeMap;
use std::fmt;

mod selectors;
mod signatures;
pub use selectors::{parse_core_edge_selector_payload, parse_core_face_selector_payload};
#[doc(hidden)]
pub use signatures::verify_core_program_with_literal_dimensions;
pub use signatures::{verify_core_program, verify_core_program_strict_units};

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(raw: u64) -> Self {
                Self(raw)
            }

            pub const fn raw(self) -> u64 {
                self.0
            }
        }
    };
}

opaque_id!(ProgramId);
opaque_id!(PartId);
opaque_id!(AnalysisId);
opaque_id!(AnalysisClauseId);
opaque_id!(ParamId);
opaque_id!(NodeId);
opaque_id!(SourceFileId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub file: Option<SourceFileId>,
    pub start: u32,
    pub end: u32,
}

impl SourceSpan {
    pub const fn new(file: Option<SourceFileId>, start: u32, end: u32) -> Self {
        Self { file, start, end }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompilerErrorKind {
    Parse,
    Resolve,
    TypeMismatch,
    UnsupportedFeature,
    Backend,
    Internal,
}

impl fmt::Display for CompilerErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Parse => "parse",
            Self::Resolve => "resolve",
            Self::TypeMismatch => "type-mismatch",
            Self::UnsupportedFeature => "unsupported-feature",
            Self::Backend => "backend",
            Self::Internal => "internal",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerError {
    pub kind: CompilerErrorKind,
    pub message: Box<str>,
    pub primary_span: Option<SourceSpan>,
    pub secondary_spans: Vec<SourceSpan>,
    pub notes: Vec<String>,
    pub help: Option<Box<str>>,
}

impl CompilerError {
    pub fn new(kind: CompilerErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into().into_boxed_str(),
            primary_span: None,
            secondary_spans: Vec::new(),
            notes: Vec::new(),
            help: None,
        }
    }

    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.primary_span = Some(span);
        self
    }

    pub fn with_secondary_span(mut self, span: SourceSpan) -> Self {
        self.secondary_spans.push(span);
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into().into_boxed_str());
        self
    }

    pub fn render_with_source(&self, source: &str) -> String {
        let mut output = self.to_string();
        if let Some(span) = self.primary_span {
            let start = (span.start as usize).min(source.len());
            let end = (span.end as usize)
                .min(source.len())
                .max(start.saturating_add(1).min(source.len()));
            let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
            let line_end = source[start..]
                .find('\n')
                .map_or(source.len(), |index| start + index);
            let line_number = source[..line_start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            let column = source[line_start..start].chars().count() + 1;
            let source_line = &source[line_start..line_end];
            let underline_end = end.min(line_end);
            let underline_width = source[start..underline_end].chars().count().max(1);
            let gutter_width = line_number.to_string().len();
            output.push_str(&format!(
                "\n--> line {line_number}, column {column}\n{line_number:>gutter_width$} | {source_line}\n{:>gutter_width$} | {}{}",
                "",
                " ".repeat(column.saturating_sub(1)),
                "^".repeat(underline_width),
            ));
        }
        for note in &self.notes {
            output.push_str("\nnote: ");
            output.push_str(note);
        }
        if let Some(help) = &self.help {
            output.push_str("\nhelp: ");
            output.push_str(help);
        }
        output
    }
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for CompilerError {}

pub type CoreResult<T> = Result<T, CompilerError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreValueKind {
    Any,
    Number,
    Boolean,
    Text,
    List,
    Point2,
    Point3,
    Sketch,
    Path,
    Frame,
    Mesh,
    Compound,
    Solid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreEdgeAxis {
    X,
    Y,
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreEdgeBound {
    Min,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreEdgeSelectorClause {
    Axis(CoreEdgeAxis),
    Boundary {
        axis: CoreEdgeAxis,
        bound: CoreEdgeBound,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreFaceAreaRank {
    Min,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreFaceSelectorClause {
    Boundary {
        axis: CoreEdgeAxis,
        bound: CoreEdgeBound,
    },
    Planar,
    Normal(CoreEdgeAxis),
    Area(CoreFaceAreaRank),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreSymbol {
    Start,
    End,
    Xy,
    Yz,
    Xz,
    Min,
    Center,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreParameterKind {
    Number,
    Boolean,
    Text,
    Choice,
    Image,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreParameterValue {
    Number(f64),
    Boolean(bool),
    Text(String),
    Choice(String),
    Image(String),
}

impl CoreParameterValue {
    pub fn kind(&self) -> CoreValueKind {
        match self {
            Self::Number(_) => CoreValueKind::Number,
            Self::Boolean(_) => CoreValueKind::Boolean,
            Self::Text(_) | Self::Choice(_) => CoreValueKind::Text,
            Self::Image(_) => CoreValueKind::Text,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreChoice {
    pub label: String,
    pub value: CoreParameterValue,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CoreParameterConstraints {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub unit: Option<String>,
    pub choices: Vec<CoreChoice>,
    pub relations: Vec<CoreRelationConstraint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreRelationOperator {
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

impl CoreRelationOperator {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LessThan => "<",
            Self::LessThanOrEqual => "<=",
            Self::GreaterThan => ">",
            Self::GreaterThanOrEqual => ">=",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreRelationOperand {
    Parameter(ParamId),
    Number(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreRelationConstraint {
    pub operator: CoreRelationOperator,
    pub left: CoreRelationOperand,
    pub right: CoreRelationOperand,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CoreProgramConstraints {
    pub relations: Vec<CoreRelationConstraint>,
    pub verify_clauses: Vec<CoreVerifyClause>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreVerifyClause {
    pub tag: CoreVerifySection,
    pub intent: Option<CoreVerifySection>,
    pub severity: Option<CoreVerifySection>,
    pub when: Option<CoreVerifySection>,
    pub metric: CoreVerifySection,
    pub expect: CoreVerifySection,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CoreVerifySection {
    pub items: Vec<CoreVerifyValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreVerifyValue {
    Symbol(String),
    Number(f64),
    Boolean(bool),
    Text(String),
    List(Vec<CoreVerifyValue>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreParameter {
    pub id: ParamId,
    pub key: String,
    pub label: String,
    pub kind: CoreParameterKind,
    pub default_value: CoreParameterValue,
    pub frozen: bool,
    pub constraints: CoreParameterConstraints,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CorePart {
    pub id: PartId,
    pub key: String,
    pub label: String,
    pub root: CoreNode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFeatureDecl {
    pub feature_id: String,
    pub role: String,
    pub param_keys: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSelectorTagKind {
    Vertex,
    Face,
    Edge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSelectorTagDecl {
    pub name: String,
    pub kind: CoreSelectorTagKind,
    pub authored_selector: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CorePreviewPartOffset {
    pub part_key: String,
    pub dx: f64,
    pub dy: f64,
    pub dz: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CorePreviewViewDecl {
    pub name: String,
    pub part_offsets: Vec<CorePreviewPartOffset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreAnalysisKind {
    LinearStatic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreAnalysisClause {
    pub id: AnalysisClauseId,
    pub kind: CoreAnalysisClauseKind,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreAnalysisScalarExpr {
    Literal { value: f64, unit: String },
    Parameter { key: String, scale: f64 },
}

impl CoreAnalysisScalarExpr {
    pub fn literal_value(&self, unit: &str) -> Option<f64> {
        match self {
            Self::Literal {
                value,
                unit: actual_unit,
            } if actual_unit == unit => Some(*value),
            _ => None,
        }
    }

    pub fn parameter_key(&self) -> Option<&str> {
        match self {
            Self::Parameter { key, scale } if *scale == 1.0 => Some(key),
            _ => None,
        }
    }

    pub fn parameter_scale(&self, expected_key: &str) -> Option<f64> {
        match self {
            Self::Parameter { key, scale } if key == expected_key => Some(*scale),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreAnalysisLocalRefinement {
    pub face_tag: String,
    pub size: CoreAnalysisScalarExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreAnalysisClauseKind {
    LinearStatic {
        part: String,
    },
    Question {
        question_id: String,
        statement: String,
        decision: String,
        acceptance_metric_ids: Vec<String>,
    },
    AcceptanceCriterion {
        metric_id: String,
        field: String,
        comparison: String,
        limit: String,
        unit: String,
        requires_convergence: bool,
    },
    Idealization {
        kind: String,
        justification: String,
        accepted_by_user: bool,
    },
    Evidence {
        evidence_id: String,
        subject: String,
        source: String,
        authority: String,
        uncertainty_percent: f64,
        decision_critical: bool,
    },
    InputEvidence {
        input_name: String,
        evidence_id: String,
    },
    Assumption {
        assumption_id: String,
        category: String,
        statement: String,
        status: String,
        evidence_ids: Vec<String>,
    },
    ValidationEvidence {
        validation_id: String,
        kind: String,
        source: String,
        result_digest: String,
    },
    Material {
        name: String,
        young_modulus: CoreAnalysisScalarExpr,
        poisson_ratio: CoreAnalysisScalarExpr,
        density: CoreAnalysisScalarExpr,
        yield_strength: CoreAnalysisScalarExpr,
    },
    VolumeMesh {
        element: String,
        size: CoreAnalysisScalarExpr,
        local_refinements: Vec<CoreAnalysisLocalRefinement>,
    },
    Refine {
        face_tag: String,
        size: CoreAnalysisScalarExpr,
    },
    TopologyControls {
        volume_fraction: CoreAnalysisScalarExpr,
        penalty: CoreAnalysisScalarExpr,
        minimum_density: CoreAnalysisScalarExpr,
        filter_radius: CoreAnalysisScalarExpr,
        move_limit: CoreAnalysisScalarExpr,
        convergence_tolerance: CoreAnalysisScalarExpr,
    },
    PassiveSolid {
        face_tag: String,
        depth: CoreAnalysisScalarExpr,
    },
    PassiveVoid {
        face_tag: String,
        depth: CoreAnalysisScalarExpr,
    },
    Fixed {
        face_tag: String,
    },
    PrescribedDisplacement {
        face_tag: String,
        displacement: [Option<CoreAnalysisScalarExpr>; 3],
    },
    SurfaceForce {
        face_tag: String,
        total: [CoreAnalysisScalarExpr; 3],
    },
    Traction {
        face_tag: String,
        vector: [CoreAnalysisScalarExpr; 3],
    },
    Pressure {
        face_tag: String,
        pressure: CoreAnalysisScalarExpr,
    },
    Solve {
        method: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreAnalysisDecl {
    pub id: AnalysisId,
    pub name: String,
    pub kind: CoreAnalysisKind,
    pub part: String,
    pub element: String,
    pub clauses: Vec<CoreAnalysisClause>,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreMetadataValue {
    Text(String),
    Symbol(String),
    Number(f64),
    Boolean(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreProgram {
    pub id: ProgramId,
    pub metadata: BTreeMap<String, CoreMetadataValue>,
    pub parameters: Vec<CoreParameter>,
    pub parts: Vec<CorePart>,
    pub analyses: Vec<CoreAnalysisDecl>,
    pub feature_decls: BTreeMap<String, CoreFeatureDecl>,
    pub selector_tags: Vec<CoreSelectorTagDecl>,
    pub preview_views: Vec<CorePreviewViewDecl>,
    pub constraints: CoreProgramConstraints,
}

impl CoreProgram {
    pub fn new(id: ProgramId, parameters: Vec<CoreParameter>, parts: Vec<CorePart>) -> Self {
        Self {
            id,
            metadata: BTreeMap::new(),
            parameters,
            parts,
            analyses: Vec::new(),
            feature_decls: BTreeMap::new(),
            selector_tags: Vec::new(),
            preview_views: Vec::new(),
            constraints: CoreProgramConstraints::default(),
        }
    }

    pub fn with_metadata(mut self, metadata: BTreeMap<String, CoreMetadataValue>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_analyses(mut self, analyses: Vec<CoreAnalysisDecl>) -> Self {
        self.analyses = analyses;
        self
    }

    pub fn with_feature_decls(mut self, feature_decls: BTreeMap<String, CoreFeatureDecl>) -> Self {
        self.feature_decls = feature_decls;
        self
    }

    pub fn with_constraints(mut self, constraints: CoreProgramConstraints) -> Self {
        self.constraints = constraints;
        self
    }

    pub fn with_selector_tags(mut self, selector_tags: Vec<CoreSelectorTagDecl>) -> Self {
        self.selector_tags = selector_tags;
        self
    }

    pub fn with_preview_views(mut self, preview_views: Vec<CorePreviewViewDecl>) -> Self {
        self.preview_views = preview_views;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreBinding {
    pub name: String,
    pub value: CoreNode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreShapeBinding {
    pub name: String,
    pub value: CoreNode,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreSelectorPayload {
    EdgeAll,
    EdgeClauses(Vec<CoreEdgeSelectorClause>),
    EdgeTag(String),
    EdgeTargetIds(Vec<String>),
    FaceClauses(Vec<CoreFaceSelectorClause>),
    FaceTag(String),
    FaceTargetIds(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreKeywordValue {
    Expr(CoreNode),
    Selector {
        source: CoreNode,
        payload: CoreSelectorPayload,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreKeywordArg {
    pub name: String,
    pub value: CoreKeywordValue,
}

impl CoreKeywordArg {
    pub fn expr(name: String, value: CoreNode) -> Self {
        Self {
            name,
            value: CoreKeywordValue::Expr(value),
        }
    }

    pub fn selector(name: String, source: CoreNode, payload: CoreSelectorPayload) -> Self {
        Self {
            name,
            value: CoreKeywordValue::Selector { source, payload },
        }
    }

    pub fn source_node(&self) -> &CoreNode {
        match &self.value {
            CoreKeywordValue::Expr(value) => value,
            CoreKeywordValue::Selector { source, .. } => source,
        }
    }

    pub fn source_node_mut(&mut self) -> &mut CoreNode {
        match &mut self.value {
            CoreKeywordValue::Expr(value) => value,
            CoreKeywordValue::Selector { source, .. } => source,
        }
    }

    pub fn selector_payload(&self) -> Option<&CoreSelectorPayload> {
        match &self.value {
            CoreKeywordValue::Expr(_) => None,
            CoreKeywordValue::Selector { payload, .. } => Some(payload),
        }
    }

    pub fn set_selector_payload(&mut self, selector: Option<CoreSelectorPayload>) {
        let source = self.source_node().clone();
        self.value = match selector {
            Some(payload) => CoreKeywordValue::Selector { source, payload },
            None => CoreKeywordValue::Expr(source),
        };
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreStlPreparationPolicy {
    pub target_triangles: CoreNode,
    pub max_error: CoreNode,
    pub preserve_boundaries: CoreNode,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreLiteral {
    Number(f64),
    Boolean(bool),
    Text(String),
    Symbol(CoreSymbol),
    Point2([f64; 2]),
    Point3([f64; 3]),
}

impl CoreLiteral {
    pub fn kind(&self) -> CoreValueKind {
        match self {
            Self::Number(_) => CoreValueKind::Number,
            Self::Boolean(_) => CoreValueKind::Boolean,
            Self::Text(_) => CoreValueKind::Text,
            Self::Symbol(_) => CoreValueKind::Any,
            Self::Point2(_) => CoreValueKind::Point2,
            Self::Point3(_) => CoreValueKind::Point3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreReference {
    Parameter(ParamId),
    Part(PartId),
    Node(NodeId),
    Local(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CorePrimitive {
    Box,
    Sphere,
    Cylinder,
    Cone,
    Torus,
    Wedge,
    Circle,
    Ellipse,
    Slot,
    SlotArc,
    Rectangle,
    RoundedRectangle,
    RoundedPolygon,
    Polygon,
    Profile,
    MakeFace,
    Text,
    Svg,
    Stl,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CoreBooleanOp {
    Union,
    Difference,
    Intersection,
    Xor,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CoreTransformOp {
    Translate,
    Rotate,
    Scale,
    Mirror,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CoreSurfaceOp {
    Extrude,
    Revolve,
    Loft,
    Sweep,
    Shell,
    Offset,
    OffsetRounded,
    Fillet,
    Chamfer,
    Taper,
    Twist,
    Draft,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CorePathOp {
    Polyline,
    BezierPath,
    Bspline,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CoreArrayOp {
    LinearArray,
    RadialArray,
    GridArray,
    ArcArray,
    Repeat,
    RepeatUnion,
    RepeatCompound,
    RepeatPick,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CoreFrameOp {
    Plane,
    Location,
    PathFrame,
    Place,
    ClipBox,
    ClipPlane,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CoreMetaOp {
    Group,
    Comment,
    Annotate,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CoreOperation {
    Primitive(CorePrimitive),
    Boolean(CoreBooleanOp),
    Transform(CoreTransformOp),
    Surface(CoreSurfaceOp),
    Path(CorePathOp),
    Array(CoreArrayOp),
    Frame(CoreFrameOp),
    Meta(CoreMetaOp),
    Custom(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreNodeKind {
    Literal(CoreLiteral),
    Reference(CoreReference),
    Build {
        bindings: Vec<CoreShapeBinding>,
        result: Box<CoreNode>,
    },
    Let {
        bindings: Vec<CoreBinding>,
        body: Box<CoreNode>,
    },
    If {
        condition: Box<CoreNode>,
        then_branch: Box<CoreNode>,
        else_branch: Box<CoreNode>,
    },
    Call {
        op: CoreOperation,
        args: Vec<CoreNode>,
        keywords: Vec<CoreKeywordArg>,
    },
    Range {
        start: Box<CoreNode>,
        end: Box<CoreNode>,
    },
    Map {
        params: Vec<String>,
        sources: Vec<CoreNode>,
        body: Box<CoreNode>,
    },
    Apply {
        op: CoreOperation,
        args: Vec<CoreNode>,
        list: Box<CoreNode>,
    },
    List(Vec<CoreNode>),
    Group(Vec<CoreNode>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreNode {
    pub id: NodeId,
    pub kind: CoreNodeKind,
    pub value_kind: CoreValueKind,
    pub span: Option<SourceSpan>,
}

impl CoreNode {
    pub fn new(id: NodeId, kind: CoreNodeKind, value_kind: CoreValueKind) -> Self {
        Self {
            id,
            kind,
            value_kind,
            span: None,
        }
    }

    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub fn stl_preparation_policy(&self) -> CoreResult<Option<CoreStlPreparationPolicy>> {
        let CoreNodeKind::Call {
            op: CoreOperation::Primitive(CorePrimitive::Stl),
            keywords,
            ..
        } = &self.kind
        else {
            return Ok(None);
        };
        signatures::normalize_stl_preparation_policy(keywords)
    }
}

/// Vertices of a regular polygon, circumradius `radius`, first vertex at
/// `rotation_deg` measured CCW from +X. Shared by every backend lowering and the
/// native planner so `regular-polygon` is geometrically identical everywhere
/// (parity by construction). Open ring — callers close it as needed.
pub fn regular_polygon_vertices(sides: u32, radius: f64, rotation_deg: f64) -> Vec<[f64; 2]> {
    let rotation = rotation_deg.to_radians();
    (0..sides)
        .map(|i| {
            let angle = rotation + std::f64::consts::TAU * (i as f64) / (sides as f64);
            [radius * angle.cos(), radius * angle.sin()]
        })
        .collect()
}

/// Vertices of a trapezoid centered at the origin: `bottom` edge at `y = -height/2`,
/// `top` edge at `y = +height/2`, each centered on x; `skew` shifts the top edge
/// along +x. Shared by every backend lowering and the native planner so `trapezoid`
/// is geometrically identical everywhere (parity by construction). CCW order:
/// bottom-left, bottom-right, top-right, top-left. Open ring — callers close it.
pub fn trapezoid_vertices(bottom: f64, top: f64, height: f64, skew: f64) -> Vec<[f64; 2]> {
    let half_h = height / 2.0;
    vec![
        [-bottom / 2.0, -half_h],
        [bottom / 2.0, -half_h],
        [top / 2.0 + skew, half_h],
        [-top / 2.0 + skew, half_h],
    ]
}

/// Decode an ISO metric coarse-pitch thread designation (e.g. `"M6"`) into the
/// parametric thread core `(radius, pitch, depth)` used by the `thread` op. The
/// crest (major) diameter is the nominal `D`; external thread depth uses the ISO
/// height of engagement `H1 = 0.6134 * P`, and the path/core radius is
/// `D/2 - depth` so the ridge crest lands at the nominal major diameter. Shared by
/// every backend so an ISO designation decodes to identical geometry. Returns
/// `None` for unknown designations.
pub fn iso_metric_thread_core(designation: &str) -> Option<(f64, f64, f64)> {
    // (designation, nominal major diameter D, coarse pitch P)
    const TABLE: &[(&str, f64, f64)] = &[
        ("M2", 2.0, 0.4),
        ("M2.5", 2.5, 0.45),
        ("M3", 3.0, 0.5),
        ("M4", 4.0, 0.7),
        ("M5", 5.0, 0.8),
        ("M6", 6.0, 1.0),
        ("M8", 8.0, 1.25),
        ("M10", 10.0, 1.5),
        ("M12", 12.0, 1.75),
        ("M16", 16.0, 2.0),
        ("M20", 20.0, 2.5),
        ("M24", 24.0, 3.0),
    ];
    let key = designation.trim().to_ascii_uppercase();
    let (_, major, pitch) = TABLE.iter().find(|(name, _, _)| *name == key)?;
    let depth = pitch * 0.6134;
    let radius = major / 2.0 - depth;
    Some((radius, *pitch, depth))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_metric_thread_core_decodes_known_and_rejects_unknown() {
        let (r, p, d) = iso_metric_thread_core("M6").expect("M6");
        assert_eq!(p, 1.0);
        assert!((d - 0.6134).abs() < 1e-9);
        assert!((r - (3.0 - 0.6134)).abs() < 1e-9);
        assert!(iso_metric_thread_core("m6").is_some(), "case-insensitive");
        assert!(
            iso_metric_thread_core("M7").is_none(),
            "unknown designation"
        );
    }

    #[test]
    fn program_clones_cleanly() {
        let node = CoreNode::new(
            NodeId::new(7),
            CoreNodeKind::Literal(CoreLiteral::Number(12.0)),
            CoreValueKind::Number,
        );
        let program = CoreProgram::new(
            ProgramId::new(1),
            vec![CoreParameter {
                id: ParamId::new(2),
                key: "width".into(),
                label: "Width".into(),
                kind: CoreParameterKind::Number,
                default_value: CoreParameterValue::Number(12.0),
                frozen: false,
                constraints: CoreParameterConstraints::default(),
            }],
            vec![CorePart {
                id: PartId::new(3),
                key: "body".into(),
                label: "Body".into(),
                root: node,
            }],
        )
        .with_constraints(CoreProgramConstraints {
            relations: vec![],
            verify_clauses: vec![CoreVerifyClause {
                tag: CoreVerifySection {
                    items: vec![CoreVerifyValue::Symbol("body".into())],
                },
                intent: None,
                severity: None,
                when: None,
                metric: CoreVerifySection {
                    items: vec![
                        CoreVerifyValue::Symbol("clearance".into()),
                        CoreVerifyValue::Number(0.2),
                    ],
                },
                expect: CoreVerifySection {
                    items: vec![CoreVerifyValue::List(vec![
                        CoreVerifyValue::Symbol(">=".into()),
                        CoreVerifyValue::Symbol("value".into()),
                        CoreVerifyValue::Number(0.2),
                    ])],
                },
            }],
        });
        let clone = program.clone();
        assert_eq!(program, clone);
    }

    #[test]
    fn compiler_error_carries_span_and_help() {
        let span = SourceSpan::new(Some(SourceFileId::new(9)), 4, 11);
        let err = CompilerError::new(CompilerErrorKind::Resolve, "unknown symbol")
            .with_span(span)
            .with_note("check spelling")
            .with_help("define the symbol first");
        assert_eq!(err.primary_span, Some(span));
        assert_eq!(err.notes, vec!["check spelling"]);
        assert_eq!(err.help.as_deref(), Some("define the symbol first"));
        assert_eq!(err.to_string(), "resolve: unknown symbol");
    }

    #[test]
    fn compiler_error_stays_small_enough_for_result_transport() {
        assert!(std::mem::size_of::<CompilerError>() < 128);
    }

    #[test]
    fn literal_and_parameter_values_report_kinds() {
        assert_eq!(
            CoreLiteral::Point3([1.0, 2.0, 3.0]).kind(),
            CoreValueKind::Point3
        );
        assert_eq!(
            CoreParameterValue::Choice("glass".into()).kind(),
            CoreValueKind::Text
        );
    }
}
