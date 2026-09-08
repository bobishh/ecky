#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepClass_FaceClassifier.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepBuilderAPI_GTransform.hxx>
#include <BRepBuilderAPI_Copy.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepBuilderAPI_Transform.hxx>
#include <BRepFilletAPI_MakeChamfer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepBndLib.hxx>
#include <Bnd_Box.hxx>
#include <BRepGProp.hxx>
#include <BRepGProp_Face.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCone.hxx>
#include <BRepPrimAPI_MakeTorus.hxx>
#include <BRepPrimAPI_MakeWedge.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepPrimAPI_MakeRevol.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRep_Tool.hxx>
#include <BRepTools.hxx>
#include <BRepTools_WireExplorer.hxx>
#include <BRep_Builder.hxx>
#include <BRepBuilderAPI_MakeSolid.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRepBuilderAPI_TransitionMode.hxx>
#include <gp_Lin.hxx>
#include <BinTools.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepLib.hxx>
#include <ShapeFix_Shape.hxx>
#include <BOPAlgo_Builder.hxx>
#include <BOPAlgo_PaveFiller.hxx>
#include <BOPAlgo_Tools.hxx>
#include <ShapeFix_Face.hxx>
#include <ShapeUpgrade_UnifySameDomain.hxx>
#include <BRepOffsetAPI_DraftAngle.hxx>
#include <BRepOffsetAPI_MakeOffset.hxx>
#include <BRepOffsetAPI_MakePipeShell.hxx>
#include <BRepOffsetAPI_MakeThickSolid.hxx>
#include <BRepOffsetAPI_ThruSections.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <GCE2d_MakeSegment.hxx>
#include <GeomAbs_JoinType.hxx>
#include <GeomAbs_Shape.hxx>
#include <Geom_BezierCurve.hxx>
#include <Geom_BSplineCurve.hxx>
#include <Geom_CylindricalSurface.hxx>
#include <Geom_ConicalSurface.hxx>
#include <Geom_Surface.hxx>
#include <GeomAPI_PointsToBSpline.hxx>
#include <GProp_GProps.hxx>
#include <gp_Pln.hxx>
#include <gp_Vec.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Poly_Triangulation.hxx>
#include <Poly_Triangle.hxx>
#include <OSD_Parallel.hxx>
#include <OSD_ThreadPool.hxx>
#include <StlAPI_Reader.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Version.hxx>
#include <StdFail_NotDone.hxx>
#include <TColgp_Array1OfPnt.hxx>
#include <TopAbs_Orientation.hxx>
#include <TopAbs_ShapeEnum.hxx>
#include <TopAbs_State.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopLoc_Location.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Iterator.hxx>
#include <TopoDS_Shape.hxx>
#include <TopoDS_Shell.hxx>
#include <TopoDS_Vertex.hxx>
#include <TopoDS_Wire.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_ListOfShape.hxx>
#include <algorithm>
#include <array>
#include <cctype>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <functional>
#include <future>
#include <iomanip>
#include <iostream>
#include <limits>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <variant>
#include <vector>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax3.hxx>
#include <gp_Circ.hxx>
#include <gp_Cylinder.hxx>
#include <gp_Lin.hxx>
#include <gp_Elips.hxx>
#include <gp_Dir.hxx>
#include <gp_GTrsf.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Pnt.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>
#include <manifold/manifold.h>
#include "execution_identity.hpp"
#include "part_mesh.hpp"
#include "vendor/yyjson/yyjson.h"

namespace fs = std::filesystem;

namespace {

struct Arg {
    enum class Kind { Number, Boolean, Text, Symbol, Point2, Point3, List, Param, Ref };

    Kind kind = Kind::Number;
    double number_value = 0.0;
    bool bool_value = false;
    std::string text_value;
    std::array<double, 2> point2_value{0.0, 0.0};
    std::array<double, 3> point3_value{0.0, 0.0, 0.0};
    std::vector<Arg> list_value;
    std::string param_value;
    std::uint64_t ref_value = 0;
};

struct Command {
    std::uint64_t output = 0;
    std::string op;
    std::vector<Arg> args;
    std::vector<struct Keyword> keywords;
};

enum class SelectorKind { Edge, Face };

enum class SelectorPayloadType { TargetIds, Clauses };

enum class SelectorClauseType { Axis, Boundary, Planar, Normal, Area };

enum class SelectorAxis { X, Y, Z };

enum class SelectorBound { Min, Max };

enum class SelectorAreaRank { Min, Max };

struct SelectorClause {
    SelectorClauseType type = SelectorClauseType::Planar;
    std::optional<SelectorAxis> axis;
    std::optional<SelectorBound> bound;
    std::optional<SelectorAreaRank> rank;
};

struct SelectorPayload {
    SelectorPayloadType type = SelectorPayloadType::TargetIds;
    SelectorKind kind = SelectorKind::Edge;
    std::vector<std::string> target_ids;
    std::vector<SelectorClause> clauses;
};

struct Keyword {
    enum class Kind { Arg, Selector };

    std::string name;
    Kind kind = Kind::Arg;
    Arg value;
    std::optional<SelectorPayload> selector_payload;
};

enum class GeometryRepresentation { AnalyticBrep, MeshDomain };

const char* geometry_representation_name(GeometryRepresentation representation) {
    return representation == GeometryRepresentation::MeshDomain
        ? "meshDomain"
        : "analyticBrep";
}

struct Part {
    struct AuthoredBinding {
        std::string name;
        std::uint64_t slot = 0;
    };

    std::string part_id;
    std::string label;
    std::uint64_t root = 0;
    GeometryRepresentation representation = GeometryRepresentation::AnalyticBrep;
    std::vector<Command> commands;
    std::vector<AuthoredBinding> authored_bindings;
};

struct PartialBooleanGroupPlan {
    std::string part_key;
    std::uint64_t parent_output = 0;
    std::string key;
    std::string operation;
    std::vector<std::uint32_t> input_indices;
    std::uint32_t ordinal = 0;
    std::uint32_t version = 0;
    GeometryRepresentation representation = GeometryRepresentation::AnalyticBrep;
};

enum class ParallelPolicy { OuterOnly, Adaptive };

const char* parallel_policy_name(ParallelPolicy policy) {
    return policy == ParallelPolicy::Adaptive ? "adaptive" : "outer-only";
}

struct Plan {
    std::uint32_t schema_version = 0;
    std::string plan_id;
    std::vector<Part> parts;
    std::vector<PartialBooleanGroupPlan> partial_boolean_groups;
};

struct ShapeRecord {
    enum class Kind { Shape, Manifold };

    std::string part_id;
    std::string label;
    Kind kind = Kind::Shape;
    TopoDS_Shape shape;
    manifold::Manifold manifold;
    std::map<std::string, TopoDS_Shape> authored_bindings;
};

struct SlotValue {
    enum class Kind { Shape, Frame, Manifold };

    Kind kind = Kind::Shape;
    TopoDS_Shape shape;
    gp_Trsf frame;
    manifold::Manifold manifold;

    SlotValue() = default;

    SlotValue(const TopoDS_Shape& value) : kind(Kind::Shape), shape(value) {}

    SlotValue(const gp_Trsf& value) : kind(Kind::Frame), frame(value) {}

    static SlotValue shape_value(const TopoDS_Shape& value) {
        SlotValue slot;
        slot.kind = Kind::Shape;
        slot.shape = value;
        return slot;
    }

    static SlotValue frame_value(const gp_Trsf& value) {
        SlotValue slot;
        slot.kind = Kind::Frame;
        slot.frame = value;
        return slot;
    }

    static SlotValue manifold_value(const manifold::Manifold& value) {
        SlotValue slot;
        slot.kind = Kind::Manifold;
        slot.manifold = value;
        return slot;
    }
};

struct ParseError : std::runtime_error {
    using std::runtime_error::runtime_error;
};

struct SchemaError : std::runtime_error {
    using std::runtime_error::runtime_error;
};

struct EvalError : std::runtime_error {
    using std::runtime_error::runtime_error;
};

struct OcctRuntimeError : std::runtime_error {
    using std::runtime_error::runtime_error;
};

struct IoError : std::runtime_error {
    using std::runtime_error::runtime_error;
};

struct CommandCacheEvidence {
    std::string command_id;
    bool cache_hit = false;
    bool cache_admitted = false;
};
struct CommandTimingEvidence {
    std::string command_id;
    std::string op;
    std::uint64_t elapsed_ms = 0;
};
struct PartialBooleanGroupEvidence {
    std::string part_id;
    std::uint64_t parent_output = 0;
    std::string key;
    bool cache_hit = false;
    std::uint32_t recompute_count = 0;
};
struct PartMeshEvidence {
    std::string identity;
    std::uint64_t facet_count = 0;
};
std::string quote_json_string(const std::string& value);

// Stable diagnostics contract. Each stage appears in this fixed order, even
// when the plan never exercises it, so callers can distinguish skipped work
// from a missing report entry.
const std::array<const char*, 8> kStageReportNames = {
    "import", "validate", "solidify", "boolean", "cleanup", "mesh", "verify", "export"
};
// One render owns all mutable diagnostics and cache/mesh evidence. Workers
// receive this context explicitly; the mutex only protects aggregate evidence.
struct ExecutionContext {
    mutable std::mutex mutex;
    std::string current_stage = "startup";
    std::uint32_t worker_budget = 1;
    std::uint32_t mesh_outer_worker_budget = 1;
    std::uint32_t mesh_pool_budget = 1;
    std::uint32_t mesh_launcher_budget = 1;
    std::uint32_t peak_dag_concurrency = 0;
    std::uint32_t active_dag_nodes = 0;
    ParallelPolicy parallel_policy = ParallelPolicy::Adaptive;
    std::uint32_t nested_kernel_units_in_use = 0;
    std::uint32_t serial_boolean_count = 0;
    std::uint32_t parallel_boolean_count = 0;
    std::uint32_t mesh_boolean_count = 0;
    std::uint32_t tessellated_step_part_count = 0;
    std::uint32_t max_nested_kernel_lease = 0;
    std::uint32_t peak_total_allocated_cpu_units = 0;
    std::map<std::string, std::uint32_t> part_executed_commands;
    std::map<std::string, bool> part_cache_hits;
    std::map<std::string, GeometryRepresentation> part_representations;
    std::vector<CommandCacheEvidence> command_cache_evidence;
    std::vector<CommandTimingEvidence> command_timing_evidence;
    std::vector<PartialBooleanGroupEvidence> partial_boolean_group_evidence;
    std::map<std::string, std::vector<std::string>> part_executed_command_ids;
    std::map<std::string, PartMeshEvidence> part_mesh_evidence;
    std::uint32_t mesh_build_count = 0;
    std::uint32_t mesh_cache_hit_count = 0;
    std::uint64_t preview_facet_count = 0;
    std::uint64_t released_slot_count = 0;
    std::uint64_t cache_read_count = 0;
    std::uint64_t cache_write_count = 0;
    std::uint64_t cache_rejection_count = 0;
    std::uint64_t partial_boolean_cache_hit_count = 0;
    std::uint64_t partial_boolean_cache_miss_count = 0;
    std::uint64_t partial_boolean_cache_write_count = 0;
    std::uint64_t topology_cache_hit_count = 0;
    std::uint64_t topology_cache_miss_count = 0;
    std::uint64_t topology_cache_write_count = 0;
    std::uint64_t four_way_intersection_count = 0;
    std::map<std::string, std::uint32_t> stage_execution_counts;
    std::map<std::string, std::chrono::steady_clock::duration> stage_elapsed;
    std::chrono::steady_clock::time_point report_started_at = std::chrono::steady_clock::now();
    std::string runner_binary_digest;

    void set_stage(std::string stage) {
        std::lock_guard<std::mutex> lock(mutex);
        current_stage = std::move(stage);
    }

    std::string stage() const {
        std::lock_guard<std::mutex> lock(mutex);
        return current_stage;
    }
};

class BooleanParallelLease {
public:
    explicit BooleanParallelLease(ExecutionContext& context) : context_(context) {
        {
            std::lock_guard<std::mutex> lock(context_.mutex);
            const std::uint32_t outer = std::max(1u, context_.active_dag_nodes);
            const std::uint32_t available = context_.worker_budget >
                    outer + context_.nested_kernel_units_in_use
                ? context_.worker_budget - outer - context_.nested_kernel_units_in_use
                : 0;
            const std::uint32_t fair_share = outer > 0
                ? (context_.worker_budget - std::min(context_.worker_budget, outer)) / outer
                : 0;
            if (context_.parallel_policy == ParallelPolicy::Adaptive &&
                available > 0 && fair_share > 0) {
                // OCCT's Boolean filler scales negatively beyond six launch
                // threads on the reference Apple host. Lease only the proven
                // useful width; leave the remaining process budget idle or
                // available to independent outer DAG work.
                nested_units_ = std::min({available, fair_share, 5u});
                context_.nested_kernel_units_in_use += nested_units_;
                context_.max_nested_kernel_lease = std::max(
                    context_.max_nested_kernel_lease, nested_units_);
                ++context_.parallel_boolean_count;
            } else {
                ++context_.serial_boolean_count;
            }
            context_.peak_total_allocated_cpu_units = std::max(
                context_.peak_total_allocated_cpu_units,
                outer + context_.nested_kernel_units_in_use);
            if (context_.peak_total_allocated_cpu_units > context_.worker_budget) {
                throw EvalError("Direct OCCT CPU lease exceeded worker budget");
            }
        }
        if (nested_units_ > 0) {
            try {
                OSD_Parallel::SetUseOcctThreads(Standard_True);
                if (!OSD_Parallel::ToUseOcctThreads()) {
                    throw EvalError(
                        "Direct OCCT runtime cannot enable shared Boolean thread pool");
                }
                const std::uint32_t launch_units = nested_units_ + 1;
                const Handle(OSD_ThreadPool)& pool =
                    OSD_ThreadPool::DefaultPool(static_cast<int>(launch_units));
                pool->SetNbDefaultThreadsToLaunch(static_cast<int>(launch_units));
            } catch (...) {
                std::lock_guard<std::mutex> lock(context_.mutex);
                context_.nested_kernel_units_in_use -= nested_units_;
                nested_units_ = 0;
                throw;
            }
        }
    }

    ~BooleanParallelLease() {
        std::lock_guard<std::mutex> lock(context_.mutex);
        context_.nested_kernel_units_in_use -= nested_units_;
    }

    bool runs_parallel() const { return nested_units_ > 0; }
    std::uint32_t nested_units() const { return nested_units_; }

private:
    ExecutionContext& context_;
    std::uint32_t nested_units_ = 0;
};

class StageExecutionTimer {
public:
    StageExecutionTimer(ExecutionContext& context, const char* name)
        : context_(context), name_(name), started_at_(std::chrono::steady_clock::now()) {
        std::lock_guard<std::mutex> lock(context_.mutex);
        ++context_.stage_execution_counts[name_];
    }

    ~StageExecutionTimer() {
        std::lock_guard<std::mutex> lock(context_.mutex);
        context_.stage_elapsed[name_] += std::chrono::steady_clock::now() - started_at_;
    }

private:
    ExecutionContext& context_;
    std::string name_;
    std::chrono::steady_clock::time_point started_at_;
};

void write_stage_report(const fs::path& path, ExecutionContext& context) {
    std::ofstream out(path);
    if (!out) {
        throw IoError("failed to open stage report file");
    }
    const auto total_elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::steady_clock::now() - context.report_started_at).count();
    std::lock_guard<std::mutex> lock(context.mutex);
    out << "{\"schemaVersion\":1,\"totalElapsedMs\":" << total_elapsed
        << ",\"workerBudget\":" << context.worker_budget
        << ",\"parallelPolicy\":" << quote_json_string(parallel_policy_name(context.parallel_policy))
        << ",\"serialBooleanCount\":" << context.serial_boolean_count
        << ",\"parallelBooleanCount\":" << context.parallel_boolean_count
        << ",\"meshBooleanCount\":" << context.mesh_boolean_count
        << ",\"tessellatedStepPartCount\":" << context.tessellated_step_part_count
        << ",\"maxNestedKernelLease\":" << context.max_nested_kernel_lease
        << ",\"peakTotalAllocatedCpuUnits\":" << context.peak_total_allocated_cpu_units
        << ",\"meshOuterWorkerBudget\":" << context.mesh_outer_worker_budget
        << ",\"meshPoolBudget\":" << context.mesh_pool_budget
        << ",\"meshLauncherBudget\":" << context.mesh_launcher_budget
        << ",\"peakDagConcurrency\":" << context.peak_dag_concurrency
        << ",\"meshBuildCount\":" << context.mesh_build_count
        << ",\"meshCacheHitCount\":" << context.mesh_cache_hit_count
        << ",\"releasedSlotCount\":" << context.released_slot_count
        << ",\"cacheReadCount\":" << context.cache_read_count
        << ",\"cacheWriteCount\":" << context.cache_write_count
        << ",\"cacheRejectionCount\":" << context.cache_rejection_count
        << ",\"partialBooleanCacheHitCount\":" << context.partial_boolean_cache_hit_count
        << ",\"partialBooleanCacheMissCount\":" << context.partial_boolean_cache_miss_count
        << ",\"partialBooleanCacheWriteCount\":" << context.partial_boolean_cache_write_count
        << ",\"topologyCacheHitCount\":" << context.topology_cache_hit_count
        << ",\"topologyCacheMissCount\":" << context.topology_cache_miss_count
        << ",\"topologyCacheWriteCount\":" << context.topology_cache_write_count
        << ",\"fourWayIntersectionCount\":" << context.four_way_intersection_count
        << ",\"previewFacetCount\":" << context.preview_facet_count
        << ",\"parts\":[";
    bool first_part = true;
    std::set<std::string> reported_parts;
    for (const auto& [part_id, count] : context.part_executed_commands) {
        reported_parts.insert(part_id);
        if (!first_part) out << ",";
        first_part = false;
        out << "{\"partId\":" << quote_json_string(part_id)
            << ",\"cacheHit\":" << (context.part_cache_hits[part_id] ? "true" : "false")
            << ",\"representation\":" << quote_json_string(
                geometry_representation_name(context.part_representations.at(part_id)))
            << ",\"executedCommandCount\":" << count;
        if (const auto mesh = context.part_mesh_evidence.find(part_id);
            mesh != context.part_mesh_evidence.end()) {
            out << ",\"meshIdentity\":" << quote_json_string(mesh->second.identity)
                << ",\"meshFacetCount\":" << mesh->second.facet_count;
        }
        out << ",\"executedCommandIds\":[";
        const auto& ids = context.part_executed_command_ids[part_id];
        for (std::size_t index = 0; index < ids.size(); ++index) {
            if (index != 0) out << ',';
            out << quote_json_string(ids[index]);
        }
        out << "]}";
    }
    for (const auto& [part_id, cache_hit] : context.part_cache_hits) {
        if (reported_parts.find(part_id) != reported_parts.end()) continue;
        if (!first_part) out << ",";
        first_part = false;
        out << "{\"partId\":" << quote_json_string(part_id)
            << ",\"cacheHit\":" << (cache_hit ? "true" : "false")
            << ",\"representation\":" << quote_json_string(
                geometry_representation_name(context.part_representations.at(part_id)))
            << ",\"executedCommandCount\":0";
        if (const auto mesh = context.part_mesh_evidence.find(part_id);
            mesh != context.part_mesh_evidence.end()) {
            out << ",\"meshIdentity\":" << quote_json_string(mesh->second.identity)
                << ",\"meshFacetCount\":" << mesh->second.facet_count;
        }
        out << ",\"executedCommandIds\":[]}";
    }
    out << "],\"commands\":[";
    for (std::size_t index = 0; index < context.command_cache_evidence.size(); ++index) {
        if (index != 0) out << ',';
        const CommandCacheEvidence& evidence = context.command_cache_evidence[index];
        out << "{\"commandId\":" << quote_json_string(evidence.command_id)
            << ",\"cacheAdmitted\":" << (evidence.cache_admitted ? "true" : "false")
            << ",\"cacheHit\":" << (evidence.cache_hit ? "true" : "false") << "}";
    }
    out << "],\"commandTimings\":[";
    for (std::size_t index = 0; index < context.command_timing_evidence.size(); ++index) {
        if (index != 0) out << ',';
        const CommandTimingEvidence& evidence = context.command_timing_evidence[index];
        out << "{\"commandId\":" << quote_json_string(evidence.command_id)
            << ",\"op\":" << quote_json_string(evidence.op)
            << ",\"elapsedMs\":" << evidence.elapsed_ms << "}";
    }
    out << "],\"partialBooleanGroups\":[";
    for (std::size_t index = 0; index < context.partial_boolean_group_evidence.size(); ++index) {
        if (index != 0) out << ',';
        const PartialBooleanGroupEvidence& evidence =
            context.partial_boolean_group_evidence[index];
        out << "{\"partId\":" << quote_json_string(evidence.part_id)
            << ",\"parentOutput\":" << evidence.parent_output
            << ",\"key\":" << quote_json_string(evidence.key)
            << ",\"cacheHit\":" << (evidence.cache_hit ? "true" : "false")
            << ",\"recomputeCount\":" << evidence.recompute_count << "}";
    }
    out << "],\"stages\":[";
    for (std::size_t index = 0; index < kStageReportNames.size(); ++index) {
        if (index != 0) {
            out << ",";
        }
        const std::string name = kStageReportNames[index];
        const std::uint32_t execution_count = context.stage_execution_counts[name];
        const auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
            context.stage_elapsed[name]).count();
        out << "{\"name\":" << quote_json_string(name)
            << ",\"status\":" << quote_json_string(execution_count == 0 ? "skipped" : "executed")
            << ",\"executionCount\":" << execution_count
            << ",\"elapsedMs\":" << elapsed << "}";
    }
    out << "]}";
    if (!out.good()) {
        throw IoError("failed to write stage report file");
    }
}

yyjson_val* json_require(yyjson_val* value, const char* key) {
    if (!value || !yyjson_is_obj(value)) {
        throw SchemaError("expected object while reading `" + std::string(key) + "`");
    }
    yyjson_val* found = yyjson_obj_get(value, key);
    if (found == nullptr) {
        throw SchemaError("missing field `" + std::string(key) + "`");
    }
    return found;
}

std::string json_string(yyjson_val* value, const std::string& label) {
    if (!value || !yyjson_is_str(value)) {
        throw SchemaError("expected string for `" + label + "`");
    }
    const char* text = yyjson_get_str(value);
    return text ? text : "";
}

GeometryRepresentation parse_geometry_representation(
    yyjson_val* value,
    const std::string& field
) {
    const std::string raw = json_string(value, field);
    if (raw == "analyticBrep") return GeometryRepresentation::AnalyticBrep;
    if (raw == "meshDomain") return GeometryRepresentation::MeshDomain;
    throw SchemaError(field + " must be `analyticBrep` or `meshDomain`");
}

double json_number(yyjson_val* value, const std::string& label) {
    if (!value || !yyjson_is_num(value)) {
        throw SchemaError("expected number for `" + label + "`");
    }
    return yyjson_get_num(value);
}

bool json_bool(yyjson_val* value, const std::string& label) {
    if (!value || !yyjson_is_bool(value)) {
        throw SchemaError("expected boolean for `" + label + "`");
    }
    return yyjson_get_bool(value);
}

yyjson_val* json_array(yyjson_val* value, const std::string& label) {
    if (!value || !yyjson_is_arr(value)) {
        throw SchemaError("expected array for `" + label + "`");
    }
    return value;
}

Arg parse_arg(yyjson_val* value);

SelectorAxis parse_selector_axis(const std::string& axis) {
    if (axis == "x") {
        return SelectorAxis::X;
    }
    if (axis == "y") {
        return SelectorAxis::Y;
    }
    if (axis == "z") {
        return SelectorAxis::Z;
    }
    throw SchemaError("unsupported selector axis `" + axis + "`");
}

SelectorBound parse_selector_bound(const std::string& bound) {
    if (bound == "min") {
        return SelectorBound::Min;
    }
    if (bound == "max") {
        return SelectorBound::Max;
    }
    throw SchemaError("unsupported selector bound `" + bound + "`");
}

SelectorAreaRank parse_selector_area_rank(const std::string& rank) {
    if (rank == "min") {
        return SelectorAreaRank::Min;
    }
    if (rank == "max") {
        return SelectorAreaRank::Max;
    }
    throw SchemaError("unsupported selector area rank `" + rank + "`");
}

SelectorKind parse_selector_kind(const std::string& kind) {
    if (kind == "edge") {
        return SelectorKind::Edge;
    }
    if (kind == "face") {
        return SelectorKind::Face;
    }
    throw SchemaError("unsupported selector kind `" + kind + "`");
}

SelectorClause parse_selector_clause(yyjson_val* value) {
    if (!value || !yyjson_is_obj(value)) {
        throw SchemaError("expected object for `selector clause`");
    }
    SelectorClause clause;
    const std::string type = json_string(json_require(value, "type"), "selector clause type");
    if (type == "axis") {
        clause.type = SelectorClauseType::Axis;
        clause.axis = parse_selector_axis(json_string(json_require(value, "axis"), "selector axis"));
        return clause;
    }
    if (type == "boundary") {
        clause.type = SelectorClauseType::Boundary;
        clause.axis = parse_selector_axis(json_string(json_require(value, "axis"), "selector axis"));
        clause.bound = parse_selector_bound(json_string(json_require(value, "bound"), "selector bound"));
        return clause;
    }
    if (type == "planar") {
        clause.type = SelectorClauseType::Planar;
        return clause;
    }
    if (type == "normal") {
        clause.type = SelectorClauseType::Normal;
        clause.axis = parse_selector_axis(json_string(json_require(value, "axis"), "selector axis"));
        return clause;
    }
    if (type == "area") {
        clause.type = SelectorClauseType::Area;
        clause.rank = parse_selector_area_rank(json_string(json_require(value, "rank"), "selector rank"));
        return clause;
    }
    throw SchemaError("unsupported selector clause type `" + type + "`");
}

SelectorPayload parse_selector_payload(yyjson_val* value) {
    if (!value || !yyjson_is_obj(value)) {
        throw SchemaError("expected object for `selector payload`");
    }
    SelectorPayload payload;
    payload.type = [&]() {
        const std::string type = json_string(json_require(value, "type"), "selector payload type");
        if (type == "targetIds") {
            return SelectorPayloadType::TargetIds;
        }
        if (type == "clauses") {
            return SelectorPayloadType::Clauses;
        }
        throw SchemaError("unsupported selector payload type `" + type + "`");
    }();
    payload.kind =
        parse_selector_kind(json_string(json_require(value, "kind"), "selector payload kind"));
    if (payload.type == SelectorPayloadType::TargetIds) {
        yyjson_val* target_ids = json_array(json_require(value, "targetIds"), "targetIds");
        size_t index;
        size_t max;
        yyjson_val* item;
        yyjson_arr_foreach(target_ids, index, max, item) {
            payload.target_ids.push_back(json_string(item, "targetId"));
        }
        return payload;
    }
    yyjson_val* clauses = json_array(json_require(value, "clauses"), "clauses");
    size_t index;
    size_t max;
    yyjson_val* item;
    yyjson_arr_foreach(clauses, index, max, item) {
        payload.clauses.push_back(parse_selector_clause(item));
    }
    return payload;
}

Keyword parse_keyword(yyjson_val* value) {
    if (!value || !yyjson_is_obj(value)) {
        throw SchemaError("expected object for `keyword`");
    }
    Keyword keyword;
    keyword.name = json_string(json_require(value, "name"), "keyword name");
    const std::string kind = json_string(json_require(value, "kind"), "keyword kind");
    if (kind == "arg") {
        keyword.kind = Keyword::Kind::Arg;
        keyword.value = parse_arg(json_require(value, "value"));
        return keyword;
    }
    if (kind == "selector") {
        keyword.kind = Keyword::Kind::Selector;
        keyword.value = parse_arg(json_require(value, "value"));
        keyword.selector_payload = parse_selector_payload(json_require(value, "payload"));
        return keyword;
    }
    throw SchemaError("unsupported keyword kind `" + kind + "`");
}

std::array<double, 2> parse_point2(yyjson_val* value) {
    yyjson_val* items = json_array(value, "point2");
    if (yyjson_arr_size(items) != 2) {
        throw SchemaError("expected 2 values for point2");
    }
    return {
        json_number(yyjson_arr_get(items, 0), "point2[0]"),
        json_number(yyjson_arr_get(items, 1), "point2[1]"),
    };
}

std::array<double, 3> parse_point3(yyjson_val* value) {
    yyjson_val* items = json_array(value, "point3");
    if (yyjson_arr_size(items) != 3) {
        throw SchemaError("expected 3 values for point3");
    }
    return {
        json_number(yyjson_arr_get(items, 0), "point3[0]"),
        json_number(yyjson_arr_get(items, 1), "point3[1]"),
        json_number(yyjson_arr_get(items, 2), "point3[2]"),
    };
}

Arg parse_arg(yyjson_val* value) {
    if (!value || !yyjson_is_obj(value)) {
        throw SchemaError("expected object for `arg`");
    }
    const std::string kind = json_string(json_require(value, "kind"), "kind");
    yyjson_val* raw_value = json_require(value, "value");

    Arg arg;
    if (kind == "number") {
        arg.kind = Arg::Kind::Number;
        arg.number_value = json_number(raw_value, "value");
        return arg;
    }
    if (kind == "boolean") {
        arg.kind = Arg::Kind::Boolean;
        arg.bool_value = json_bool(raw_value, "value");
        return arg;
    }
    if (kind == "text") {
        arg.kind = Arg::Kind::Text;
        arg.text_value = json_string(raw_value, "value");
        return arg;
    }
    if (kind == "symbol") {
        arg.kind = Arg::Kind::Symbol;
        arg.text_value = json_string(raw_value, "value");
        return arg;
    }
    if (kind == "point2") {
        arg.kind = Arg::Kind::Point2;
        arg.point2_value = parse_point2(raw_value);
        return arg;
    }
    if (kind == "point3") {
        arg.kind = Arg::Kind::Point3;
        arg.point3_value = parse_point3(raw_value);
        return arg;
    }
    if (kind == "list") {
        arg.kind = Arg::Kind::List;
        yyjson_val* items = json_array(raw_value, "value");
        size_t index;
        size_t max;
        yyjson_val* item;
        yyjson_arr_foreach(items, index, max, item) {
            arg.list_value.push_back(parse_arg(item));
        }
        return arg;
    }
    if (kind == "param") {
        throw SchemaError("runner plan requires resolved args; `param` values are not allowed");
    }
    if (kind == "ref") {
        arg.kind = Arg::Kind::Ref;
        arg.ref_value = static_cast<std::uint64_t>(json_number(raw_value, "value"));
        return arg;
    }
    throw SchemaError("unsupported arg kind `" + kind + "`");
}

Command parse_command(yyjson_val* value) {
    if (!value || !yyjson_is_obj(value)) {
        throw SchemaError("expected object for `command`");
    }
    Command command;
    command.output =
        static_cast<std::uint64_t>(json_number(json_require(value, "output"), "output"));
    command.op = json_string(json_require(value, "op"), "op");
    yyjson_val* args = json_array(json_require(value, "args"), "args");
    size_t arg_index;
    size_t arg_max;
    yyjson_val* arg;
    yyjson_arr_foreach(args, arg_index, arg_max, arg) {
        command.args.push_back(parse_arg(arg));
    }
    yyjson_val* keywords = json_array(json_require(value, "keywords"), "keywords");
    size_t keyword_index;
    size_t keyword_max;
    yyjson_val* keyword;
    yyjson_arr_foreach(keywords, keyword_index, keyword_max, keyword) {
        command.keywords.push_back(parse_keyword(keyword));
    }
    return command;
}

Part parse_part(yyjson_val* value) {
    if (!value || !yyjson_is_obj(value)) {
        throw SchemaError("expected object for `part`");
    }
    Part part;
    part.part_id = json_string(json_require(value, "key"), "key");
    part.label = json_string(json_require(value, "label"), "label");
    part.root = static_cast<std::uint64_t>(json_number(json_require(value, "root"), "root"));
    if (yyjson_val* representation = yyjson_obj_get(value, "representation")) {
        part.representation = parse_geometry_representation(
            representation, "part.representation");
    }
    yyjson_val* commands = json_array(json_require(value, "commands"), "commands");
    size_t command_index;
    size_t command_max;
    yyjson_val* command;
    yyjson_arr_foreach(commands, command_index, command_max, command) {
        part.commands.push_back(parse_command(command));
    }
    if (yyjson_val* authored_bindings = yyjson_obj_get(value, "authoredBindings")) {
        authored_bindings = json_array(authored_bindings, "part.authoredBindings");
        std::set<std::string> names;
        size_t binding_index;
        size_t binding_max;
        yyjson_val* binding;
        yyjson_arr_foreach(authored_bindings, binding_index, binding_max, binding) {
            if (!yyjson_is_obj(binding)) {
                throw SchemaError("part.authoredBindings entry must be an object");
            }
            Part::AuthoredBinding parsed;
            parsed.name = json_string(
                json_require(binding, "name"), "part.authoredBindings.name");
            parsed.slot = static_cast<std::uint64_t>(json_number(
                json_require(binding, "slot"), "part.authoredBindings.slot"));
            if (parsed.name.empty() || !names.insert(parsed.name).second) {
                throw SchemaError("part.authoredBindings names must be unique and non-empty");
            }
            part.authored_bindings.push_back(std::move(parsed));
        }
    }
    return part;
}

PartialBooleanGroupPlan parse_partial_boolean_group(yyjson_val* value) {
    PartialBooleanGroupPlan group;
    group.part_key = json_string(json_require(value, "partKey"), "partialBooleanGroup.partKey");
    group.parent_output = static_cast<std::uint64_t>(
        json_number(json_require(value, "parentOutput"), "partialBooleanGroup.parentOutput"));
    group.key = json_string(json_require(value, "key"), "partialBooleanGroup.key");
    if (yyjson_val* representation = yyjson_obj_get(value, "representation")) {
        group.representation = parse_geometry_representation(
            representation, "partialBooleanGroup.representation");
    }
    group.operation = json_string(json_require(value, "operation"), "partialBooleanGroup.operation");
    group.ordinal = static_cast<std::uint32_t>(
        json_number(json_require(value, "ordinal"), "partialBooleanGroup.ordinal"));
    group.version = static_cast<std::uint32_t>(
        json_number(json_require(value, "version"), "partialBooleanGroup.version"));
    yyjson_val* indices = json_array(json_require(value, "inputIndices"), "partialBooleanGroup.inputIndices");
    size_t index;
    size_t max;
    yyjson_val* item;
    yyjson_arr_foreach(indices, index, max, item) {
        group.input_indices.push_back(static_cast<std::uint32_t>(json_number(item, "partialBooleanGroup.inputIndex")));
    }
    if (group.key.empty() || group.operation != "union" || group.version != 2 ||
        group.input_indices.size() != 2 || group.input_indices[0] == group.input_indices[1]) {
        throw SchemaError("unsupported partialBooleanGroup");
    }
    return group;
}

Plan parse_plan(yyjson_val* value) {
    if (!value || !yyjson_is_obj(value)) {
        throw SchemaError("expected root plan object");
    }
    Plan plan;
    plan.schema_version =
        static_cast<std::uint32_t>(json_number(json_require(value, "schemaVersion"), "schemaVersion"));
    plan.plan_id = json_string(json_require(value, "planId"), "planId");
    if (plan.schema_version != 1) {
        throw SchemaError("unsupported plan schema version");
    }
    yyjson_val* parts = json_array(json_require(value, "parts"), "parts");
    size_t part_index;
    size_t part_max;
    yyjson_val* part;
    yyjson_arr_foreach(parts, part_index, part_max, part) {
        plan.parts.push_back(parse_part(part));
    }
    if (yyjson_val* groups = yyjson_obj_get(value, "partialBooleanGroups")) {
        groups = json_array(groups, "partialBooleanGroups");
        size_t group_index;
        size_t group_max;
        yyjson_val* group;
        yyjson_arr_foreach(groups, group_index, group_max, group) {
            plan.partial_boolean_groups.push_back(parse_partial_boolean_group(group));
        }
    }
    std::map<std::pair<std::string, std::uint64_t>, std::vector<const PartialBooleanGroupPlan*>>
        groups_by_parent;
    for (const PartialBooleanGroupPlan& group : plan.partial_boolean_groups) {
        groups_by_parent[{group.part_key, group.parent_output}].push_back(&group);
    }
    for (const auto& [parent, groups] : groups_by_parent) {
        const std::string parent_part = parent.first;
        const std::uint64_t parent_output = parent.second;
        const auto part = std::find_if(plan.parts.begin(), plan.parts.end(), [&](const Part& item) {
            return item.part_id == parent_part;
        });
        if (part == plan.parts.end()) throw SchemaError("partialBooleanGroup part does not exist");
        const auto command = std::find_if(
            part->commands.begin(), part->commands.end(), [&](const Command& item) {
                return item.output == parent_output;
            });
        if (command == part->commands.end() || command->op != "union" ||
            command->args.size() != 4 || groups.size() != 2) {
            throw SchemaError("partialBooleanGroups require one four-input union and two groups");
        }
        std::set<std::uint32_t> ordinals;
        std::set<std::uint32_t> covered_indices;
        std::set<std::string> keys;
        for (const PartialBooleanGroupPlan* group : groups) {
            ordinals.insert(group->ordinal);
            keys.insert(group->key);
            for (std::uint32_t index : group->input_indices) covered_indices.insert(index);
        }
        if (ordinals != std::set<std::uint32_t>{0, 1} ||
            covered_indices != std::set<std::uint32_t>{0, 1, 2, 3} || keys.size() != 2) {
            throw SchemaError(
                "partialBooleanGroups must have unique keys and exactly cover inputs 0..3");
        }
        const bool mesh_group = std::any_of(
            groups.begin(), groups.end(), [](const PartialBooleanGroupPlan* group) {
                return group->representation == GeometryRepresentation::MeshDomain;
            });
        if (mesh_group !=
            (part->representation == GeometryRepresentation::MeshDomain)) {
            throw SchemaError(
                "partialBooleanGroup representation conflicts with parent part");
        }
    }
    return plan;
}

std::string quote_json_string(const std::string& value) {
    std::ostringstream out;
    out << '"';
    for (char ch : value) {
        switch (ch) {
            case '\\': out << "\\\\"; break;
            case '"': out << "\\\""; break;
            case '\n': out << "\\n"; break;
            case '\r': out << "\\r"; break;
            case '\t': out << "\\t"; break;
            default: out << ch; break;
        }
    }
    out << '"';
    return out.str();
}

void write_json_number(std::ostream& out, double value) {
    if (!std::isfinite(value)) {
        out << 0;
        return;
    }
    out << std::setprecision(17) << value;
}

std::string format_coordinate(double value) {
    if (!std::isfinite(value) || std::abs(value) < 0.0005) {
        return "0";
    }
    std::ostringstream out;
    out << std::fixed << std::setprecision(3) << value;
    std::string text = out.str();
    while (!text.empty() && text.back() == '0') {
        text.pop_back();
    }
    if (!text.empty() && text.back() == '.') {
        text.pop_back();
    }
    if (text.empty() || text == "-0") {
        return "0";
    }
    return text;
}

std::string point_signature(const gp_Pnt& point) {
    return format_coordinate(point.X()) + "-" + format_coordinate(point.Y()) + "-" +
           format_coordinate(point.Z());
}

std::string edge_signature(const gp_Pnt& start, const gp_Pnt& end) {
    std::string first = point_signature(start);
    std::string second = point_signature(end);
    if (second < first) {
        std::swap(first, second);
    }
    return first + "_" + second;
}

std::string edge_target_id(const std::string& part_id, int edge_index, const TopoDS_Edge& edge) {
    try {
        BRepAdaptor_Curve curve(edge);
        double first_param = curve.FirstParameter();
        double last_param = curve.LastParameter();
        if (std::isfinite(first_param) && std::isfinite(last_param)) {
            gp_Pnt start = curve.Value(first_param);
            gp_Pnt end = curve.Value(last_param);
            return part_id + ":edge:" + std::to_string(edge_index) + ":" +
                   edge_signature(start, end);
        }
    } catch (...) {
    }
    return part_id + ":edge:" + std::to_string(edge_index);
}

std::string vertex_target_id(
    const std::string& part_id,
    int vertex_index,
    const TopoDS_Vertex& vertex
) {
    try {
        return part_id + ":vertex:" + std::to_string(vertex_index) + ":" +
               point_signature(BRep_Tool::Pnt(vertex));
    } catch (...) {
    }
    return part_id + ":vertex:" + std::to_string(vertex_index);
}

std::string face_target_id(const std::string& part_id, int face_index, const TopoDS_Face& face) {
    try {
        GProp_GProps props;
        BRepGProp::SurfaceProperties(face, props);
        gp_Pnt center = props.CentreOfMass();
        double area = props.Mass();
        return part_id + ":face:" + std::to_string(face_index) + ":" +
               point_signature(center) + ":" + format_coordinate(area);
    } catch (...) {
    }
    return part_id + ":face:" + std::to_string(face_index);
}

std::string stable_target_suffix(const std::string& payload) {
    std::size_t first_colon = payload.find(':');
    if (first_colon == std::string::npos) {
        return payload;
    }
    bool numeric_prefix = first_colon > 0 &&
        std::all_of(payload.begin(), payload.begin() + static_cast<long>(first_colon), [](char ch) {
            return ch >= '0' && ch <= '9';
        });
    if (!numeric_prefix) {
        return payload;
    }
    return payload.substr(first_colon + 1);
}

std::string stable_edge_target_id(const std::string& target_id) {
    const std::string marker = ":edge:";
    std::size_t marker_pos = target_id.find(marker);
    if (marker_pos == std::string::npos) {
        return target_id;
    }
    std::string prefix = target_id.substr(0, marker_pos);
    std::size_t node_marker_pos = prefix.find(":node:");
    std::size_t stable_node_marker_pos = prefix.find(":stable-node-key:");
    if (node_marker_pos != std::string::npos) {
        prefix = prefix.substr(0, node_marker_pos);
    } else if (stable_node_marker_pos != std::string::npos) {
        prefix = prefix.substr(0, stable_node_marker_pos);
    }
    std::string payload = target_id.substr(marker_pos + marker.size());
    return prefix + marker + stable_target_suffix(payload);
}

std::string stable_face_target_id(const std::string& target_id) {
    const std::string marker = ":face:";
    std::size_t marker_pos = target_id.find(marker);
    if (marker_pos == std::string::npos) {
        return target_id;
    }
    std::string prefix = target_id.substr(0, marker_pos);
    std::size_t node_marker_pos = prefix.find(":node:");
    std::size_t stable_node_marker_pos = prefix.find(":stable-node-key:");
    if (node_marker_pos != std::string::npos) {
        prefix = prefix.substr(0, node_marker_pos);
    } else if (stable_node_marker_pos != std::string::npos) {
        prefix = prefix.substr(0, stable_node_marker_pos);
    }
    std::string payload = target_id.substr(marker_pos + marker.size());
    return prefix + marker + stable_target_suffix(payload);
}

void write_topology_point(std::ostream& out, const gp_Pnt& point) {
    out << "{\"x\":";
    write_json_number(out, point.X());
    out << ",\"y\":";
    write_json_number(out, point.Y());
    out << ",\"z\":";
    write_json_number(out, point.Z());
    out << "}";
}

void write_topology_direction(std::ostream& out, const gp_Dir& direction) {
    out << "[";
    write_json_number(out, direction.X());
    out << ",";
    write_json_number(out, direction.Y());
    out << ",";
    write_json_number(out, direction.Z());
    out << "]";
}

bool faces_share_support_surface(const TopoDS_Face& lhs, const TopoDS_Face& rhs) {
    constexpr double linear_tolerance = 1.0e-6;
    constexpr double angular_tolerance = 1.0e-9;
    try {
        BRepAdaptor_Surface lhs_surface(lhs);
        BRepAdaptor_Surface rhs_surface(rhs);
        if (lhs_surface.GetType() != rhs_surface.GetType()) return false;
        switch (lhs_surface.GetType()) {
            case GeomAbs_Plane: {
                const gp_Pln lhs_plane = lhs_surface.Plane();
                const gp_Pln rhs_plane = rhs_surface.Plane();
                return lhs_plane.Axis().Direction().IsParallel(
                           rhs_plane.Axis().Direction(), angular_tolerance) &&
                       lhs_plane.Distance(rhs_plane.Location()) <= linear_tolerance;
            }
            case GeomAbs_Cylinder: {
                const gp_Cylinder lhs_cylinder = lhs_surface.Cylinder();
                const gp_Cylinder rhs_cylinder = rhs_surface.Cylinder();
                return std::abs(lhs_cylinder.Radius() - rhs_cylinder.Radius()) <=
                           linear_tolerance &&
                       lhs_cylinder.Axis().Direction().IsParallel(
                           rhs_cylinder.Axis().Direction(), angular_tolerance) &&
                       gp_Lin(lhs_cylinder.Axis()).Distance(rhs_cylinder.Location()) <=
                           linear_tolerance;
            }
            case GeomAbs_Cone: {
                const gp_Cone lhs_cone = lhs_surface.Cone();
                const gp_Cone rhs_cone = rhs_surface.Cone();
                return std::abs(lhs_cone.SemiAngle() - rhs_cone.SemiAngle()) <=
                           angular_tolerance &&
                       std::abs(lhs_cone.RefRadius() - rhs_cone.RefRadius()) <=
                           linear_tolerance &&
                       lhs_cone.Axis().Direction().IsParallel(
                           rhs_cone.Axis().Direction(), angular_tolerance) &&
                       gp_Lin(lhs_cone.Axis()).Distance(rhs_cone.Location()) <= linear_tolerance;
            }
            case GeomAbs_Torus: {
                const gp_Torus lhs_torus = lhs_surface.Torus();
                const gp_Torus rhs_torus = rhs_surface.Torus();
                return lhs_torus.Location().Distance(rhs_torus.Location()) <= linear_tolerance &&
                       std::abs(lhs_torus.MajorRadius() - rhs_torus.MajorRadius()) <=
                           linear_tolerance &&
                       std::abs(lhs_torus.MinorRadius() - rhs_torus.MinorRadius()) <=
                           linear_tolerance &&
                       lhs_torus.Axis().Direction().IsParallel(
                           rhs_torus.Axis().Direction(), angular_tolerance);
            }
            default: {
                TopLoc_Location lhs_location;
                TopLoc_Location rhs_location;
                const Handle(Geom_Surface) lhs_geometry = BRep_Tool::Surface(lhs, lhs_location);
                const Handle(Geom_Surface) rhs_geometry = BRep_Tool::Surface(rhs, rhs_location);
                if (lhs_geometry.IsNull() || lhs_geometry != rhs_geometry) return false;
                const gp_Trsf lhs_transform = lhs_location.Transformation();
                const gp_Trsf rhs_transform = rhs_location.Transformation();
                for (int row = 1; row <= 3; ++row) {
                    for (int column = 1; column <= 4; ++column) {
                        if (std::abs(lhs_transform.Value(row, column) -
                                     rhs_transform.Value(row, column)) > linear_tolerance) {
                            return false;
                        }
                    }
                }
                return true;
            }
        }
    } catch (...) {
        return false;
    }
}

struct CachedTopologyFragment {
    std::string json;
    std::string source_geometry_digest;
};

std::string topology_fragment_cache_key(
    const Part& part,
    const ExecutionContext& context,
    double boundary_linear_deflection,
    double boundary_angular_deflection,
    const std::optional<std::string>& source_geometry_digest_override
);
std::optional<CachedTopologyFragment> read_cached_topology_fragment(
    const fs::path& root,
    const std::string& key,
    ExecutionContext& context
);
void write_cached_topology_fragment(
    const fs::path& root,
    const std::string& key,
    const CachedTopologyFragment& fragment,
    ExecutionContext& context
);

std::string write_part_topology(
    std::ostream& out,
    const std::string& part_id,
    const std::string& label,
    const TopoDS_Shape& shape,
    const std::map<std::string, TopoDS_Shape>& authored_bindings,
    double boundary_linear_deflection,
    double boundary_angular_deflection,
    const std::optional<std::string>& source_geometry_digest_override,
    bool& first_part
) {
    if (!first_part) {
        out << ",";
    }
    first_part = false;

    out << "{\"partId\":";
    out << quote_json_string(part_id);
    out << ",\"label\":";
    out << quote_json_string(label);
    out << ",\"representation\":\"analyticBrep\"";
    int solid_count = shape.ShapeType() == TopAbs_SOLID ? 1 : 0;
    if (solid_count == 0) {
        for (TopExp_Explorer explorer(shape, TopAbs_SOLID); explorer.More(); explorer.Next()) {
            ++solid_count;
        }
    }
    const bool brep_valid = !shape.IsNull() && BRepCheck_Analyzer(shape).IsValid();
    out << ",\"solidCount\":" << solid_count;
    out << ",\"brepValid\":" << (brep_valid ? "true" : "false");
    out << ",\"solidBounds\":[";
    bool first_solid_bound = true;
    auto write_solid_bound = [&](const TopoDS_Shape& solid) {
        Bnd_Box bounds;
        BRepBndLib::Add(solid, bounds);
        if (bounds.IsVoid()) return;
        double x_min, y_min, z_min, x_max, y_max, z_max;
        bounds.Get(x_min, y_min, z_min, x_max, y_max, z_max);
        if (!first_solid_bound) out << ",";
        first_solid_bound = false;
        out << "{\"xMin\":" << x_min
            << ",\"yMin\":" << y_min
            << ",\"zMin\":" << z_min
            << ",\"xMax\":" << x_max
            << ",\"yMax\":" << y_max
            << ",\"zMax\":" << z_max << "}";
    };
    if (shape.ShapeType() == TopAbs_SOLID) {
        write_solid_bound(shape);
    } else {
        for (TopExp_Explorer explorer(shape, TopAbs_SOLID); explorer.More(); explorer.Next()) {
            write_solid_bound(explorer.Current());
        }
    }
    out << "]";

    BRepBuilderAPI_Copy private_topology(shape, Standard_False, Standard_False);
    const TopoDS_Shape mesh_shape = private_topology.Shape();
    if (!mesh_shape.IsNull()) {
        BRepMesh_IncrementalMesh mesher(
            mesh_shape,
            boundary_linear_deflection,
            Standard_False,
            boundary_angular_deflection,
            Standard_True);
        (void)mesher;
    }
    TopTools_IndexedMapOfShape edge_map;
    TopExp::MapShapes(mesh_shape, TopAbs_EDGE, edge_map);
    out << ",\"authoredBindingEdgeOrder\":[";
    bool first_ordered_binding = true;
    for (const auto& [name, binding_shape] : authored_bindings) {
        std::vector<TopoDS_Edge> source_edges;
        if (binding_shape.ShapeType() == TopAbs_EDGE) {
            source_edges.push_back(TopoDS::Edge(binding_shape));
        } else {
            TopTools_IndexedMapOfShape binding_wires;
            TopExp::MapShapes(binding_shape, TopAbs_WIRE, binding_wires);
            if (binding_shape.ShapeType() == TopAbs_WIRE) {
                binding_wires.Clear();
                binding_wires.Add(binding_shape);
            }
            if (binding_wires.Extent() != 1) {
                continue;
            }
            BRepTools_WireExplorer explorer(TopoDS::Wire(binding_wires.FindKey(1)));
            while (explorer.More()) {
                source_edges.push_back(TopoDS::Edge(explorer.Current()));
                explorer.Next();
            }
        }
        if (source_edges.empty()) {
            continue;
        }
        std::vector<std::string> ordered_target_ids;
        bool complete_mapping = true;
        for (const TopoDS_Edge& source_edge : source_edges) {
            try {
                const TopoDS_Shape copied = private_topology.ModifiedShape(source_edge);
                const int copied_index = copied.IsNull() ? 0 : edge_map.FindIndex(copied);
                if (copied_index <= 0) {
                    complete_mapping = false;
                    break;
                }
                const TopoDS_Edge copied_edge = TopoDS::Edge(edge_map.FindKey(copied_index));
                ordered_target_ids.push_back(
                    edge_target_id(part_id, copied_index - 1, copied_edge));
            } catch (...) {
                complete_mapping = false;
                break;
            }
        }
        if (!complete_mapping || ordered_target_ids.empty()) {
            continue;
        }
        if (!first_ordered_binding) out << ",";
        first_ordered_binding = false;
        out << "{\"name\":" << quote_json_string(name) << ",\"targetIds\":[";
        for (std::size_t index = 0; index < ordered_target_ids.size(); ++index) {
            if (index > 0) out << ",";
            out << quote_json_string(ordered_target_ids[index]);
        }
        out << "]}";
    }
    out << "]";
    out << ",\"vertices\":[";
    struct AuthoredBindingTopologyIndex {
        std::string name;
        TopTools_IndexedMapOfShape copied_vertices;
        TopTools_IndexedMapOfShape copied_edges;
        TopTools_IndexedMapOfShape copied_faces;
        std::vector<TopoDS_Face> source_faces;
    };
    std::vector<AuthoredBindingTopologyIndex> authored_binding_indices;
    authored_binding_indices.reserve(authored_bindings.size());
    for (const auto& [name, binding_shape] : authored_bindings) {
        AuthoredBindingTopologyIndex index;
        index.name = name;
        const TopoDS_Shape indexed_binding_shape = binding_shape;
        const auto index_subshapes = [&](TopAbs_ShapeEnum shape_type,
                                         TopTools_IndexedMapOfShape& copied_shapes) {
            TopTools_IndexedMapOfShape source_shapes;
            TopExp::MapShapes(indexed_binding_shape, shape_type, source_shapes);
            for (int ordinal = 1; ordinal <= source_shapes.Extent(); ++ordinal) {
                const TopoDS_Shape source_shape = source_shapes.FindKey(ordinal);
                if (shape_type == TopAbs_FACE) {
                    index.source_faces.push_back(TopoDS::Face(source_shape));
                }
                try {
                    const TopoDS_Shape copied = private_topology.ModifiedShape(source_shape);
                    if (!copied.IsNull() && copied.ShapeType() == shape_type) {
                        copied_shapes.Add(copied);
                    }
                } catch (...) {
                }
            }
        };
        index_subshapes(TopAbs_VERTEX, index.copied_vertices);
        index_subshapes(TopAbs_EDGE, index.copied_edges);
        index_subshapes(TopAbs_FACE, index.copied_faces);
        authored_binding_indices.push_back(std::move(index));
    }
    auto write_authored_bindings = [&](const TopoDS_Shape& target) {
        out << ",\"authoredBindings\":[";
        bool first_binding = true;
        for (const AuthoredBindingTopologyIndex& index : authored_binding_indices) {
            bool matched = target.ShapeType() == TopAbs_VERTEX
                ? index.copied_vertices.FindIndex(target) > 0
                : target.ShapeType() == TopAbs_EDGE
                    ? index.copied_edges.FindIndex(target) > 0
                    : target.ShapeType() == TopAbs_FACE &&
                        index.copied_faces.FindIndex(target) > 0;
            if (!matched && target.ShapeType() == TopAbs_FACE) {
                for (const TopoDS_Face& source_face : index.source_faces) {
                    if (faces_share_support_surface(TopoDS::Face(target), source_face)) {
                        matched = true;
                        break;
                    }
                }
            }
            if (!matched) continue;
            if (!first_binding) out << ",";
            first_binding = false;
            out << quote_json_string(index.name);
        }
        out << "]";
    };

    bool first_vertex = true;
    TopTools_IndexedMapOfShape vertex_map;
    TopExp::MapShapes(mesh_shape, TopAbs_VERTEX, vertex_map);
    for (int vertex_ordinal = 1; vertex_ordinal <= vertex_map.Extent(); ++vertex_ordinal) {
        try {
            const TopoDS_Vertex vertex = TopoDS::Vertex(vertex_map.FindKey(vertex_ordinal));
            const gp_Pnt point = BRep_Tool::Pnt(vertex);
            if (!first_vertex) {
                out << ",";
            }
            first_vertex = false;
            const int vertex_index = vertex_ordinal - 1;
            out << "{\"targetId\":";
            out << quote_json_string(vertex_target_id(part_id, vertex_index, vertex));
            out << ",\"vertexIndex\":" << vertex_index;
            out << ",\"label\":";
            out << quote_json_string(label + ".Vertex" + std::to_string(vertex_ordinal));
            out << ",\"point\":";
            write_topology_point(out, point);
            out << ",\"exactGeometry\":{\"kind\":\"vertex\",\"point\":";
            write_topology_point(out, point);
            out << "}";
            write_authored_bindings(vertex);
            out << "}";
        } catch (...) {
        }
    }

    out << "],\"edges\":[";

    bool first_edge = true;
    for (int edge_ordinal = 1; edge_ordinal <= edge_map.Extent(); ++edge_ordinal) {
        try {
            TopoDS_Edge edge = TopoDS::Edge(edge_map.FindKey(edge_ordinal));
            BRepAdaptor_Curve curve(edge);
            double first_param = curve.FirstParameter();
            double last_param = curve.LastParameter();
            if (!std::isfinite(first_param) || !std::isfinite(last_param)) {
                continue;
            }
            gp_Pnt start = curve.Value(first_param);
            gp_Pnt end = curve.Value(last_param);
            if (!first_edge) {
                out << ",";
            }
            first_edge = false;
            int edge_index = edge_ordinal - 1;
            out << "{\"targetId\":";
            out << quote_json_string(edge_target_id(part_id, edge_index, edge));
            out << ",\"edgeIndex\":" << edge_index;
            out << ",\"label\":";
            out << quote_json_string(label + ".Edge" + std::to_string(edge_ordinal));
            out << ",\"start\":";
            write_topology_point(out, start);
            out << ",\"end\":";
            write_topology_point(out, end);
            TopoDS_Vertex first_vertex_shape;
            TopoDS_Vertex last_vertex_shape;
            TopExp::Vertices(edge, first_vertex_shape, last_vertex_shape);
            out << ",\"vertexTargetIds\":[";
            bool first_endpoint = true;
            for (const TopoDS_Vertex& endpoint : {first_vertex_shape, last_vertex_shape}) {
                if (endpoint.IsNull()) continue;
                const int vertex_ordinal = vertex_map.FindIndex(endpoint);
                if (vertex_ordinal <= 0) continue;
                if (!first_endpoint) out << ",";
                first_endpoint = false;
                out << quote_json_string(vertex_target_id(
                    part_id,
                    vertex_ordinal - 1,
                    TopoDS::Vertex(vertex_map.FindKey(vertex_ordinal))));
            }
            out << "]";
            if (curve.GetType() == GeomAbs_Line) {
                out << ",\"exactGeometry\":{\"kind\":\"lineEdge\",\"start\":";
                write_topology_point(out, start);
                out << ",\"end\":";
                write_topology_point(out, end);
                out << "}";
            } else if (curve.GetType() == GeomAbs_Circle) {
                const gp_Circ circle = curve.Circle();
                out << ",\"exactGeometry\":{\"kind\":\"circleEdge\",\"center\":";
                write_topology_point(out, circle.Location());
                out << ",\"normal\":";
                write_topology_direction(out, circle.Axis().Direction());
                out << ",\"xDirection\":";
                write_topology_direction(out, circle.XAxis().Direction());
                out << ",\"radius\":";
                write_json_number(out, circle.Radius());
                out << ",\"firstParameter\":";
                write_json_number(out, first_param);
                out << ",\"lastParameter\":";
                write_json_number(out, last_param);
                out << "}";
            }
            write_authored_bindings(edge);
            out << "}";
        } catch (...) {
        }
    }

    out << "],\"faces\":[";
    bool first_face = true;
    int face_index = 0;
    std::vector<std::array<gp_Pnt, 3>> boundary_triangles;
    std::vector<int> triangle_face_group_indices;
    ecky::Sha256 source_geometry_hash;
    source_geometry_hash.update("ecky-analysis-boundary-source-v1");
    source_geometry_hash.update("|");
    source_geometry_hash.update(part_id);
    source_geometry_hash.update("|");
    source_geometry_hash.update(label);
    source_geometry_hash.update("|");
    auto write_face_boundary_edge_target_ids = [&](const TopoDS_Face& face) {
        std::vector<std::vector<std::string>> loops;
        bool complete = true;
        for (TopExp_Explorer wire_explorer(face, TopAbs_WIRE);
             wire_explorer.More(); wire_explorer.Next()) {
            std::vector<std::string> loop;
            BRepTools_WireExplorer edge_explorer(TopoDS::Wire(wire_explorer.Current()), face);
            while (edge_explorer.More()) {
                const TopoDS_Edge edge = TopoDS::Edge(edge_explorer.Current());
                const int edge_index = edge_map.FindIndex(edge);
                if (edge_index <= 0) {
                    complete = false;
                    break;
                }
                loop.push_back(edge_target_id(
                    part_id,
                    edge_index - 1,
                    TopoDS::Edge(edge_map.FindKey(edge_index))));
                edge_explorer.Next();
            }
            if (!complete || loop.empty()) {
                complete = false;
                break;
            }
            loops.push_back(std::move(loop));
        }
        out << ",\"boundaryEdgeTargetIds\":[";
        if (complete) {
            for (std::size_t loop_index = 0; loop_index < loops.size(); ++loop_index) {
                if (loop_index > 0) out << ",";
                out << "[";
                for (std::size_t edge_index = 0; edge_index < loops[loop_index].size(); ++edge_index) {
                    if (edge_index > 0) out << ",";
                    out << quote_json_string(loops[loop_index][edge_index]);
                }
                out << "]";
            }
        }
        out << "]";
    };
    for (TopExp_Explorer explorer(mesh_shape, TopAbs_FACE); explorer.More(); explorer.Next(), ++face_index) {
        try {
            TopoDS_Face face = TopoDS::Face(explorer.Current());
            GProp_GProps props;
            BRepGProp::SurfaceProperties(face, props);
            gp_Pnt center = props.CentreOfMass();
            double area = props.Mass();
            const std::string face_target = face_target_id(part_id, face_index, face);

            double normal_x = 0.0;
            double normal_y = 0.0;
            double normal_z = 0.0;
            try {
                double u_min = 0.0;
                double u_max = 0.0;
                double v_min = 0.0;
                double v_max = 0.0;
                BRepTools::UVBounds(face, u_min, u_max, v_min, v_max);
                if (std::isfinite(u_min) && std::isfinite(u_max) && std::isfinite(v_min) &&
                    std::isfinite(v_max)) {
                    BRepAdaptor_Surface surface(face);
                    gp_Pnt surface_point;
                    gp_Vec du;
                    gp_Vec dv;
                    surface.D1((u_min + u_max) / 2.0, (v_min + v_max) / 2.0, surface_point, du, dv);
                    gp_Vec normal = du.Crossed(dv);
                    if (normal.Magnitude() > 1.0e-9) {
                        normal.Normalize();
                        normal_x = normal.X();
                        normal_y = normal.Y();
                        normal_z = normal.Z();
                    }
                }
            } catch (...) {
            }

            if (!first_face) {
                out << ",";
            }
            first_face = false;
            out << "{\"targetId\":";
            out << quote_json_string(face_target);
            out << ",\"faceIndex\":" << face_index;
            out << ",\"label\":";
            out << quote_json_string(label + ".Face" + std::to_string(face_index + 1));
            out << ",\"center\":";
            write_topology_point(out, center);
            out << ",\"normal\":[";
            write_json_number(out, normal_x);
            out << ",";
            write_json_number(out, normal_y);
            out << ",";
            write_json_number(out, normal_z);
            out << "],\"area\":";
            write_json_number(out, area);
            try {
                BRepAdaptor_Surface exact_surface(face);
                if (exact_surface.GetType() == GeomAbs_Plane) {
                    const gp_Pln plane = exact_surface.Plane();
                    out << ",\"exactGeometry\":{\"kind\":\"planeFace\",\"origin\":";
                    write_topology_point(out, plane.Location());
                    out << ",\"normal\":";
                    write_topology_direction(out, plane.Axis().Direction());
                    write_face_boundary_edge_target_ids(face);
                    out << "}";
                } else if (exact_surface.GetType() == GeomAbs_Cylinder) {
                    const gp_Cylinder cylinder = exact_surface.Cylinder();
                    out << ",\"exactGeometry\":{\"kind\":\"cylinderFace\",\"axisOrigin\":";
                    write_topology_point(out, cylinder.Location());
                    out << ",\"axisDirection\":";
                    write_topology_direction(out, cylinder.Axis().Direction());
                    out << ",\"radius\":";
                    write_json_number(out, cylinder.Radius());
                    write_face_boundary_edge_target_ids(face);
                    out << "}";
                }
            } catch (...) {
            }
            write_face_boundary_edge_target_ids(face);
            write_authored_bindings(face);
            out << "}";

            source_geometry_hash.update(face_target);
            source_geometry_hash.update("|");
            source_geometry_hash.update(std::to_string(face_index));
            source_geometry_hash.update("|");
            source_geometry_hash.update(ecky::canonical_f64(area));
            source_geometry_hash.update("|");

            TopLoc_Location location;
            Handle(Poly_Triangulation) triangulation = BRep_Tool::Triangulation(face, location);
            if (triangulation.IsNull()) {
                continue;
            }
            gp_Trsf transform = location.Transformation();
            for (Standard_Integer triangle_index = 1;
                 triangle_index <= triangulation->NbTriangles();
                 ++triangle_index) {
                Standard_Integer n1 = 0;
                Standard_Integer n2 = 0;
                Standard_Integer n3 = 0;
                triangulation->Triangle(triangle_index).Get(n1, n2, n3);
                gp_Pnt p1 = triangulation->Node(n1).Transformed(transform);
                gp_Pnt p2 = triangulation->Node(n2).Transformed(transform);
                gp_Pnt p3 = triangulation->Node(n3).Transformed(transform);
                if (face.Orientation() == TopAbs_REVERSED) std::swap(p2, p3);
                const gp_Vec normal = gp_Vec(p1, p2).Crossed(gp_Vec(p1, p3));
                if (normal.SquareMagnitude() <= 1.0e-18) {
                    continue;
                }
                boundary_triangles.push_back({p1, p2, p3});
                triangle_face_group_indices.push_back(face_index);
                for (const gp_Pnt& vertex : boundary_triangles.back()) {
                    source_geometry_hash.update(ecky::canonical_f64(vertex.X()));
                    source_geometry_hash.update("|");
                    source_geometry_hash.update(ecky::canonical_f64(vertex.Y()));
                    source_geometry_hash.update("|");
                    source_geometry_hash.update(ecky::canonical_f64(vertex.Z()));
                    source_geometry_hash.update("|");
                }
            }
        } catch (...) {
        }
    }

    out << "],\"triangles\":[";
    auto write_triangle_vertex = [&](const gp_Pnt& point) {
        out << "[";
        write_json_number(out, point.X());
        out << ",";
        write_json_number(out, point.Y());
        out << ",";
        write_json_number(out, point.Z());
        out << "]";
    };
    for (std::size_t index = 0; index < boundary_triangles.size(); ++index) {
        if (index != 0) {
            out << ",";
        }
        const auto& triangle = boundary_triangles[index];
        out << "{\"vertices\":";
        out << "[";
        write_triangle_vertex(triangle[0]);
        out << ",";
        write_triangle_vertex(triangle[1]);
        out << ",";
        write_triangle_vertex(triangle[2]);
        out << "]}";
    }
    out << "],\"triangleFaceGroupIndices\":[";
    for (std::size_t index = 0; index < triangle_face_group_indices.size(); ++index) {
        if (index != 0) {
            out << ",";
        }
        out << triangle_face_group_indices[index];
    }
    const std::string computed_source_geometry_digest =
        "sha256:" + source_geometry_hash.finish_hex();
    const std::string& source_geometry_digest = source_geometry_digest_override.has_value()
        ? *source_geometry_digest_override
        : computed_source_geometry_digest;
    out << "],\"sourceGeometryDigest\":";
    out << quote_json_string(source_geometry_digest);
    out << "}";
    return source_geometry_digest;
}

// FEM volume meshing must not inherit the dense display/export tessellation.
// These errors stay well below the authored 1.2 mm local Tet4 target while the
// wider angular bound avoids preserving tens of thousands of display facets.
static constexpr double kAnalysisBoundaryLinearDeflection = 0.25;
static constexpr double kAnalysisBoundaryAngularDeflection = 0.30;

std::map<std::string, std::string> write_topology_report(
    const fs::path& topology_path,
    const Plan& plan,
    const std::vector<ShapeRecord>& parts,
    double boundary_linear_deflection,
    double boundary_angular_deflection,
    const std::optional<fs::path>& cache_root,
    ecky::RenderCacheTransaction* cache_transaction,
    ExecutionContext& context,
    const std::map<std::string, std::string>& source_geometry_digest_overrides = {}
) {
    if (plan.parts.size() != parts.size()) {
        throw EvalError("topology report plan/shape part count mismatch");
    }
    if (cache_root.has_value() != (cache_transaction != nullptr)) {
        throw EvalError("topology cache transaction must match cache configuration");
    }
    std::ofstream out(topology_path);
    if (!out) {
        throw IoError("failed to open topology file");
    }
    out << "{\"schemaVersion\":1,\"tessellationPolicy\":{\"linearDeflectionMm\":";
    write_json_number(out, boundary_linear_deflection);
    out << ",\"angularDeflectionRad\":";
    write_json_number(out, boundary_angular_deflection);
    out << "},\"parts\":[";
    bool first_part = true;
    std::map<std::string, std::string> source_geometry_digests;
    for (std::size_t part_index = 0; part_index < parts.size(); ++part_index) {
        const auto& part = parts[part_index];
        if (part.kind == ShapeRecord::Kind::Shape) {
            const auto override_it = source_geometry_digest_overrides.find(part.part_id);
            const std::optional<std::string> source_geometry_digest_override =
                override_it == source_geometry_digest_overrides.end()
                    ? std::nullopt
                    : std::optional<std::string>(override_it->second);
            const std::string cache_key = topology_fragment_cache_key(
                plan.parts[part_index],
                context,
                boundary_linear_deflection,
                boundary_angular_deflection,
                source_geometry_digest_override);
            std::optional<CachedTopologyFragment> fragment;
            if (cache_root.has_value()) {
                fragment = read_cached_topology_fragment(*cache_root, cache_key, context);
                std::lock_guard<std::mutex> lock(context.mutex);
                if (fragment.has_value()) ++context.topology_cache_hit_count;
                else ++context.topology_cache_miss_count;
            }
            if (!fragment.has_value()) {
                std::ostringstream fragment_json;
                bool first_fragment_part = true;
                const std::string source_geometry_digest = write_part_topology(
                    fragment_json,
                    part.part_id,
                    part.label,
                    part.shape,
                    part.authored_bindings,
                    boundary_linear_deflection,
                    boundary_angular_deflection,
                    source_geometry_digest_override,
                    first_fragment_part);
                fragment = CachedTopologyFragment{
                    fragment_json.str(), source_geometry_digest};
                if (cache_transaction != nullptr) {
                    write_cached_topology_fragment(
                        cache_transaction->staging_root(), cache_key, *fragment, context);
                    std::lock_guard<std::mutex> lock(context.mutex);
                    ++context.topology_cache_write_count;
                }
            }
            if (!first_part) out << ',';
            first_part = false;
            out << fragment->json;
            source_geometry_digests[part.part_id] = fragment->source_geometry_digest;
        } else {
            if (!first_part) out << ',';
            first_part = false;
            out << "{\"partId\":" << quote_json_string(part.part_id)
                << ",\"label\":" << quote_json_string(part.label)
                << ",\"representation\":\"meshDomain\",\"edges\":[],\"faces\":[]}";
        }
    }
    out << "]}";
    if (!out.good()) {
        throw IoError("failed to write topology file");
    }
    return source_geometry_digests;
}

Arg require_ref_arg(const std::vector<Arg>& args, std::size_t index, const std::string& op) {
    if (index >= args.size()) {
        throw EvalError(op + " expects a shape reference");
    }
    const Arg& arg = args[index];
    if (arg.kind != Arg::Kind::Ref) {
        throw EvalError(op + " expects a shape reference");
    }
    return arg;
}

double require_number_arg(const std::vector<Arg>& args, std::size_t index, const std::string& op) {
    if (index >= args.size()) {
        throw EvalError(op + " expects a number");
    }
    const Arg& arg = args[index];
    if (arg.kind != Arg::Kind::Number) {
        throw EvalError(op + " expects a number");
    }
    return arg.number_value;
}

bool require_bool_arg(const std::vector<Arg>& args, std::size_t index, const std::string& op) {
    if (index >= args.size()) {
        throw EvalError(op + " expects a boolean");
    }
    const Arg& arg = args[index];
    if (arg.kind != Arg::Kind::Boolean) {
        throw EvalError(op + " expects a boolean");
    }
    return arg.bool_value;
}

std::size_t require_count_arg(const std::vector<Arg>& args, std::size_t index, const std::string& op) {
    double value = require_number_arg(args, index, op);
    if (!std::isfinite(value)) {
        throw EvalError(op + " expects a finite count");
    }
    return static_cast<std::size_t>(std::max(1.0, std::round(value)));
}

std::vector<Arg> require_ref_list(const std::vector<Arg>& args, const std::string& op) {
    std::vector<Arg> refs;
    for (std::size_t index = 0; index < args.size(); ++index) {
        const Arg& arg = args[index];
        if (arg.kind != Arg::Kind::Ref) {
            throw EvalError(op + " expects shape references");
        }
        refs.push_back(arg);
    }
    if (refs.empty()) {
        throw EvalError(op + " expects at least one shape reference");
    }
    return refs;
}

std::vector<std::uint64_t> require_ref_collection_arg(const Arg& arg, const std::string& label) {
    if (arg.kind == Arg::Kind::Ref) {
        return {arg.ref_value};
    }
    if (arg.kind == Arg::Kind::List) {
        std::vector<std::uint64_t> refs;
        refs.reserve(arg.list_value.size());
        for (std::size_t index = 0; index < arg.list_value.size(); ++index) {
            if (arg.list_value[index].kind != Arg::Kind::Ref) {
                throw EvalError(label + " expects shape reference at index " + std::to_string(index));
            }
            refs.push_back(arg.list_value[index].ref_value);
        }
        return refs;
    }
    throw EvalError(label + " expects shape reference or reference list");
}

std::array<double, 2> require_range_arg(const Arg& arg, const std::string& label) {
    double first = 0.0;
    double second = 0.0;
    if (arg.kind == Arg::Kind::Point2) {
        first = arg.point2_value[0];
        second = arg.point2_value[1];
    } else if (arg.kind == Arg::Kind::List && arg.list_value.size() == 2 &&
               arg.list_value[0].kind == Arg::Kind::Number &&
               arg.list_value[1].kind == Arg::Kind::Number) {
        first = arg.list_value[0].number_value;
        second = arg.list_value[1].number_value;
    } else {
        throw EvalError(label + " expects numeric 2D range");
    }
    if (std::abs(first - second) <= 1.0e-12) {
        throw EvalError(label + " must not be zero width");
    }
    return {std::min(first, second), std::max(first, second)};
}

std::array<double, 2> require_point2_arg(const Arg& arg, const std::string& label) {
    if (arg.kind == Arg::Kind::Point2) {
        return arg.point2_value;
    }
    if (arg.kind == Arg::Kind::List && arg.list_value.size() == 2 &&
        arg.list_value[0].kind == Arg::Kind::Number &&
        arg.list_value[1].kind == Arg::Kind::Number) {
        return {arg.list_value[0].number_value, arg.list_value[1].number_value};
    }
    throw EvalError(label + " expects point2 values");
}

struct ProfileRefs {
    std::vector<std::uint64_t> outer;
    std::vector<std::uint64_t> holes;
    bool soup = false;
};

ProfileRefs profile_refs(const Command& command) {
    ProfileRefs refs;
    if (command.keywords.empty()) {
        if (command.args.empty()) {
            throw EvalError("profile needs at least one outer loop");
        }
        for (const auto& arg : command.args) {
            if (arg.kind != Arg::Kind::Ref) {
                throw EvalError("profile positional outer loops expect shape references");
            }
            refs.outer.push_back(arg.ref_value);
        }
        return refs;
    }
    if (!command.args.empty()) {
        throw EvalError("profile does not mix positional loops with keyword loops");
    }
    for (const auto& keyword : command.keywords) {
        if (keyword.kind != Keyword::Kind::Arg) {
            throw EvalError("profile keywords expect arg values only");
        }
        if (keyword.name == "outer") {
            std::vector<std::uint64_t> outer =
                require_ref_collection_arg(keyword.value, "profile :outer");
            refs.outer.insert(refs.outer.end(), outer.begin(), outer.end());
            continue;
        }
        if (keyword.name == "holes") {
            std::vector<std::uint64_t> holes =
                require_ref_collection_arg(keyword.value, "profile :holes");
            refs.holes.insert(refs.holes.end(), holes.begin(), holes.end());
            continue;
        }
        if (keyword.name == "fill-rule") {
            // SVG wire-soup contract: marks wires as unclassified soup; holes
            // are resolved by containment parity at execute time.
            refs.soup = true;
            continue;
        }
        throw EvalError("profile does not recognize `:" + keyword.name + "`");
    }
    if (refs.outer.empty()) {
        throw EvalError("profile needs at least one outer loop");
    }
    return refs;
}

struct ClipBoxArgs {
    std::uint64_t shape_ref = 0;
    std::array<double, 2> x{0.0, 0.0};
    std::array<double, 2> y{0.0, 0.0};
    std::array<double, 2> z{0.0, 0.0};
};

ClipBoxArgs clip_box_args(const Command& command) {
    if (command.args.size() != 1 || command.args[0].kind != Arg::Kind::Ref) {
        throw EvalError("clip-box expects one shape reference");
    }
    ClipBoxArgs args;
    args.shape_ref = command.args[0].ref_value;
    bool has_x = false;
    bool has_y = false;
    bool has_z = false;
    for (const auto& keyword : command.keywords) {
        if (keyword.kind != Keyword::Kind::Arg) {
            throw EvalError("clip-box keywords expect arg values only");
        }
        if (keyword.name == "x") {
            args.x = require_range_arg(keyword.value, "clip-box :x");
            has_x = true;
            continue;
        }
        if (keyword.name == "y") {
            args.y = require_range_arg(keyword.value, "clip-box :y");
            has_y = true;
            continue;
        }
        if (keyword.name == "z") {
            args.z = require_range_arg(keyword.value, "clip-box :z");
            has_z = true;
            continue;
        }
        throw EvalError("clip-box does not recognize `:" + keyword.name + "`");
    }
    if (!has_x) {
        throw EvalError("clip-box requires `:x`");
    }
    if (!has_y) {
        throw EvalError("clip-box requires `:y`");
    }
    if (!has_z) {
        throw EvalError("clip-box requires `:z`");
    }
    return args;
}

struct ClipPlaneArgs {
    std::uint64_t shape_ref = 0;
    std::array<double, 3> origin{0.0, 0.0, 0.0};
    std::array<double, 3> normal{0.0, 0.0, 1.0};
    bool keep_positive = true;
};

std::array<double, 3> require_point3_like_arg(const Arg& arg, const std::string& label);

ClipPlaneArgs clip_plane_args(const Command& command) {
    if (command.args.size() != 1 || command.args[0].kind != Arg::Kind::Ref) {
        throw EvalError("clip-plane expects one shape reference");
    }
    ClipPlaneArgs args;
    args.shape_ref = command.args[0].ref_value;
    bool has_origin = false;
    bool has_normal = false;
    bool has_keep = false;
    for (const auto& keyword : command.keywords) {
        if (keyword.kind != Keyword::Kind::Arg) {
            throw EvalError("clip-plane keywords expect arg values only");
        }
        if (keyword.name == "origin") {
            args.origin = require_point3_like_arg(keyword.value, "clip-plane :origin");
            has_origin = true;
            continue;
        }
        if (keyword.name == "normal") {
            args.normal = require_point3_like_arg(keyword.value, "clip-plane :normal");
            has_normal = true;
            continue;
        }
        if (keyword.name == "keep") {
            if (keyword.value.kind != Arg::Kind::Text &&
                keyword.value.kind != Arg::Kind::Symbol) {
                throw EvalError("clip-plane :keep expects `positive` or `negative`");
            }
            if (keyword.value.text_value == "positive") {
                args.keep_positive = true;
            } else if (keyword.value.text_value == "negative") {
                args.keep_positive = false;
            } else {
                throw EvalError("clip-plane :keep expects `positive` or `negative`");
            }
            has_keep = true;
            continue;
        }
        throw EvalError("clip-plane does not recognize `:" + keyword.name + "`");
    }
    if (!has_origin) {
        throw EvalError("clip-plane requires `:origin`");
    }
    if (!has_normal) {
        throw EvalError("clip-plane requires `:normal`");
    }
    if (!has_keep) {
        throw EvalError("clip-plane requires `:keep`");
    }
    return args;
}

enum class AlignMode {
    Min,
    Center,
    Max,
};

struct BoxArgs {
    double width = 0.0;
    double depth = 0.0;
    double height = 0.0;
    std::array<AlignMode, 3> align{AlignMode::Center, AlignMode::Center, AlignMode::Min};
};

struct SphereArgs {
    double radius = 0.0;
    std::array<AlignMode, 3> align{AlignMode::Center, AlignMode::Center, AlignMode::Center};
};

struct CylinderArgs {
    double radius = 0.0;
    double height = 0.0;
    std::array<AlignMode, 3> align{AlignMode::Center, AlignMode::Center, AlignMode::Min};
};

struct ConeArgs {
    double radius1 = 0.0;
    double radius2 = 0.0;
    double height = 0.0;
    std::array<AlignMode, 3> align{AlignMode::Center, AlignMode::Center, AlignMode::Min};
};

struct TorusArgs {
    double major = 0.0;
    double minor = 0.0;
    std::array<AlignMode, 3> align{AlignMode::Center, AlignMode::Center, AlignMode::Center};
};

struct WedgeArgs {
    std::array<double, 7> dims{0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
    std::array<AlignMode, 3> align{AlignMode::Center, AlignMode::Center, AlignMode::Center};
};

AlignMode require_align_mode(const Arg& arg, const std::string& label) {
    if (arg.kind != Arg::Kind::Symbol && arg.kind != Arg::Kind::Text) {
        throw EvalError(label + " expects `min`, `center`, or `max` symbols");
    }
    const std::string& value = arg.text_value;
    if (value == "min") {
        return AlignMode::Min;
    }
    if (value == "center") {
        return AlignMode::Center;
    }
    if (value == "max") {
        return AlignMode::Max;
    }
    throw EvalError(label + " expects `min`, `center`, or `max`, got `" + value + "`");
}

std::array<AlignMode, 3> require_align_tuple(const Arg& arg, const std::string& label) {
    if (arg.kind != Arg::Kind::List || arg.list_value.size() != 3) {
        throw EvalError(label + " expects `(x y z)` axis symbols");
    }
    return {
        require_align_mode(arg.list_value[0], label),
        require_align_mode(arg.list_value[1], label),
        require_align_mode(arg.list_value[2], label),
    };
}

double align_offset(double size, AlignMode align) {
    switch (align) {
        case AlignMode::Min:
            return 0.0;
        case AlignMode::Center:
            return -size * 0.5;
        case AlignMode::Max:
            return -size;
    }
    return 0.0;
}

double centered_align_offset(double size, AlignMode align) {
    switch (align) {
        case AlignMode::Min:
            return size * 0.5;
        case AlignMode::Center:
            return 0.0;
        case AlignMode::Max:
            return -size * 0.5;
    }
    return 0.0;
}

BoxArgs box_args(const Command& command) {
    if (command.args.size() != 3) {
        throw EvalError("box expects width, depth, and height");
    }
    BoxArgs args;
    args.width = require_number_arg(command.args, 0, "box");
    args.depth = require_number_arg(command.args, 1, "box");
    args.height = require_number_arg(command.args, 2, "box");
    for (const auto& keyword : command.keywords) {
        if (keyword.kind != Keyword::Kind::Arg) {
            throw EvalError("box keywords expect arg values only");
        }
        if (keyword.name == "align") {
            args.align = require_align_tuple(keyword.value, "box :align");
            continue;
        }
        throw EvalError("box does not recognize `:" + keyword.name + "`");
    }
    return args;
}

void apply_align_keywords(
    const std::string& op,
    const std::vector<Keyword>& keywords,
    std::array<AlignMode, 3>& align
) {
    for (const auto& keyword : keywords) {
        if (keyword.kind != Keyword::Kind::Arg) {
            throw EvalError(op + " keywords expect arg values only");
        }
        if (keyword.name == "align") {
            align = require_align_tuple(keyword.value, op + " :align");
            continue;
        }
        throw EvalError(op + " does not recognize `:" + keyword.name + "`");
    }
}

SphereArgs sphere_args(const Command& command) {
    if (command.args.empty()) {
        throw EvalError("sphere expects radius");
    }
    SphereArgs args;
    args.radius = require_number_arg(command.args, 0, "sphere");
    apply_align_keywords("sphere", command.keywords, args.align);
    return args;
}

CylinderArgs cylinder_args(const Command& command) {
    if (command.args.size() < 2) {
        throw EvalError("cylinder expects radius and height");
    }
    CylinderArgs args;
    args.radius = require_number_arg(command.args, 0, "cylinder");
    args.height = require_number_arg(command.args, 1, "cylinder");
    apply_align_keywords("cylinder", command.keywords, args.align);
    return args;
}

ConeArgs cone_args(const Command& command) {
    if (command.args.size() < 3) {
        throw EvalError("cone expects two radii and height");
    }
    ConeArgs args;
    args.radius1 = require_number_arg(command.args, 0, "cone");
    args.radius2 = require_number_arg(command.args, 1, "cone");
    args.height = require_number_arg(command.args, 2, "cone");
    apply_align_keywords("cone", command.keywords, args.align);
    return args;
}

TorusArgs torus_args(const Command& command) {
    if (command.args.size() < 2) {
        throw EvalError("torus expects major and minor radius");
    }
    TorusArgs args;
    args.major = require_number_arg(command.args, 0, "torus");
    args.minor = require_number_arg(command.args, 1, "torus");
    apply_align_keywords("torus", command.keywords, args.align);
    return args;
}

WedgeArgs wedge_args(const Command& command) {
    if (command.args.size() < 7) {
        throw EvalError("wedge expects dx, dy, dz, xmin, zmin, xmax, zmax");
    }
    WedgeArgs args;
    for (std::size_t i = 0; i < 7; ++i) {
        args.dims[i] = require_number_arg(command.args, i, "wedge");
    }
    apply_align_keywords("wedge", command.keywords, args.align);
    return args;
}

struct PlaneArgs {
    std::array<double, 3> origin{0.0, 0.0, 0.0};
    std::array<double, 3> x_axis{1.0, 0.0, 0.0};
    std::array<double, 3> normal{0.0, 0.0, 1.0};
};

std::array<double, 3> require_point3_like_arg(const Arg& arg, const std::string& label);

PlaneArgs plane_args(const Command& command) {
    if (!command.args.empty()) {
        throw EvalError("plane expects no positional arguments");
    }
    PlaneArgs args;
    for (const auto& keyword : command.keywords) {
        if (keyword.kind != Keyword::Kind::Arg) {
            throw EvalError("plane keywords expect arg values only");
        }
        if (keyword.name == "origin") {
            args.origin = require_point3_like_arg(keyword.value, "plane :origin");
            continue;
        }
        if (keyword.name == "x") {
            args.x_axis = require_point3_like_arg(keyword.value, "plane :x");
            continue;
        }
        if (keyword.name == "normal") {
            args.normal = require_point3_like_arg(keyword.value, "plane :normal");
            continue;
        }
        throw EvalError("plane does not recognize `:" + keyword.name + "`");
    }
    return args;
}

std::optional<SelectorPayload> exact_edge_selector(const Command& command, const std::string& op) {
    if (command.keywords.empty()) {
        return std::nullopt;
    }
    if (command.keywords.size() != 1) {
        throw EvalError(op + " supports only one `:edges` selector keyword");
    }
    const Keyword& keyword = command.keywords.front();
    if (keyword.name != "edges") {
        throw EvalError(op + " does not recognize `:" + keyword.name + "`");
    }
    if (keyword.kind != Keyword::Kind::Selector || !keyword.selector_payload.has_value()) {
        throw EvalError(op + " `:edges` requires typed selector payload");
    }
    const SelectorPayload& payload = *keyword.selector_payload;
    if (payload.kind != SelectorKind::Edge) {
        throw EvalError(op + " `:edges` got non-edge selector payload");
    }
    if (payload.type != SelectorPayloadType::TargetIds &&
        payload.type != SelectorPayloadType::Clauses) {
        throw EvalError(op + " `:edges` got unsupported selector payload");
    }
    return payload;
}

std::optional<SelectorPayload> exact_face_selector(const Command& command, const std::string& op) {
    if (command.keywords.empty()) {
        return std::nullopt;
    }
    if (command.keywords.size() != 1) {
        throw EvalError(op + " supports only one `:faces` selector keyword");
    }
    const Keyword& keyword = command.keywords.front();
    if (keyword.name != "faces") {
        throw EvalError(op + " does not recognize `:" + keyword.name + "`");
    }
    if (keyword.kind != Keyword::Kind::Selector || !keyword.selector_payload.has_value()) {
        throw EvalError(op + " `:faces` requires typed selector payload");
    }
    const SelectorPayload& payload = *keyword.selector_payload;
    if (payload.kind != SelectorKind::Face) {
        throw EvalError(op + " `:faces` got non-face selector payload");
    }
    if (payload.type != SelectorPayloadType::TargetIds &&
        payload.type != SelectorPayloadType::Clauses) {
        throw EvalError(op + " `:faces` got unsupported selector payload");
    }
    return payload;
}

std::vector<std::array<double, 2>> require_point2_list(
    const std::vector<Arg>& args,
    std::size_t index,
    const std::string& op,
    std::size_t min_points = 3
) {
    if (index >= args.size() || args[index].kind != Arg::Kind::List) {
        throw EvalError(op + " expects a list of 2D points");
    }
    std::vector<std::array<double, 2>> points;
    for (const Arg& arg : args[index].list_value) {
        points.push_back(require_point2_arg(arg, op));
    }
    if (points.size() < min_points) {
        throw EvalError(op + " expects at least " + std::to_string(min_points) + " points");
    }
    return points;
}

std::vector<double> require_number_list_arg(const Arg& arg, const std::string& label) {
    if (arg.kind == Arg::Kind::Point2) {
        return {arg.point2_value[0], arg.point2_value[1]};
    }
    if (arg.kind == Arg::Kind::Point3) {
        return {arg.point3_value[0], arg.point3_value[1], arg.point3_value[2]};
    }
    if (arg.kind != Arg::Kind::List) {
        throw EvalError(label + " expects number list");
    }
    std::vector<double> values;
    values.reserve(arg.list_value.size());
    for (const Arg& item : arg.list_value) {
        if (item.kind != Arg::Kind::Number) {
            throw EvalError(label + " expects number list");
        }
        values.push_back(item.number_value);
    }
    return values;
}

std::vector<std::array<double, 2>> require_point2_list_arg(const Arg& arg, const std::string& label, std::size_t min_points) {
    if (arg.kind != Arg::Kind::List) {
        throw EvalError(label + " expects point2 list");
    }
    std::vector<std::array<double, 2>> points;
    points.reserve(arg.list_value.size());
    for (const Arg& item : arg.list_value) {
        points.push_back(require_point2_arg(item, label));
    }
    if (points.size() < min_points) {
        throw EvalError(label + " expects at least " + std::to_string(min_points) + " points");
    }
    return points;
}

std::array<double, 3> require_point3_arg(const Arg& arg, const std::string& label) {
    if (arg.kind != Arg::Kind::Point3) {
        throw EvalError(label + " expects point3 value");
    }
    return arg.point3_value;
}

std::array<double, 3> require_point3_like_arg(const Arg& arg, const std::string& label) {
    if (arg.kind == Arg::Kind::Point3) {
        return arg.point3_value;
    }
    if (arg.kind == Arg::Kind::List && arg.list_value.size() == 3) {
        std::array<double, 3> point{};
        for (std::size_t index = 0; index < point.size(); ++index) {
            if (arg.list_value[index].kind != Arg::Kind::Number) {
                throw EvalError(label + " expects point3 or three-number list value");
            }
            point[index] = arg.list_value[index].number_value;
        }
        return point;
    }
    throw EvalError(label + " expects point3 or three-number list value");
}

std::vector<std::array<double, 3>> require_point3_sequence(
    const std::vector<Arg>& args,
    const std::string& op
) {
    const std::vector<Arg>* items = &args;
    if (args.size() == 1 && args[0].kind == Arg::Kind::List) {
        items = &args[0].list_value;
    }
    std::vector<std::array<double, 3>> points;
    for (const Arg& arg : *items) {
        if (arg.kind != Arg::Kind::Point3) {
            throw EvalError(op + " expects point3 values");
        }
        points.push_back(arg.point3_value);
    }
    if (points.size() < 2) {
        throw EvalError(op + " expects at least two points");
    }
    return points;
}

double selector_axis_min(
    SelectorAxis axis,
    double xmin,
    double ymin,
    double zmin
);

double selector_axis_max(
    SelectorAxis axis,
    double xmax,
    double ymax,
    double zmax
);

double distance2(const std::array<double, 2>& left, const std::array<double, 2>& right) {
    double dx = left[0] - right[0];
    double dy = left[1] - right[1];
    return std::sqrt(dx * dx + dy * dy);
}

std::array<double, 2> add2(const std::array<double, 2>& left, const std::array<double, 2>& right) {
    return {left[0] + right[0], left[1] + right[1]};
}

std::array<double, 2> sub2(const std::array<double, 2>& left, const std::array<double, 2>& right) {
    return {left[0] - right[0], left[1] - right[1]};
}

std::array<double, 2> mul2(const std::array<double, 2>& point, double scalar) {
    return {point[0] * scalar, point[1] * scalar};
}

double dot2(const std::array<double, 2>& left, const std::array<double, 2>& right) {
    return left[0] * right[0] + left[1] * right[1];
}

double length2(const std::array<double, 2>& point) {
    return std::sqrt(point[0] * point[0] + point[1] * point[1]);
}

TopoDS_Shape make_polygon_face(const std::vector<std::array<double, 2>>& points) {
    if (points.size() < 3) {
        throw EvalError("polygon expects at least three points");
    }
    BRepBuilderAPI_MakePolygon polygon;
    for (const auto& point : points) {
        polygon.Add(gp_Pnt(point[0], point[1], 0.0));
    }
    polygon.Close();
    return BRepBuilderAPI_MakeFace(polygon.Wire()).Shape();
}

TopoDS_Wire first_wire(const TopoDS_Shape& shape, const std::string& op) {
    for (TopExp_Explorer explorer(shape, TopAbs_WIRE); explorer.More(); explorer.Next()) {
        return TopoDS::Wire(explorer.Current());
    }
    throw EvalError(op + " expects a wire/profile shape");
}

TopoDS_Shape make_face_from_shape(const TopoDS_Shape& shape, const std::string& op) {
    BRepBuilderAPI_MakeWire wire_builder;
    bool has_profile_edge = false;
    for (TopExp_Explorer explorer(shape, TopAbs_WIRE); explorer.More(); explorer.Next()) {
        wire_builder.Add(TopoDS::Wire(explorer.Current()));
        has_profile_edge = true;
    }
    if (!has_profile_edge) {
        for (TopExp_Explorer explorer(shape, TopAbs_EDGE); explorer.More(); explorer.Next()) {
            wire_builder.Add(TopoDS::Edge(explorer.Current()));
            has_profile_edge = true;
        }
    }
    if (!has_profile_edge) {
        throw EvalError(op + " expects a wire/profile shape");
    }
    if (!wire_builder.IsDone()) {
        throw EvalError(op + " could not assemble profile wire");
    }
    BRepBuilderAPI_MakeFace face(wire_builder.Wire());
    if (!face.IsDone()) {
        throw EvalError(op + " could not build face");
    }
    return face.Shape();
}

gp_Pnt wire_sample_point(const TopoDS_Wire& wire, const std::string& op) {
    for (TopExp_Explorer explorer(wire, TopAbs_EDGE); explorer.More(); explorer.Next()) {
        BRepAdaptor_Curve curve(TopoDS::Edge(explorer.Current()));
        double first_param = curve.FirstParameter();
        double last_param = curve.LastParameter();
        if (!std::isfinite(first_param) || !std::isfinite(last_param)) {
            continue;
        }
        return curve.Value((first_param + last_param) / 2.0);
    }
    throw EvalError(op + " expects wire with at least one edge");
}

double face_area(const TopoDS_Face& face) {
    GProp_GProps props;
    BRepGProp::SurfaceProperties(face, props);
    return std::abs(props.Mass());
}

// ocpsvg parity (`ensure_face_normal_up`): profile faces must present +Z
// normals before extrusion, or the prism comes out inverted and silently
// poisons downstream boolean operations.
TopoDS_Face ensure_face_normal_up(TopoDS_Face face) {
    double u_min = 0.0;
    double u_max = 0.0;
    double v_min = 0.0;
    double v_max = 0.0;
    BRepTools::UVBounds(face, u_min, u_max, v_min, v_max);
    if (!std::isfinite(u_min) || !std::isfinite(u_max) ||
        !std::isfinite(v_min) || !std::isfinite(v_max)) {
        return face;
    }
    BRepAdaptor_Surface surface(face);
    gp_Pnt point;
    gp_Vec du;
    gp_Vec dv;
    surface.D1((u_min + u_max) / 2.0, (v_min + v_max) / 2.0, point, du, dv);
    gp_Vec normal = du.Crossed(dv);
    if (normal.Magnitude() <= 1.0e-9) {
        return face;
    }
    if (face.Orientation() == TopAbs_REVERSED) {
        normal.Reverse();
    }
    if (normal.Z() < 0.0) {
        face.Reverse();
    }
    return face;
}

// Whole-wire containment test: every vertex of the wire must classify
// IN/ON against the candidate face. Uses `theUseBndBox=true` (recommended
// by OCCT for faces with >10 edges) to keep dense SVG polyline tests fast.
bool wire_inside_face(const TopoDS_Wire& wire, const TopoDS_Face& face) {
    bool saw_vertex = false;
    BRepClass_FaceClassifier classifier;
    for (TopExp_Explorer explorer(wire, TopAbs_VERTEX); explorer.More(); explorer.Next()) {
        saw_vertex = true;
        gp_Pnt point = BRep_Tool::Pnt(TopoDS::Vertex(explorer.Current()));
        classifier.Perform(face, point, 1.0e-7, Standard_True);
        TopAbs_State state = classifier.State();
        if (state != TopAbs_IN && state != TopAbs_ON) {
            return false;
        }
    }
    return saw_vertex;
}

// Resolve a raw SVG wire soup into planar faces. First choice is OCCT's
// canonical BOPAlgo_Tools::WiresToFaces — it splits intersecting/overlapping
// wires and resolves hole nesting itself, which the hand-rolled per-wire
// MakeFace + containment-parity below cannot do for self-intersecting artwork
// (real lineart icons produced thousands of non-manifold edges and swallowed
// downstream fuses). The hand-rolled path stays as a fallback for soups
// WiresToFaces cannot face.
TopoDS_Shape make_faces_from_wire_soup(
    const std::vector<TopoDS_Shape>& wire_shapes
) {
    struct FacedWire {
        TopoDS_Wire wire;
        TopoDS_Face face;
        double area = 0.0;
    };
    std::vector<FacedWire> faced;
    faced.reserve(wire_shapes.size());
    for (const auto& shape : wire_shapes) {
        try {
            TopoDS_Wire wire = first_wire(shape, "profile");
            BRepBuilderAPI_MakeFace builder(wire, Standard_True);
            if (!builder.IsDone()) {
                continue;
            }
            TopoDS_Face face = TopoDS::Face(builder.Shape());
            ShapeFix_Face fixer(face);
            fixer.Perform();
            fixer.FixOrientation();
            face = fixer.Face();
            if (!BRepCheck_Analyzer(face).IsValid()) {
                continue;
            }
            FacedWire entry;
            entry.wire = wire;
            entry.face = face;
            entry.area = face_area(face);
            faced.push_back(entry);
        } catch (...) {
            continue;
        }
    }
    const bool can_classify_fill = faced.size() == wire_shapes.size();
    {
        // 1. General-fuse every edge against every other: self-intersecting
        //    and mutually intersecting artwork contours get split at their
        //    crossing points (lineart icons — the case per-wire MakeFace
        //    cannot represent).
        BOPAlgo_Builder splitter;
        int edge_count = 0;
        for (const auto& shape : wire_shapes) {
            for (TopExp_Explorer explorer(shape, TopAbs_EDGE); explorer.More(); explorer.Next()) {
                splitter.AddArgument(explorer.Current());
                ++edge_count;
            }
        }
        if (edge_count > 0) {
            try {
                splitter.Perform();
                if (!splitter.HasErrors()) {
                    // 2. Chain the split edges back into closed wires and let
                    //    OCCT build the planar region faces with hole nesting.
                    TopoDS_Shape wires_shape;
                    BOPAlgo_Tools::EdgesToWires(splitter.Shape(), wires_shape, Standard_False);
                    TopoDS_Shape faces_shape;
                    if (BOPAlgo_Tools::WiresToFaces(wires_shape, faces_shape)) {
                        // WiresToFaces returns every atomic region. Keep only
                        // regions covered by an odd number of source contours;
                        // otherwise nested glyph counters become material.
                        TopoDS_Shape filled_faces = faces_shape;
                        if (can_classify_fill) {
                            BRep_Builder filled_builder;
                            TopoDS_Compound filled_compound;
                            filled_builder.MakeCompound(filled_compound);
                            int filled_count = 0;
                            for (TopExp_Explorer explorer(faces_shape, TopAbs_FACE);
                                 explorer.More(); explorer.Next()) {
                                TopoDS_Face candidate = TopoDS::Face(explorer.Current());
                                TopoDS_Wire outer = BRepTools::OuterWire(candidate);
                                int coverage = 0;
                                for (const auto& source : faced) {
                                    if (wire_inside_face(outer, source.face)) {
                                        ++coverage;
                                    }
                                }
                                if (coverage % 2 != 0) {
                                    filled_builder.Add(filled_compound, candidate);
                                    ++filled_count;
                                }
                            }
                            if (filled_count > 0) {
                                filled_faces = filled_compound;
                            }
                        }
                        // 3. Selected material regions share seam edges
                        //    from the arrangement; unify them into one face so
                        //    the per-region prisms don't fight over coincident
                        //    walls in later booleans. Unify (not fuse!) keeps
                        //    hole rings intact.
                        ShapeUpgrade_UnifySameDomain unify(
                            filled_faces, Standard_True, Standard_True, Standard_False);
                        unify.Build();
                        TopoDS_Shape unified = unify.Shape();
                        if (unified.IsNull()) {
                            unified = filled_faces;
                        }
                        std::vector<TopoDS_Shape> region_faces;
                        for (TopExp_Explorer explorer(unified, TopAbs_FACE); explorer.More();
                             explorer.Next()) {
                            region_faces.push_back(
                                ensure_face_normal_up(TopoDS::Face(explorer.Current())));
                        }
                        if (region_faces.size() == 1) {
                            return region_faces.front();
                        }
                        if (!region_faces.empty()) {
                            BRep_Builder faces_builder;
                            TopoDS_Compound face_compound;
                            faces_builder.MakeCompound(face_compound);
                            for (const auto& face : region_faces) {
                                faces_builder.Add(face_compound, face);
                            }
                            return face_compound;
                        }
                    }
                }
            } catch (...) {
            }
        }
    }
    if (faced.empty()) {
        throw EvalError("profile wire soup produced no faceable regions");
    }

    const std::size_t n = faced.size();
    std::vector<int> depth(n, 0);
    std::vector<std::optional<std::size_t>> parent(n);
    for (std::size_t i = 0; i < n; ++i) {
        double parent_area = 0.0;
        for (std::size_t j = 0; j < n; ++j) {
            if (i == j) {
                continue;
            }
            if (!wire_inside_face(faced[i].wire, faced[j].face)) {
                continue;
            }
            depth[i] += 1;
            if (!parent[i].has_value() || faced[j].area < parent_area) {
                parent[i] = j;
                parent_area = faced[j].area;
            }
        }
    }

    std::vector<TopoDS_Shape> faces;
    for (std::size_t i = 0; i < n; ++i) {
        if (depth[i] % 2 != 0) {
            continue;
        }
        try {
        BRepBuilderAPI_MakeFace face_builder(faced[i].wire, Standard_True);
        if (!face_builder.IsDone()) {
            continue;
        }
        for (std::size_t j = 0; j < n; ++j) {
            if (depth[j] % 2 == 0 || !parent[j].has_value() || *parent[j] != i) {
                continue;
            }
            face_builder.Add(TopoDS::Wire(faced[j].wire.Reversed()));
        }
        TopoDS_Face region = TopoDS::Face(face_builder.Shape());
        ShapeFix_Face region_fixer(region);
        region_fixer.Perform();
        region_fixer.FixOrientation();
        faces.push_back(ensure_face_normal_up(region_fixer.Face()));
        } catch (...) {
            continue;
        }
    }
    if (faces.empty()) {
        throw EvalError("profile wire soup produced no regions");
    }
    if (faces.size() == 1) {
        return faces.front();
    }
    BRep_Builder builder;
    TopoDS_Compound compound;
    builder.MakeCompound(compound);
    for (const auto& face : faces) {
        builder.Add(compound, face);
    }
    return compound;
}

TopoDS_Shape make_profile_face(
    const std::vector<TopoDS_Shape>& outer_shapes,
    const std::vector<TopoDS_Shape>& hole_shapes
) {
    if (outer_shapes.empty()) {
        throw EvalError("profile needs at least one outer loop");
    }
    std::vector<TopoDS_Wire> outer_wires;
    std::vector<TopoDS_Face> outer_faces;
    std::vector<double> outer_areas;
    std::vector<std::vector<TopoDS_Wire>> hole_wires_by_outer;
    outer_wires.reserve(outer_shapes.size());
    outer_faces.reserve(outer_shapes.size());
    outer_areas.reserve(outer_shapes.size());
    hole_wires_by_outer.resize(outer_shapes.size());
    for (const auto& outer_shape : outer_shapes) {
        TopoDS_Wire outer_wire = first_wire(outer_shape, "profile");
        BRepBuilderAPI_MakeFace outer_face_builder(outer_wire);
        if (!outer_face_builder.IsDone()) {
            throw EvalError("profile could not build outer face");
        }
        TopoDS_Face outer_face = TopoDS::Face(outer_face_builder.Shape());
        outer_wires.push_back(outer_wire);
        outer_faces.push_back(outer_face);
        outer_areas.push_back(face_area(outer_face));
    }
    for (const auto& hole_shape : hole_shapes) {
        TopoDS_Wire hole_wire = first_wire(hole_shape, "profile");
        gp_Pnt sample = wire_sample_point(hole_wire, "profile");
        std::optional<std::size_t> matched_outer;
        double matched_area = 0.0;
        for (std::size_t index = 0; index < outer_faces.size(); ++index) {
            BRepClass_FaceClassifier classifier(outer_faces[index], sample, 1.0e-7);
            TopAbs_State state = classifier.State();
            if (state != TopAbs_IN && state != TopAbs_ON) {
                continue;
            }
            if (!matched_outer.has_value() || outer_areas[index] < matched_area) {
                matched_outer = index;
                matched_area = outer_areas[index];
            }
        }
        if (!matched_outer.has_value()) {
            throw EvalError("profile hole does not lie inside any outer loop");
        }
        hole_wires_by_outer[*matched_outer].push_back(hole_wire);
    }

    std::vector<TopoDS_Shape> faces;
    faces.reserve(outer_wires.size());
    for (std::size_t index = 0; index < outer_wires.size(); ++index) {
        BRepBuilderAPI_MakeFace face_builder(outer_wires[index]);
        if (!face_builder.IsDone()) {
            throw EvalError("profile could not build outer face");
        }
        for (const auto& hole_wire : hole_wires_by_outer[index]) {
            face_builder.Add(TopoDS::Wire(hole_wire.Reversed()));
        }
        // ocpsvg parity: outer/hole wires arrive in arbitrary winding (font
        // glyph counters are the usual offender). ShapeFix_Face repairs ring
        // orientation; ensure_face_normal_up keeps the +Z normal so the
        // extruded prism is not inverted — an inverted prism silently swallows
        // the other operand of a later fuse.
        TopoDS_Face oriented_face = TopoDS::Face(face_builder.Shape());
        ShapeFix_Face face_fixer(oriented_face);
        face_fixer.Perform();
        face_fixer.FixOrientation();
        faces.push_back(ensure_face_normal_up(face_fixer.Face()));
    }
    if (faces.size() == 1) {
        return faces.front();
    }

    BRep_Builder builder;
    TopoDS_Compound compound;
    builder.MakeCompound(compound);
    for (const auto& face : faces) {
        builder.Add(compound, face);
    }
    return compound;
}

TopoDS_Shape make_circle_face(double radius) {
    gp_Circ circle(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), radius);
    TopoDS_Wire wire = BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge(circle).Edge()).Wire();
    return BRepBuilderAPI_MakeFace(wire).Shape();
}

TopoDS_Shape make_slot_face(double length, double width) {
    double r = width / 2.0;
    double half = (length - width) / 2.0;
    BRepBuilderAPI_MakeWire builder;
    builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt(-half, -r, 0), gp_Pnt(half, -r, 0)).Edge());
    builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt(half, -r, 0), gp_Pnt(half + r, 0, 0), gp_Pnt(half, r, 0)).Value()).Edge());
    builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt(half, r, 0), gp_Pnt(-half, r, 0)).Edge());
    builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(gp_Pnt(-half, r, 0), gp_Pnt(-half - r, 0, 0), gp_Pnt(-half, -r, 0)).Value()).Edge());
    TopoDS_Wire wire = builder.Wire();
    return BRepBuilderAPI_MakeFace(wire).Shape();
}

TopoDS_Shape make_slot_arc_face(double radius, double start_deg, double end_deg, double width) {
    double r = width / 2.0;
    double ro = radius + r;
    double ri = radius - r;
    double a0 = start_deg * M_PI / 180.0;
    double a1 = end_deg * M_PI / 180.0;
    double am = (a0 + a1) / 2.0;
    auto pt = [](double rad, double ang) {
        return gp_Pnt(rad * std::cos(ang), rad * std::sin(ang), 0);
    };
    gp_Pnt cap1(radius * std::cos(a1) - r * std::sin(a1), radius * std::sin(a1) + r * std::cos(a1), 0);
    gp_Pnt cap0(radius * std::cos(a0) + r * std::sin(a0), radius * std::sin(a0) - r * std::cos(a0), 0);
    BRepBuilderAPI_MakeWire builder;
    builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(pt(ro, a0), pt(ro, am), pt(ro, a1)).Value()).Edge());
    builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(pt(ro, a1), cap1, pt(ri, a1)).Value()).Edge());
    builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(pt(ri, a1), pt(ri, am), pt(ri, a0)).Value()).Edge());
    builder.Add(BRepBuilderAPI_MakeEdge(GC_MakeArcOfCircle(pt(ri, a0), cap0, pt(ro, a0)).Value()).Edge());
    TopoDS_Wire wire = builder.Wire();
    return BRepBuilderAPI_MakeFace(wire).Shape();
}

TopoDS_Shape make_ellipse_face(double rx, double ry) {
    gp_Ax2 axes = (rx >= ry)
        ? gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1), gp_Dir(1, 0, 0))
        : gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1), gp_Dir(0, 1, 0));
    gp_Elips ellipse(axes, std::max(rx, ry), std::min(rx, ry));
    TopoDS_Wire wire = BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge(ellipse).Edge()).Wire();
    return BRepBuilderAPI_MakeFace(wire).Shape();
}

TopoDS_Shape make_rounded_rect_face(double width, double height, double radius) {
    double r = std::min(std::abs(radius), std::min(std::abs(width) / 2.0, std::abs(height) / 2.0));
    double x0 = -width / 2.0;
    double y0 = -height / 2.0;
    double x1 = width / 2.0;
    double y1 = height / 2.0;
    if (r <= 1.0e-12) {
        return make_polygon_face({{x0, y0}, {x1, y0}, {x1, y1}, {x0, y1}});
    }
    double arc_mid = r * std::sqrt(0.5);
    BRepBuilderAPI_MakeWire wire_builder;
    auto add_line_if_distinct = [&](const gp_Pnt& start, const gp_Pnt& end) {
        if (start.SquareDistance(end) > 1.0e-24) {
            wire_builder.Add(BRepBuilderAPI_MakeEdge(start, end).Edge());
        }
    };
    add_line_if_distinct(gp_Pnt(x0 + r, y0, 0), gp_Pnt(x1 - r, y0, 0));
    wire_builder.Add(BRepBuilderAPI_MakeEdge(
                         GC_MakeArcOfCircle(gp_Pnt(x1 - r, y0, 0),
                                            gp_Pnt(x1 - r + arc_mid, y0 + r - arc_mid, 0),
                                            gp_Pnt(x1, y0 + r, 0))
                             .Value())
                         .Edge());
    add_line_if_distinct(gp_Pnt(x1, y0 + r, 0), gp_Pnt(x1, y1 - r, 0));
    wire_builder.Add(BRepBuilderAPI_MakeEdge(
                         GC_MakeArcOfCircle(gp_Pnt(x1, y1 - r, 0),
                                            gp_Pnt(x1 - r + arc_mid, y1 - r + arc_mid, 0),
                                            gp_Pnt(x1 - r, y1, 0))
                             .Value())
                         .Edge());
    add_line_if_distinct(gp_Pnt(x1 - r, y1, 0), gp_Pnt(x0 + r, y1, 0));
    wire_builder.Add(BRepBuilderAPI_MakeEdge(
                         GC_MakeArcOfCircle(gp_Pnt(x0 + r, y1, 0),
                                            gp_Pnt(x0 + r - arc_mid, y1 - r + arc_mid, 0),
                                            gp_Pnt(x0, y1 - r, 0))
                             .Value())
                         .Edge());
    add_line_if_distinct(gp_Pnt(x0, y1 - r, 0), gp_Pnt(x0, y0 + r, 0));
    wire_builder.Add(BRepBuilderAPI_MakeEdge(
                         GC_MakeArcOfCircle(gp_Pnt(x0, y0 + r, 0),
                                            gp_Pnt(x0 + r - arc_mid, y0 + r - arc_mid, 0),
                                            gp_Pnt(x0 + r, y0, 0))
                             .Value())
                         .Edge());
    return BRepBuilderAPI_MakeFace(wire_builder.Wire()).Shape();
}

struct RoundedCorner {
    std::array<double, 2> p_in{0.0, 0.0};
    std::array<double, 2> p_out{0.0, 0.0};
    std::array<double, 2> mid{0.0, 0.0};
    bool rounded = false;
};

std::vector<std::array<double, 2>> normalize_closed_points(
    const std::vector<std::array<double, 2>>& points,
    const std::string& op
) {
    if (points.size() < 3) {
        throw EvalError(op + " expects at least three points");
    }
    std::vector<std::array<double, 2>> normalized = points;
    if (normalized.size() >= 2 && distance2(normalized.front(), normalized.back()) <= 1.0e-12) {
        normalized.pop_back();
    }
    if (normalized.size() < 3) {
        throw EvalError(op + " expects at least three points");
    }
    return normalized;
}

std::vector<RoundedCorner> rounded_polygon_corners(
    const std::vector<std::array<double, 2>>& raw_points,
    double radius,
    const std::string& op
) {
    std::vector<std::array<double, 2>> points = normalize_closed_points(raw_points, op);
    double requested_radius = std::abs(radius);
    std::vector<RoundedCorner> corners;
    corners.reserve(points.size());
    if (requested_radius <= 1.0e-12) {
        for (const auto& point : points) {
            corners.push_back({point, point, point, false});
        }
        return corners;
    }

    std::size_t count = points.size();
    for (std::size_t index = 0; index < count; ++index) {
        auto prev = points[(index + count - 1) % count];
        auto curr = points[index];
        auto next = points[(index + 1) % count];
        auto in_vec = sub2(prev, curr);
        auto out_vec = sub2(next, curr);
        double len_in = length2(in_vec);
        double len_out = length2(out_vec);
        if (len_in <= 1.0e-12 || len_out <= 1.0e-12) {
            throw EvalError(op + " got a zero-length edge");
        }
        auto in_dir = mul2(in_vec, 1.0 / len_in);
        auto out_dir = mul2(out_vec, 1.0 / len_out);
        double dot = std::clamp(dot2(in_dir, out_dir), -1.0, 1.0);
        double theta = std::acos(dot);
        double tan_half = theta > 1.0e-12 ? std::tan(theta / 2.0) : 0.0;
        auto bisector = add2(in_dir, out_dir);
        double bisector_len = length2(bisector);
        if (tan_half <= 1.0e-12 || bisector_len <= 1.0e-12) {
            corners.push_back({curr, curr, curr, false});
            continue;
        }
        double corner_radius = std::min(requested_radius, std::min(len_in, len_out) * tan_half);
        if (corner_radius <= 1.0e-12) {
            corners.push_back({curr, curr, curr, false});
            continue;
        }
        double tangent = corner_radius / tan_half;
        bisector = mul2(bisector, 1.0 / bisector_len);
        double center_dist = corner_radius / std::sin(theta / 2.0);
        auto p_in = add2(curr, mul2(in_dir, tangent));
        auto p_out = add2(curr, mul2(out_dir, tangent));
        auto center = add2(curr, mul2(bisector, center_dist));
        auto mid_dir = sub2(curr, center);
        double mid_len = length2(mid_dir);
        if (mid_len <= 1.0e-12) {
            corners.push_back({curr, curr, curr, false});
            continue;
        }
        auto mid = add2(center, mul2(mid_dir, corner_radius / mid_len));
        corners.push_back({p_in, p_out, mid, true});
    }
    return corners;
}

TopoDS_Shape make_rounded_polygon_face(const std::vector<std::array<double, 2>>& points, double radius) {
    std::vector<RoundedCorner> corners = rounded_polygon_corners(points, radius, "rounded-polygon");
    bool any_rounded = false;
    for (const auto& corner : corners) {
        any_rounded = any_rounded || corner.rounded;
    }
    if (!any_rounded) {
        return make_polygon_face(normalize_closed_points(points, "rounded-polygon"));
    }
    BRepBuilderAPI_MakeWire wire_builder;
    for (std::size_t index = 0; index < corners.size(); ++index) {
        const RoundedCorner& current = corners[index];
        const RoundedCorner& next = corners[(index + 1) % corners.size()];
        if (distance2(current.p_out, next.p_in) > 1.0e-9) {
            wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt(current.p_out[0], current.p_out[1], 0),
                                                     gp_Pnt(next.p_in[0], next.p_in[1], 0))
                                 .Edge());
        }
        if (next.rounded) {
            wire_builder.Add(BRepBuilderAPI_MakeEdge(
                                 GC_MakeArcOfCircle(gp_Pnt(next.p_in[0], next.p_in[1], 0),
                                                    gp_Pnt(next.mid[0], next.mid[1], 0),
                                                    gp_Pnt(next.p_out[0], next.p_out[1], 0))
                                     .Value())
                                 .Edge());
        }
    }
    return BRepBuilderAPI_MakeFace(wire_builder.Wire()).Shape();
}

TopoDS_Shape make_box(double width, double depth, double height, const std::array<AlignMode, 3>& align) {
    TopoDS_Shape shape = BRepPrimAPI_MakeBox(width, depth, height).Shape();
    double tx = align_offset(width, align[0]);
    double ty = align_offset(depth, align[1]);
    double tz = align_offset(height, align[2]);
    if (std::abs(tx) <= 1.0e-12 && std::abs(ty) <= 1.0e-12 && std::abs(tz) <= 1.0e-12) {
        return shape;
    }
    gp_Trsf trsf;
    trsf.SetTranslation(gp_Vec(tx, ty, tz));
    return BRepBuilderAPI_Transform(shape, trsf, true).Shape();
}

TopoDS_Shape make_sphere(double radius, const std::array<AlignMode, 3>& align) {
    TopoDS_Shape shape = BRepPrimAPI_MakeSphere(radius).Shape();
    double span = radius * 2.0;
    double tx = centered_align_offset(span, align[0]);
    double ty = centered_align_offset(span, align[1]);
    double tz = centered_align_offset(span, align[2]);
    if (std::abs(tx) <= 1.0e-12 && std::abs(ty) <= 1.0e-12 && std::abs(tz) <= 1.0e-12) {
        return shape;
    }
    gp_Trsf trsf;
    trsf.SetTranslation(gp_Vec(tx, ty, tz));
    return BRepBuilderAPI_Transform(shape, trsf, true).Shape();
}

TopoDS_Shape make_cylinder(double radius, double height, const std::array<AlignMode, 3>& align) {
    TopoDS_Shape shape = BRepPrimAPI_MakeCylinder(radius, height).Shape();
    double span = radius * 2.0;
    double tx = centered_align_offset(span, align[0]);
    double ty = centered_align_offset(span, align[1]);
    double tz = align_offset(height, align[2]);
    if (std::abs(tx) <= 1.0e-12 && std::abs(ty) <= 1.0e-12 && std::abs(tz) <= 1.0e-12) {
        return shape;
    }
    gp_Trsf trsf;
    trsf.SetTranslation(gp_Vec(tx, ty, tz));
    return BRepBuilderAPI_Transform(shape, trsf, true).Shape();
}

TopoDS_Shape make_cone(
    double radius1,
    double radius2,
    double height,
    const std::array<AlignMode, 3>& align
) {
    TopoDS_Shape shape = BRepPrimAPI_MakeCone(radius1, radius2, height).Shape();
    double span = std::max(radius1, radius2) * 2.0;
    double tx = centered_align_offset(span, align[0]);
    double ty = centered_align_offset(span, align[1]);
    double tz = align_offset(height, align[2]);
    if (std::abs(tx) <= 1.0e-12 && std::abs(ty) <= 1.0e-12 && std::abs(tz) <= 1.0e-12) {
        return shape;
    }
    gp_Trsf trsf;
    trsf.SetTranslation(gp_Vec(tx, ty, tz));
    return BRepBuilderAPI_Transform(shape, trsf, true).Shape();
}

TopoDS_Shape make_torus(
    double major,
    double minor,
    const std::array<AlignMode, 3>& align
) {
    TopoDS_Shape shape = BRepPrimAPI_MakeTorus(major, minor).Shape();
    double span = (major + minor) * 2.0;
    double tx = centered_align_offset(span, align[0]);
    double ty = centered_align_offset(span, align[1]);
    double tz = centered_align_offset(minor * 2.0, align[2]);
    if (std::abs(tx) <= 1.0e-12 && std::abs(ty) <= 1.0e-12 && std::abs(tz) <= 1.0e-12) {
        return shape;
    }
    gp_Trsf trsf;
    trsf.SetTranslation(gp_Vec(tx, ty, tz));
    return BRepBuilderAPI_Transform(shape, trsf, true).Shape();
}

TopoDS_Shape make_wedge(
    const std::array<double, 7>& dims,
    const std::array<AlignMode, 3>& align
) {
    double dx = dims[0], dy = dims[1], dz = dims[2];
    double xmin = dims[3], zmin = dims[4], xmax = dims[5], zmax = dims[6];
    TopoDS_Shape shape = BRepPrimAPI_MakeWedge(dx, dy, dz, xmin, zmin, xmax, zmax).Shape();
    double tx = align_offset(dx, align[0]);
    double ty = align_offset(dy, align[1]);
    double tz = align_offset(dz, align[2]);
    if (std::abs(tx) <= 1.0e-12 && std::abs(ty) <= 1.0e-12 && std::abs(tz) <= 1.0e-12) {
        return shape;
    }
    gp_Trsf trsf;
    trsf.SetTranslation(gp_Vec(tx, ty, tz));
    return BRepBuilderAPI_Transform(shape, trsf, true).Shape();
}

TopoDS_Shape compound_shapes(const std::vector<TopoDS_Shape>& shapes);

TopoDS_Shape extrude_shape(const TopoDS_Shape& input_shape, double height, bool symmetric) {
    TopoDS_Shape shape = input_shape;
    if (symmetric) {
        gp_Trsf center_trsf;
        center_trsf.SetTranslation(gp_Vec(0, 0, -height / 2.0));
        shape = BRepBuilderAPI_Transform(shape, center_trsf, true).Shape();
    }
    // Multi-region profiles (glyph text, SVG artwork soup) arrive as compounds
    // whose faces may overlap. Prism of the raw compound keeps the overlapping
    // solids side by side and poisons downstream booleans; extrude per face
    // and fuse the prisms into one valid solid instead (build123d parity).
    std::vector<TopoDS_Shape> profile_faces;
    if (shape.ShapeType() == TopAbs_COMPOUND) {
        for (TopExp_Explorer face_explorer(shape, TopAbs_FACE); face_explorer.More();
             face_explorer.Next()) {
            profile_faces.push_back(face_explorer.Current());
        }
    }
    if (profile_faces.size() > 1) {
        // Merge overlapping regions in 2D before extruding: fusing overlapping
        // *prisms* leaves coincident-but-unshared seam edges in the coplanar
        // cap faces (hairline non-manifold cracks in the STL). Fusing the
        // planar faces first and unifying the seam (build123d `clean` parity)
        // gives each merged region a single clean cap.
        TopoDS_Shape merged = profile_faces.front();
        for (std::size_t face_index = 1; face_index < profile_faces.size(); ++face_index) {
            BRepAlgoAPI_Fuse region_fuse(merged, profile_faces[face_index]);
            if (!region_fuse.IsDone()) {
                throw EvalError("extrude failed to merge overlapping profile regions");
            }
            merged = region_fuse.Shape();
        }
        ShapeUpgrade_UnifySameDomain unify(merged, Standard_True, Standard_True, Standard_False);
        unify.Build();
        merged = unify.Shape();

        std::vector<TopoDS_Shape> region_faces;
        for (TopExp_Explorer region_explorer(merged, TopAbs_FACE); region_explorer.More();
             region_explorer.Next()) {
            region_faces.push_back(
                ensure_face_normal_up(TopoDS::Face(region_explorer.Current())));
        }
        if (region_faces.empty()) {
            throw EvalError("extrude found no faces in multi-region profile");
        }
        // After the 2D merge the regions are disjoint (overlaps were fused in
        // the plane). Keep the prisms as a compound: a boolean fuse of the
        // disjoint prisms rebuilds every face and leaves cap/wall boundary
        // edges duplicated instead of shared — later transforms drift the
        // duplicates a few ULPs apart and the STL grows hairline non-manifold
        // cracks along entire glyph outlines.
        std::vector<TopoDS_Shape> prisms;
        prisms.reserve(region_faces.size());
        for (const auto& region_face : region_faces) {
            prisms.push_back(BRepPrimAPI_MakePrism(region_face, gp_Vec(0, 0, height)).Shape());
        }
        if (prisms.size() == 1) {
            return prisms.front();
        }
        return compound_shapes(prisms);
    }
    return BRepPrimAPI_MakePrism(shape, gp_Vec(0, 0, height)).Shape();
}

TopoDS_Shape revolve_shape(const TopoDS_Shape& shape, double angle_degrees) {
    gp_Trsf profile_trsf;
    profile_trsf.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(1, 0, 0)), 1.5707963267948966);
    TopoDS_Shape profile = BRepBuilderAPI_Transform(shape, profile_trsf, true).Shape();
    return BRepPrimAPI_MakeRevol(profile, gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)),
                                 angle_degrees * M_PI / 180.0)
        .Shape();
}

TopoDS_Shape loft_shapes(double distance, const std::vector<TopoDS_Shape>& profiles) {
    if (profiles.size() < 2) {
        throw EvalError("loft requires at least two profiles");
    }
    double denominator = static_cast<double>(profiles.size() - 1);
    BRepOffsetAPI_ThruSections loft(true, false, 1.0e-6);
    for (std::size_t index = 0; index < profiles.size(); ++index) {
        gp_Trsf trsf;
        trsf.SetTranslation(gp_Vec(0, 0, distance * static_cast<double>(index) / denominator));
        TopoDS_Shape section_shape = BRepBuilderAPI_Transform(profiles[index], trsf, true).Shape();
        loft.AddWire(first_wire(section_shape, "loft"));
    }
    loft.Build();
    if (!loft.IsDone()) {
        throw EvalError("loft failed to build");
    }
    return loft.Shape();
}

// True when the shape contains at least one TopAbs_SOLID. Booleans
// (BRepAlgoAPI_Common/Cut/Fuse) only behave on solids; an open shell that
// slips through silently produces empty results downstream.
bool shape_has_solid(const TopoDS_Shape& shape) {
    TopExp_Explorer it(shape, TopAbs_SOLID);
    return it.More();
}

// Best-effort conversion of a closed-but-unmarked shell into a solid: sew the
// faces, then wrap each resulting closed shell in a solid. Returns the original
// shape if no shell could be solidified.
TopoDS_Shape solidify_swept_shell(const TopoDS_Shape& shape) {
    BRepBuilderAPI_Sewing sewer(1.0e-6);
    int face_count = 0;
    for (TopExp_Explorer it(shape, TopAbs_FACE); it.More(); it.Next()) {
        sewer.Add(it.Current());
        ++face_count;
    }
    if (face_count == 0) {
        return shape;
    }
    sewer.Perform();
    TopoDS_Shape sewn = sewer.SewedShape();

    BRepBuilderAPI_MakeSolid maker;
    bool added = false;
    for (TopExp_Explorer it(sewn, TopAbs_SHELL); it.More(); it.Next()) {
        maker.Add(TopoDS::Shell(it.Current()));
        added = true;
    }
    if (!added) {
        return shape;
    }
    return maker.Solid();
}

bool sweep_points_are_collinear(const gp_Pnt& first, const gp_Pnt& middle,
                                const gp_Pnt& last) {
    const gp_Vec first_to_middle(first, middle);
    const gp_Vec middle_to_last(middle, last);
    const double first_length = first_to_middle.Magnitude();
    const double last_length = middle_to_last.Magnitude();
    if (first_length <= 1.0e-9 || last_length <= 1.0e-9) {
        return true;
    }
    if (first_to_middle.Dot(middle_to_last) < 0.0) {
        return false;
    }
    const double cross_length = first_to_middle.Crossed(middle_to_last).Magnitude();
    return cross_length <= 1.0e-8 * std::max(1.0, first_length * last_length);
}

std::vector<gp_Pnt> simplify_linear_sweep_points(const std::vector<gp_Pnt>& points) {
    std::vector<gp_Pnt> simplified;
    simplified.reserve(points.size());
    for (const gp_Pnt& point : points) {
        simplified.push_back(point);
        while (simplified.size() >= 3) {
            const std::size_t last = simplified.size() - 1;
            if (!sweep_points_are_collinear(
                    simplified[last - 2], simplified[last - 1], simplified[last])) {
                break;
            }
            simplified.erase(simplified.end() - 2);
        }
    }
    return simplified;
}

TopoDS_Shape sweep_shape(const TopoDS_Shape& profile, const TopoDS_Shape& path, bool frenet) {
    const TopoDS_Wire spine = first_wire(path, "sweep");
    if (frenet) {
        TopExp_Explorer edge_explorer(spine, TopAbs_EDGE);
        if (!edge_explorer.More()) {
            throw EvalError("helical sweep path has no edge");
        }
        BRepAdaptor_Curve curve(TopoDS::Edge(edge_explorer.Current()));
        constexpr std::size_t kHelicalSectionCount = 64;
        BRepOffsetAPI_ThruSections loft(Standard_True, Standard_False, 1.0e-6);
        for (std::size_t index = 0; index <= kHelicalSectionCount; ++index) {
            const double ratio = static_cast<double>(index) /
                static_cast<double>(kHelicalSectionCount);
            const double parameter = curve.FirstParameter() +
                (curve.LastParameter() - curve.FirstParameter()) * ratio;
            gp_Pnt point;
            gp_Vec tangent;
            curve.D1(parameter, point, tangent);
            if (tangent.Magnitude() <= 1.0e-12) {
                throw EvalError("helical sweep has a zero tangent");
            }
            tangent.Normalize();

            gp_Vec radial(point.X(), point.Y(), 0.0);
            radial.Subtract(tangent.Multiplied(radial.Dot(tangent)));
            if (radial.Magnitude() <= 1.0e-12) {
                throw EvalError("helical sweep section is on its axis");
            }
            radial.Normalize();
            gp_Vec section_axis = radial.Crossed(tangent);
            section_axis.Normalize();

            gp_Trsf placement;
            placement.SetValues(
                radial.X(), tangent.X(), section_axis.X(), point.X(),
                radial.Y(), tangent.Y(), section_axis.Y(), point.Y(),
                radial.Z(), tangent.Z(), section_axis.Z(), point.Z());
            const TopoDS_Shape section =
                BRepBuilderAPI_Transform(profile, placement, true).Shape();
            loft.AddWire(first_wire(section, "helical sweep"));
        }
        loft.Build();
        if (!loft.IsDone() || loft.Shape().IsNull()) {
            throw EvalError("helical sweep loft failed to build");
        }
        return loft.Shape();
    }

    // Bézier paths are currently represented by short linear edges to avoid an
    // OCCT RTTI boundary defect in this runner. PipeShell treats each sampled
    // vertex as a transition and can collapse a circular section into a ribbon.
    // For a fully linear spine, loft circular sections in transported normal
    // planes instead. This preserves both transverse dimensions in the BRep.
    std::vector<gp_Pnt> dense_points;
    bool linear_spine = true;
    for (BRepTools_WireExplorer explorer(spine); explorer.More(); explorer.Next()) {
        const TopoDS_Edge edge = TopoDS::Edge(explorer.Current());
        BRepAdaptor_Curve curve(edge);
        if (curve.GetType() != GeomAbs_Line) {
            linear_spine = false;
            break;
        }
        const bool reversed = edge.Orientation() == TopAbs_REVERSED;
        gp_Pnt first;
        gp_Pnt last;
        curve.D0(reversed ? curve.LastParameter() : curve.FirstParameter(), first);
        curve.D0(reversed ? curve.FirstParameter() : curve.LastParameter(), last);
        if (dense_points.empty() || dense_points.back().Distance(first) > 1.0e-7) {
            dense_points.push_back(first);
        }
        if (dense_points.empty() || dense_points.back().Distance(last) > 1.0e-7) {
            dense_points.push_back(last);
        }
    }
    if (linear_spine && dense_points.size() >= 2) {
        // Every direction-changing vertex is authored geometry. Remove only
        // numerically collinear interior samples, which preserves the exact
        // centerline while collapsing straight Bézier samples. The 1024-point
        // bound covers authored multi-row, multi-loop noodle workloads at 16
        // samples per cubic, while keeping loft cost deterministic.
        const std::vector<gp_Pnt> section_points =
            simplify_linear_sweep_points(dense_points);
        constexpr std::size_t kMaxSweepSectionCount = 1024;
        if (section_points.size() > kMaxSweepSectionCount) {
            throw EvalError("sweep path has too many linear sections (maximum 1024)");
        }

        // Ruled sections keep authored polyline corners as separate straight
        // spans. A smooth BSpline loft overshoots sharply changing tangents.
        BRepOffsetAPI_ThruSections loft(Standard_True, Standard_True, 1.0e-6);
        gp_Vec previous_x;
        gp_Vec previous_y;
        bool have_frame = false;
        for (std::size_t index = 0; index < section_points.size(); ++index) {
            gp_Vec tangent;
            if (index == 0) {
                tangent = gp_Vec(section_points[0], section_points[1]);
            } else if (index + 1 == section_points.size()) {
                tangent = gp_Vec(section_points[index - 1], section_points[index]);
            } else {
                tangent = gp_Vec(section_points[index - 1], section_points[index + 1]);
            }
            if (tangent.Magnitude() <= 1.0e-12) {
                throw EvalError("sweep section has a zero tangent");
            }
            tangent.Normalize();

            gp_Vec section_x;
            if (!have_frame) {
                section_x = gp_Vec(0.0, 0.0, 1.0);
                section_x.Subtract(tangent.Multiplied(section_x.Dot(tangent)));
                if (section_x.Magnitude() <= 1.0e-12) {
                    section_x = gp_Vec(0.0, 1.0, 0.0);
                    section_x.Subtract(tangent.Multiplied(section_x.Dot(tangent)));
                }
            } else {
                section_x = previous_x;
                section_x.Subtract(tangent.Multiplied(section_x.Dot(tangent)));
                if (section_x.Magnitude() <= 1.0e-12) {
                    section_x = previous_y;
                    section_x.Subtract(tangent.Multiplied(section_x.Dot(tangent)));
                }
            }
            if (section_x.Magnitude() <= 1.0e-12) {
                throw EvalError("sweep section frame collapsed");
            }
            section_x.Normalize();
            gp_Vec section_y = tangent.Crossed(section_x);
            section_y.Normalize();
            section_x = section_y.Crossed(tangent);
            section_x.Normalize();

            gp_Trsf placement;
            const gp_Pnt& point = section_points[index];
            placement.SetValues(
                section_x.X(), section_y.X(), tangent.X(), point.X(),
                section_x.Y(), section_y.Y(), tangent.Y(), point.Y(),
                section_x.Z(), section_y.Z(), tangent.Z(), point.Z());
            const TopoDS_Shape section =
                BRepBuilderAPI_Transform(profile, placement, true).Shape();
            loft.AddWire(first_wire(section, "sweep section"));
            previous_x = section_x;
            previous_y = section_y;
            have_frame = true;
        }
        loft.Build();
        if (!loft.IsDone() || loft.Shape().IsNull()) {
            throw EvalError("section loft sweep failed to build");
        }
        TopoDS_Shape swept = loft.Shape();
        if (!shape_has_solid(swept) || !BRepCheck_Analyzer(swept).IsValid()) {
            ShapeFix_Shape fixer(swept);
            fixer.Perform();
            swept = fixer.Shape();
        }
        if (!shape_has_solid(swept) || !BRepCheck_Analyzer(swept).IsValid()) {
            throw EvalError("section loft sweep produced an invalid solid");
        }
        return swept;
    }

    BRepOffsetAPI_MakePipeShell pipe(spine);
    // Match build123d's `Solid.sweep`: corrected-Frenet trihedron
    // (is_frenet=False) for generic spines, Transformed transition, and
    // Add(profile, withContact=False, withCorrection=False). A helical spine
    // instead needs the Frenet trihedron (`frenet=true`): its centripetal normal
    // points at the axis, keeping a thread section radial. Corrected-Frenet banks
    // the section off `radius` (the thread defect). Without an explicit SetMode
    // the builder has no trihedron and throws Standard_NullObject on a curve.
    // A helical thread spine (`frenet=true`) uses the Frenet trihedron so the
    // section stays radial (centripetal normal at the axis), with RightCorner
    // transitions — the proven thread recipe (FreeCAD FastenersWB `screw_maker`).
    // Corrected-Frenet banks the section off `radius` (the observed defect). The
    // helix edge already carries a 3D curve (BRepLib::BuildCurves3d) so Frenet
    // does not hit Standard_NullObject. Generic spines keep corrected-Frenet +
    // Transformed to match build123d's `Solid.sweep`.
    pipe.SetMode(Standard_False);
    pipe.SetTransitionMode(BRepBuilderAPI_Transformed);
    BRepTools_WireExplorer start_explorer(spine);
    if (!start_explorer.More()) {
        throw EvalError("sweep path has no edge");
    }
    const TopoDS_Edge start_edge = TopoDS::Edge(start_explorer.Current());
    BRepAdaptor_Curve start_curve(start_edge);
    const bool reversed = start_edge.Orientation() == TopAbs_REVERSED;
    const double start_parameter = reversed
        ? start_curve.LastParameter()
        : start_curve.FirstParameter();
    gp_Pnt start_point;
    gp_Vec start_tangent;
    start_curve.D1(start_parameter, start_point, start_tangent);
    if (reversed) {
        start_tangent.Reverse();
    }
    if (start_tangent.Magnitude() <= 1.0e-12) {
        throw EvalError("sweep path has a zero start tangent");
    }
    start_tangent.Normalize();

    gp_Vec profile_x(0.0, 0.0, 1.0);
    profile_x.Subtract(start_tangent.Multiplied(profile_x.Dot(start_tangent)));
    if (profile_x.Magnitude() <= 1.0e-12) {
        profile_x = gp_Vec(0.0, 1.0, 0.0);
        profile_x.Subtract(start_tangent.Multiplied(profile_x.Dot(start_tangent)));
    }
    profile_x.Normalize();
    gp_Vec profile_y = start_tangent.Crossed(profile_x);
    profile_y.Normalize();
    profile_x = profile_y.Crossed(start_tangent);
    profile_x.Normalize();

    gp_Trsf start_frame;
    start_frame.SetValues(
        profile_x.X(), profile_y.X(), start_tangent.X(), start_point.X(),
        profile_x.Y(), profile_y.Y(), start_tangent.Y(), start_point.Y(),
        profile_x.Z(), profile_y.Z(), start_tangent.Z(), start_point.Z()
    );
    const TopoDS_Shape placed_profile =
        BRepBuilderAPI_Transform(profile, start_frame, true).Shape();
    pipe.Add(first_wire(placed_profile, "sweep"), Standard_False, Standard_True);
    pipe.Build();
    if (!pipe.IsDone()) {
        throw EvalError("sweep failed to build");
    }
    // MakeSolid caps the swept tube into a solid. Its boolean return is the
    // difference between "renders as a tube" and "can be clipped/cut": a helix
    // whose ends will not auto-cap leaves an open shell here, which later
    // BRepAlgoAPI_Common (clip-box) silently reduces to nothing.
    TopoDS_Shape swept;
    try {
        pipe.MakeSolid();
        swept = pipe.Shape();
    } catch (const Standard_Failure&) {
        // Some OCCT versions throw while marking a face sweep solid even when
        // the generated closed shell is usable; keep the shell for sewing.
        swept = pipe.Shape();
    }
    // Defensive: if the pipe-shell did not cap into a solid, sew + close it so
    // downstream booleans have a solid to operate on.
    if (!shape_has_solid(swept)) {
        swept = solidify_swept_shell(swept);
    }
    if (!shape_has_solid(swept)) {
        throw EvalError("sweep did not produce a closed solid");
    }
    // Collapse duplicate same-domain faces emitted at spline section seams.
    ShapeUpgrade_UnifySameDomain unify_sweep(
        swept, Standard_True, Standard_True, Standard_False);
    unify_sweep.Build();
    if (!unify_sweep.Shape().IsNull() &&
        BRepCheck_Analyzer(unify_sweep.Shape()).IsValid()) {
        swept = unify_sweep.Shape();
    }
    // Sew coincident PipeShell seams before downstream tessellation. This is
    // topology repair on the analytic BRep, not a mesh weld.
    try {
        BRepBuilderAPI_Sewing sewer(1.0e-5);
        sewer.Add(swept);
        sewer.Perform();
        const TopoDS_Shape sewn = sewer.SewedShape();
        for (TopExp_Explorer shell_explorer(sewn, TopAbs_SHELL);
             shell_explorer.More(); shell_explorer.Next()) {
            BRepBuilderAPI_MakeSolid maker(TopoDS::Shell(shell_explorer.Current()));
            if (!maker.IsDone()) continue;
            const TopoDS_Shape candidate = maker.Solid();
            if (BRepCheck_Analyzer(candidate).IsValid()) {
                swept = candidate;
                break;
            }
        }
    } catch (const Standard_Failure&) {
    }
    // Heal an invalid (self-intersecting / out-of-tolerance) swept solid so it
    // can be intersected and subtracted like any other solid.
    if (!BRepCheck_Analyzer(swept).IsValid()) {
        ShapeFix_Shape fixer(swept);
        fixer.Perform();
        TopoDS_Shape fixed = fixer.Shape();
        if (shape_has_solid(fixed) && BRepCheck_Analyzer(fixed).IsValid()) {
            swept = fixed;
        }
    }
    if (!BRepCheck_Analyzer(swept).IsValid()) {
        throw EvalError("sweep produced an invalid solid after healing");
    }
    return swept;
}

TopoDS_Shape offset_shape(const TopoDS_Shape& profile, double amount) {
    BRepOffsetAPI_MakeOffset offset(first_wire(profile, "offset"), GeomAbs_Arc, false);
    offset.Perform(amount);
    TopoDS_Shape offset_result = offset.Shape();
    return BRepBuilderAPI_MakeFace(first_wire(offset_result, "offset")).Shape();
}

TopoDS_Shape twist_shape(const TopoDS_Shape& profile, double height, double angle_degrees) {
    constexpr std::size_t segments = 12;
    BRepOffsetAPI_ThruSections twist(true, false, 1.0e-6);
    for (std::size_t index = 0; index <= segments; ++index) {
        double ratio = static_cast<double>(index) / static_cast<double>(segments);
        gp_Trsf rotate;
        rotate.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)),
                           angle_degrees * ratio * M_PI / 180.0);
        TopoDS_Shape rotated = BRepBuilderAPI_Transform(profile, rotate, true).Shape();
        gp_Trsf translate;
        translate.SetTranslation(gp_Vec(0, 0, height * ratio));
        TopoDS_Shape section = BRepBuilderAPI_Transform(rotated, translate, true).Shape();
        twist.AddWire(first_wire(section, "twist"));
    }
    twist.Build();
    if (!twist.IsDone()) {
        throw EvalError("twist failed to build");
    }
    return twist.Shape();
}

// MVP: taper every vertical (side) face about the z = neutral_z plane,
// pulling +Z. Faces whose normal is perpendicular to the pull direction are
// the side walls. Mirrors emit_draft_operation in direct_occt_executor.rs so
// the runner and generated-source paths produce identical geometry.
TopoDS_Shape draft_shape(const TopoDS_Shape& input, double angle_degrees, double neutral_z) {
    double angle_radians = angle_degrees * M_PI / 180.0;
    BRepOffsetAPI_DraftAngle draft(input);
    gp_Dir pull(0, 0, 1);
    gp_Pln neutral(gp_Pnt(0, 0, neutral_z), pull);
    for (TopExp_Explorer face_explorer(input, TopAbs_FACE); face_explorer.More(); face_explorer.Next()) {
        TopoDS_Face face = TopoDS::Face(face_explorer.Current());
        BRepGProp_Face props(face);
        Standard_Real u1, u2, v1, v2;
        props.Bounds(u1, u2, v1, v2);
        gp_Pnt point;
        gp_Vec normal;
        props.Normal((u1 + u2) / 2.0, (v1 + v2) / 2.0, point, normal);
        if (normal.Magnitude() < 1.0e-12) {
            continue;
        }
        gp_Dir normal_dir(normal);
        if (std::abs(normal_dir.Z()) < 1.0e-6) {
            draft.Add(face, pull, angle_radians, neutral);
        }
    }
    draft.Build();
    if (!draft.IsDone()) {
        throw EvalError("draft failed to build");
    }
    return draft.Shape();
}

TopoDS_Shape taper_shape(const TopoDS_Shape& profile, double height, double scale_x, double scale_y) {
    TopoDS_Wire base_wire = first_wire(profile, "taper");
    gp_GTrsf top_scale;
    top_scale.SetValue(1, 1, scale_x);
    top_scale.SetValue(2, 2, scale_y);
    top_scale.SetValue(3, 3, 1.0);
    TopoDS_Shape top_scaled = BRepBuilderAPI_GTransform(profile, top_scale, true).Shape();
    gp_Trsf top_translate;
    top_translate.SetTranslation(gp_Vec(0, 0, height));
    TopoDS_Shape top_shape = BRepBuilderAPI_Transform(top_scaled, top_translate, true).Shape();
    BRepOffsetAPI_ThruSections taper(true, false, 1.0e-6);
    taper.AddWire(base_wire);
    taper.AddWire(first_wire(top_shape, "taper"));
    taper.Build();
    if (!taper.IsDone()) {
        throw EvalError("taper failed to build");
    }
    return taper.Shape();
}

TopoDS_Shape private_boolean_operand(const TopoDS_Shape& shape) {
    BRepBuilderAPI_Copy copy(shape, Standard_False, Standard_False);
    const TopoDS_Shape result = copy.Shape();
    if (result.IsNull()) throw EvalError("Direct OCCT boolean could not copy operand");
    return result;
}

void append_private_boolean_solids(
    const TopoDS_Shape& shape,
    TopTools_ListOfShape& destination,
    std::vector<TopoDS_Shape>& storage
) {
    bool appended = false;
    if (shape.ShapeType() == TopAbs_SOLID) {
        storage.push_back(private_boolean_operand(shape));
        destination.Append(storage.back());
        return;
    }
    for (TopExp_Explorer solid(shape, TopAbs_SOLID); solid.More(); solid.Next()) {
        storage.push_back(private_boolean_operand(solid.Current()));
        destination.Append(storage.back());
        appended = true;
    }
    if (!appended) {
        storage.push_back(private_boolean_operand(shape));
        destination.Append(storage.back());
    }
}

bool boolean_inputs_are_valid_solids(const std::vector<TopoDS_Shape>& shapes) {
    return std::all_of(shapes.begin(), shapes.end(), [](const TopoDS_Shape& shape) {
        return !shape.IsNull() && shape_has_solid(shape) && BRepCheck_Analyzer(shape).IsValid();
    });
}

constexpr std::size_t kMaximumBooleanUnifyFaceCount = 24;
constexpr std::size_t kMaximumBooleanUnifyEdgeCount = 96;

bool boolean_result_is_within_unify_budget(const TopoDS_Shape& shape) {
    std::size_t face_count = 0;
    for (TopExp_Explorer face(shape, TopAbs_FACE); face.More(); face.Next()) {
        if (++face_count > kMaximumBooleanUnifyFaceCount) return false;
    }
    std::size_t edge_count = 0;
    for (TopExp_Explorer edge(shape, TopAbs_EDGE); edge.More(); edge.Next()) {
        if (++edge_count > kMaximumBooleanUnifyEdgeCount) return false;
    }
    return true;
}

TopoDS_Shape unify_boolean_same_domain(const TopoDS_Shape& shape) {
    if (!boolean_result_is_within_unify_budget(shape)) return shape;
    ShapeUpgrade_UnifySameDomain unify(
        shape, Standard_True, Standard_True, Standard_False);
    unify.Build();
    const TopoDS_Shape unified = unify.Shape();
    if (unified.IsNull() || !BRepCheck_Analyzer(unified).IsValid()) {
        return shape;
    }
    return unified;
}

template <typename BooleanBuilder>
TopoDS_Shape checked_boolean_shapes(
    const std::vector<TopoDS_Shape>& argument_shapes,
    const std::vector<TopoDS_Shape>& tool_shapes,
    const std::string& op,
    ExecutionContext& context
) {
    StageExecutionTimer stage_timer(context, "boolean");
    TopTools_ListOfShape arguments;
    std::vector<TopoDS_Shape> private_arguments;
    for (const TopoDS_Shape& shape : argument_shapes) {
        append_private_boolean_solids(shape, arguments, private_arguments);
    }
    TopTools_ListOfShape tools;
    std::vector<TopoDS_Shape> private_tools;
    for (const TopoDS_Shape& shape : tool_shapes) {
        append_private_boolean_solids(shape, tools, private_tools);
    }
    BooleanBuilder builder;
    builder.SetArguments(arguments);
    builder.SetTools(tools);
    // Private copies isolate published slots, so OCCT avoids its slower
    // non-destructive bookkeeping without mutating a shared DAG input.
    builder.SetNonDestructive(false);
    builder.SetFuzzyValue(1.0e-5);
    // Inputs proved valid solids. The inverted-solid scan repeats validation
    // already completed by this predicate and is expensive for dense imports.
    const bool inputs_valid = boolean_inputs_are_valid_solids(argument_shapes) &&
        boolean_inputs_are_valid_solids(tool_shapes);
    builder.SetCheckInverted(inputs_valid ? Standard_False : Standard_True);
    BooleanParallelLease parallel_lease(context);
    builder.SetRunParallel(parallel_lease.runs_parallel());
    builder.SetUseOBB(true);
    builder.Build();
    if (!builder.IsDone()) {
        throw EvalError("Direct OCCT boolean `" + op + "` failed");
    }
    TopoDS_Shape result = builder.Shape();
    if (result.IsNull()) {
        throw EvalError("Direct OCCT boolean `" + op + "` returned a null shape");
    }
    TopExp_Explorer face(result, TopAbs_FACE);
    if (!face.More()) {
        throw EvalError(
            "Direct OCCT boolean `" + op +
            "` produced empty geometry; operands do not overlap or the crop plane lies outside the source bounds"
        );
    }
    return unify_boolean_same_domain(result);
}

TopoDS_Shape fuse_shapes(
    const TopoDS_Shape& lhs, const TopoDS_Shape& rhs, ExecutionContext& context
) {
    return checked_boolean_shapes<BRepAlgoAPI_Fuse>({lhs}, {rhs}, "union", context);
}

struct PreparedUnionResult {
    TopoDS_Shape full;
    std::vector<TopoDS_Shape> groups;
};

PreparedUnionResult prepare_four_way_union(
    const std::vector<TopoDS_Shape>& shapes,
    const std::vector<PartialBooleanGroupPlan>& groups,
    ExecutionContext& context
) {
    if (shapes.size() != 4) throw EvalError("prepared four-way union requires four operands");
    if (groups.size() != 2) throw EvalError("prepared four-way union requires two groups");
    StageExecutionTimer stage_timer(context, "boolean");
    TopTools_ListOfShape arguments;
    std::vector<TopoDS_Shape> prepared;
    prepared.reserve(shapes.size());
    for (const TopoDS_Shape& shape : shapes) {
        prepared.push_back(private_boolean_operand(shape));
        arguments.Append(prepared.back());
    }
    BOPAlgo_PaveFiller filler;
    filler.SetArguments(arguments);
    filler.SetFuzzyValue(1.0e-5);
    BooleanParallelLease parallel_lease(context);
    filler.SetRunParallel(parallel_lease.runs_parallel());
    filler.Perform();
    if (filler.HasErrors()) {
        throw EvalError("Direct OCCT boolean `union` intersection failed");
    }
    const auto materialize = [&](const std::vector<std::uint32_t>& indices) {
        if (indices.empty()) throw EvalError("prepared union group is empty");
        BOPAlgo_Builder builder;
        builder.SetArguments(arguments);
        builder.PerformWithFiller(filler);
        if (builder.HasErrors()) {
            throw EvalError("Direct OCCT union could not use prepared intersections");
        }
        TopTools_ListOfShape objects;
        objects.Append(prepared.at(indices.front()));
        TopTools_ListOfShape tools;
        for (std::size_t offset = 1; offset < indices.size(); ++offset) {
            tools.Append(prepared.at(indices[offset]));
        }
        builder.BuildBOP(objects, tools, BOPAlgo_FUSE, Message_ProgressRange());
        if (builder.HasErrors() || builder.Shape().IsNull()) {
            throw EvalError("Direct OCCT union materialization failed");
        }
        return unify_boolean_same_domain(builder.Shape());
    };
    std::vector<TopoDS_Shape> partials(2);
    for (const PartialBooleanGroupPlan& group : groups) {
        if (group.ordinal >= partials.size()) {
            throw EvalError("prepared union group ordinal out of range");
        }
        partials[group.ordinal] = materialize(group.input_indices);
    }
    return {materialize({0, 1, 2, 3}), std::move(partials)};
}

TopoDS_Shape fuse_shapes(const std::vector<TopoDS_Shape>& shapes, ExecutionContext& context) {
    if (shapes.empty()) {
        throw EvalError("Direct OCCT boolean `union` requires at least one operand");
    }
    if (shapes.size() == 1) {
        return shapes.front();
    }
    StageExecutionTimer stage_timer(context, "boolean");
    TopTools_ListOfShape arguments;
    std::vector<TopoDS_Shape> prepared;
    prepared.reserve(shapes.size() * 2);
    for (const TopoDS_Shape& shape : shapes) {
        append_private_boolean_solids(shape, arguments, prepared);
    }
    BOPAlgo_PaveFiller filler;
    filler.SetArguments(arguments);
    filler.SetFuzzyValue(1.0e-5);
    BooleanParallelLease parallel_lease(context);
    filler.SetRunParallel(parallel_lease.runs_parallel());
    filler.Perform();
    if (filler.HasErrors()) {
        throw EvalError("Direct OCCT boolean `union` intersection failed");
    }
    BOPAlgo_Builder builder;
    builder.SetArguments(arguments);
    builder.PerformWithFiller(filler);
    if (builder.HasErrors()) {
        throw EvalError("Direct OCCT boolean `union` could not materialize prepared intersections");
    }
    TopTools_ListOfShape objects;
    objects.Append(prepared.front());
    TopTools_ListOfShape tools;
    for (std::size_t index = 1; index < prepared.size(); ++index) {
        tools.Append(prepared[index]);
    }
    builder.BuildBOP(objects, tools, BOPAlgo_FUSE, Message_ProgressRange());
    if (builder.HasErrors() || builder.Shape().IsNull()) {
        throw EvalError("Direct OCCT boolean `union` failed");
    }
    return unify_boolean_same_domain(builder.Shape());
}

TopoDS_Shape cut_shapes(
    const TopoDS_Shape& lhs, const TopoDS_Shape& rhs, ExecutionContext& context
) {
    return checked_boolean_shapes<BRepAlgoAPI_Cut>({lhs}, {rhs}, "difference", context);
}

TopoDS_Shape cut_shapes(
    const TopoDS_Shape& head,
    const std::vector<TopoDS_Shape>& tools,
    ExecutionContext& context
) {
    if (tools.empty()) {
        throw EvalError("Direct OCCT boolean `difference` requires at least two operands");
    }
    return checked_boolean_shapes<BRepAlgoAPI_Cut>({head}, tools, "difference", context);
}

TopoDS_Shape common_shapes(
    const TopoDS_Shape& lhs, const TopoDS_Shape& rhs, ExecutionContext& context
) {
    return checked_boolean_shapes<BRepAlgoAPI_Common>({lhs}, {rhs}, "intersection", context);
}

// --- Convex hull -----------------------------------------------------------
// OCCT ships no convex-hull primitive, so the hull op gathers a surface point
// cloud (tessellated nodes plus BREP vertices) from every child shape and
// builds the 3-D convex hull with an incremental algorithm: seed a tetrahedron
// from four extreme non-coplanar points, then fold each remaining point into
// the hull by deleting the faces it can see and stitching new faces across the
// horizon. The resulting triangle set is sewn into a closed shell and a solid.
namespace hull_detail {

// Signed volume of the parallelepiped spanned by (b-a, c-a, d-a) == 6·V of the
// tetrahedron. Positive when d sits on the +normal side of triangle (a,b,c).
inline double orient(const gp_Pnt& a, const gp_Pnt& b, const gp_Pnt& c, const gp_Pnt& d) {
    gp_Vec ab(a, b);
    gp_Vec ac(a, c);
    gp_Vec ad(a, d);
    return ab.Crossed(ac).Dot(ad);
}

struct Face {
    int v[3];
};

std::vector<Face> incremental_hull(const std::vector<gp_Pnt>& pts) {
    const std::size_t n = pts.size();
    const double eps = 1.0e-9;

    // i0/i1: the two points farthest apart along the initial extent.
    int i0 = 0;
    int i1 = -1;
    double best = -1.0;
    for (std::size_t j = 1; j < n; ++j) {
        double d = pts[0].SquareDistance(pts[j]);
        if (d > best) {
            best = d;
            i1 = static_cast<int>(j);
        }
    }
    if (i1 < 0 || best <= eps) {
        throw EvalError("hull requires input geometry with more than one distinct point");
    }

    // i2: farthest from the line i0-i1.
    int i2 = -1;
    best = -1.0;
    for (std::size_t j = 0; j < n; ++j) {
        gp_Vec e(pts[i0], pts[i1]);
        gp_Vec p(pts[i0], pts[j]);
        double area = e.Crossed(p).SquareMagnitude();
        if (area > best) {
            best = area;
            i2 = static_cast<int>(j);
        }
    }
    if (i2 < 0 || best <= eps) {
        throw EvalError("hull requires non-collinear input geometry");
    }

    // i3: farthest from the plane i0-i1-i2.
    int i3 = -1;
    best = 0.0;
    for (std::size_t j = 0; j < n; ++j) {
        double vol = std::abs(orient(pts[i0], pts[i1], pts[i2], pts[j]));
        if (vol > best) {
            best = vol;
            i3 = static_cast<int>(j);
        }
    }
    if (i3 < 0 || best <= eps) {
        throw EvalError("hull requires non-coplanar input geometry (need volume)");
    }

    // Seed tetrahedron with every face oriented so its normal points outward
    // (away from the opposite vertex, i.e. away from the interior).
    auto make_outward = [&](int a, int b, int c, int apex) -> Face {
        if (orient(pts[a], pts[b], pts[c], pts[apex]) > 0.0) {
            return Face{{a, c, b}};
        }
        return Face{{a, b, c}};
    };
    std::vector<Face> faces;
    faces.push_back(make_outward(i0, i1, i2, i3));
    faces.push_back(make_outward(i0, i1, i3, i2));
    faces.push_back(make_outward(i0, i2, i3, i1));
    faces.push_back(make_outward(i1, i2, i3, i0));

    std::vector<bool> used(n, false);
    used[i0] = used[i1] = used[i2] = used[i3] = true;

    for (std::size_t p = 0; p < n; ++p) {
        if (used[p]) {
            continue;
        }
        // Faces the point can see (it lies on their outward side).
        std::vector<char> visible(faces.size(), 0);
        bool any = false;
        for (std::size_t f = 0; f < faces.size(); ++f) {
            const Face& face = faces[f];
            if (orient(pts[face.v[0]], pts[face.v[1]], pts[face.v[2]], pts[p]) > eps) {
                visible[f] = 1;
                any = true;
            }
        }
        if (!any) {
            continue;  // interior point
        }

        // Horizon = directed edges of visible faces whose reverse is not also
        // visible. Count directed edges to find the boundary of the visible set.
        std::map<std::pair<int, int>, int> edge_count;
        for (std::size_t f = 0; f < faces.size(); ++f) {
            if (!visible[f]) {
                continue;
            }
            const Face& face = faces[f];
            for (int e = 0; e < 3; ++e) {
                int a = face.v[e];
                int b = face.v[(e + 1) % 3];
                edge_count[{a, b}] += 1;
            }
        }
        std::vector<Face> kept;
        kept.reserve(faces.size());
        for (std::size_t f = 0; f < faces.size(); ++f) {
            if (!visible[f]) {
                kept.push_back(faces[f]);
            }
        }
        for (const auto& entry : edge_count) {
            int a = entry.first.first;
            int b = entry.first.second;
            if (edge_count.find({b, a}) == edge_count.end()) {
                // Boundary edge: cone it to the new point, preserving winding.
                kept.push_back(Face{{a, b, static_cast<int>(p)}});
            }
        }
        faces.swap(kept);
        used[p] = true;
    }

    return faces;
}

}  // namespace hull_detail

TopoDS_Shape convex_hull_shapes(const std::vector<TopoDS_Shape>& shapes) {
    std::vector<gp_Pnt> pts;
    for (const TopoDS_Shape& shape : shapes) {
        BRepMesh_IncrementalMesh mesh(shape, 0.1, Standard_False, 0.5, Standard_True);
        (void)mesh;
        for (TopExp_Explorer ex(shape, TopAbs_FACE); ex.More(); ex.Next()) {
            TopoDS_Face face = TopoDS::Face(ex.Current());
            TopLoc_Location loc;
            Handle(Poly_Triangulation) tri = BRep_Tool::Triangulation(face, loc);
            if (tri.IsNull()) {
                continue;
            }
            gp_Trsf t = loc.Transformation();
            for (Standard_Integer i = 1; i <= tri->NbNodes(); ++i) {
                pts.push_back(tri->Node(i).Transformed(t));
            }
        }
        // BREP vertices cover sketch/polygon inputs that carry no triangulation.
        for (TopExp_Explorer ex(shape, TopAbs_VERTEX); ex.More(); ex.Next()) {
            pts.push_back(BRep_Tool::Pnt(TopoDS::Vertex(ex.Current())));
        }
    }
    if (pts.size() < 4) {
        throw EvalError("hull requires at least four surface points across its inputs");
    }

    std::vector<hull_detail::Face> faces = hull_detail::incremental_hull(pts);
    if (faces.empty()) {
        throw EvalError("hull produced no faces");
    }

    BRepBuilderAPI_Sewing sewing(1.0e-6);
    for (const hull_detail::Face& face : faces) {
        BRepBuilderAPI_MakePolygon poly(pts[face.v[0]], pts[face.v[1]], pts[face.v[2]], Standard_True);
        if (!poly.IsDone()) {
            continue;
        }
        BRepBuilderAPI_MakeFace mk(poly.Wire(), Standard_True);
        if (!mk.IsDone()) {
            continue;
        }
        sewing.Add(mk.Face());
    }
    sewing.Perform();
    TopoDS_Shape sewn = sewing.SewedShape();

    TopoDS_Shell shell;
    bool found = false;
    for (TopExp_Explorer ex(sewn, TopAbs_SHELL); ex.More(); ex.Next()) {
        shell = TopoDS::Shell(ex.Current());
        found = true;
        break;
    }
    if (!found) {
        throw EvalError("hull failed to sew a closed shell");
    }

    BRepBuilderAPI_MakeSolid mk_solid(shell);
    if (!mk_solid.IsDone()) {
        throw EvalError("hull failed to build a solid from its shell");
    }
    TopoDS_Solid solid = mk_solid.Solid();

    // A shell sewn from outward triangles can still yield an inverted solid;
    // flip it if the enclosed volume comes out negative.
    GProp_GProps props;
    BRepGProp::VolumeProperties(solid, props);
    if (props.Mass() < 0.0) {
        solid.Reverse();
    }
    return solid;
}

TopoDS_Shape solidify_shape(const TopoDS_Shape& input) {
    TopoDS_Shell shell;
    bool found_shell = false;
    for (TopExp_Explorer shell_explorer(input, TopAbs_SHELL);
         shell_explorer.More();
         shell_explorer.Next()) {
        shell = TopoDS::Shell(shell_explorer.Current());
        found_shell = true;
        break;
    }
    if (!found_shell) {
        BRep_Builder builder;
        builder.MakeShell(shell);
        std::size_t face_count = 0;
        for (TopExp_Explorer face_explorer(input, TopAbs_FACE);
             face_explorer.More();
             face_explorer.Next()) {
            builder.Add(shell, TopoDS::Face(face_explorer.Current()));
            ++face_count;
        }
        if (face_count == 0) {
            throw EvalError("solidify input contains no faces");
        }
    }

    BRepBuilderAPI_MakeSolid maker(shell);
    if (!maker.IsDone()) {
        throw EvalError("solidify failed to build a solid from its shell");
    }
    TopoDS_Solid solid = maker.Solid();
    GProp_GProps properties;
    BRepGProp::VolumeProperties(solid, properties);
    if (properties.Mass() < 0.0) {
        solid.Reverse();
    }
    return solid;
}

// Clip by subtracting the six half-slabs outside [x,y,z] with BRepAlgoAPI_Cut.
// BRepAlgoAPI_Common silently returns an empty shape on some valid faceted
// swept solids (notably the polyline-spine `helical-ridge`), while Cut/Fuse on
// the same solid succeed. Removing the outside material with Cut is the robust
// equivalent of intersecting with the box.
TopoDS_Shape clip_by_cut(
    const TopoDS_Shape& shape,
    const std::array<double, 2>& x,
    const std::array<double, 2>& y,
    const std::array<double, 2>& z
) {
    Bnd_Box bounds;
    BRepBndLib::Add(shape, bounds);
    if (bounds.IsVoid()) {
        return shape;
    }
    double bx0, by0, bz0, bx1, by1, bz1;
    bounds.Get(bx0, by0, bz0, bx1, by1, bz1);
    const double pad = 1.0;
    const double X0 = std::min(bx0, x[0]) - pad;
    const double X1 = std::max(bx1, x[1]) + pad;
    const double Y0 = std::min(by0, y[0]) - pad;
    const double Y1 = std::max(by1, y[1]) + pad;
    const double Z0 = std::min(bz0, z[0]) - pad;
    const double Z1 = std::max(bz1, z[1]) + pad;

    TopoDS_Shape result = shape;
    auto cut_away = [&](double ax, double ay, double az, double bx, double by, double bz) {
        if (ax >= bx || ay >= by || az >= bz) {
            return;
        }
        TopoDS_Shape tool = BRepPrimAPI_MakeBox(gp_Pnt(ax, ay, az), gp_Pnt(bx, by, bz)).Shape();
        BRepAlgoAPI_Cut cut(result, tool);
        cut.Build();
        if (cut.IsDone()) {
            result = cut.Shape();
        }
    };
    cut_away(X0, Y0, Z0, x[0], Y1, Z1);  // x below
    cut_away(x[1], Y0, Z0, X1, Y1, Z1);  // x above
    cut_away(X0, Y0, Z0, X1, y[0], Z1);  // y below
    cut_away(X0, y[1], Z0, X1, Y1, Z1);  // y above
    cut_away(X0, Y0, Z0, X1, Y1, z[0]);  // z below
    cut_away(X0, Y0, z[1], X1, Y1, Z1);  // z above
    return result;
}

TopoDS_Shape clip_box_shape(
    const TopoDS_Shape& shape,
    const std::array<double, 2>& x,
    const std::array<double, 2>& y,
    const std::array<double, 2>& z
) {
    TopoDS_Shape clip_box =
        BRepPrimAPI_MakeBox(gp_Pnt(x[0], y[0], z[0]), gp_Pnt(x[1], y[1], z[1])).Shape();
    TopoDS_Shape result = BRepAlgoAPI_Common(shape, clip_box).Shape();
    // BRepAlgoAPI_Common can silently collapse a valid solid to nothing
    // (faceted swept helixes are the common offender). Fall back to carving the
    // outside material away with Cut before treating it as truly empty.
    if (!shape_has_solid(result) && shape_has_solid(shape)) {
        TopoDS_Shape carved = clip_by_cut(shape, x, y, z);
        if (shape_has_solid(carved)) {
            result = carved;
        }
    }
    // A clip that still keeps no solid is a real error (non-solid input or a box
    // that misses the shape). Fail loudly instead of letting the empty shape
    // vanish silently through a later fuse/cut.
    if (!shape_has_solid(result) && shape_has_solid(shape)) {
        throw EvalError(
            "clip-box removed all geometry: the clip box keeps no solid of the input shape");
    }
    return result;
}

// Bound a fused external thread to its authored major-radius and length
// envelope. Trim the spill at both helix ends with Cut first: OCCT Common can
// collapse an untrimmed swept helicoid to an empty shape. The now-bounded solid
// can then be intersected with its exact cylindrical/conical radial envelope.
// If OCCT still rejects Common for a valid swept solid, the axial Cut result is
// safe because the authored radial sweep already uses these same major radii.
TopoDS_Shape clip_thread_envelope_shape(
    const TopoDS_Shape& shape,
    double bottom_major_radius,
    double top_major_radius,
    double length
) {
    if (!std::isfinite(bottom_major_radius) || bottom_major_radius <= 0.0 ||
        !std::isfinite(top_major_radius) || top_major_radius <= 0.0 ||
        !std::isfinite(length) || length <= 0.0) {
        throw EvalError(
            "clip-thread-envelope radii and length must be positive and finite");
    }

    const double radial_extent = std::max(bottom_major_radius, top_major_radius);
    TopoDS_Shape axial_result = clip_by_cut(
        shape,
        {-radial_extent, radial_extent},
        {-radial_extent, radial_extent},
        {0.0, length});
    const TopoDS_Shape envelope =
        std::abs(bottom_major_radius - top_major_radius) <= 1.0e-12
        ? BRepPrimAPI_MakeCylinder(bottom_major_radius, length).Shape()
        : BRepPrimAPI_MakeCone(bottom_major_radius, top_major_radius, length).Shape();
    BRepAlgoAPI_Common radial_common(axial_result, envelope);
    radial_common.Build();
    TopoDS_Shape result = axial_result;
    if (radial_common.IsDone() && shape_has_solid(radial_common.Shape()) &&
        BRepCheck_Analyzer(radial_common.Shape()).IsValid()) {
        result = radial_common.Shape();
    }
    if (!shape_has_solid(result)) {
        throw EvalError("clip-thread-envelope removed the external thread solid");
    }
    if (!BRepCheck_Analyzer(result).IsValid()) {
        ShapeFix_Shape fixer(result);
        fixer.Perform();
        const TopoDS_Shape fixed = fixer.Shape();
        if (shape_has_solid(fixed) && BRepCheck_Analyzer(fixed).IsValid()) {
            result = fixed;
        }
    }
    return result;
}

TopoDS_Shape clip_plane_shape(
    const TopoDS_Shape& shape,
    const std::array<double, 3>& origin,
    const std::array<double, 3>& normal,
    bool keep_positive
) {
    const double magnitude = std::sqrt(
        normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]);
    if (!std::isfinite(magnitude) || magnitude <= 1e-12) {
        throw EvalError("clip-plane normal must be finite and non-zero");
    }
    if (!std::all_of(origin.begin(), origin.end(), [](double value) { return std::isfinite(value); })) {
        throw EvalError("clip-plane origin must be finite");
    }

    const double sign = keep_positive ? 1.0 : -1.0;
    const gp_Dir kept_direction(
        sign * normal[0] / magnitude,
        sign * normal[1] / magnitude,
        sign * normal[2] / magnitude);
    const gp_Pnt plane_origin(origin[0], origin[1], origin[2]);

    Bnd_Box bounds;
    BRepBndLib::Add(shape, bounds);
    if (bounds.IsVoid()) {
        throw EvalError("clip-plane input shape has no bounds");
    }
    double x0, y0, z0, x1, y1, z1;
    bounds.Get(x0, y0, z0, x1, y1, z1);
    double farthest = 0.0;
    for (double x : {x0, x1}) {
        for (double y : {y0, y1}) {
            for (double z : {z0, z1}) {
                farthest = std::max(farthest, plane_origin.Distance(gp_Pnt(x, y, z)));
            }
        }
    }
    const double extent = std::max(1.0, farthest * 2.0 + 1.0);
    const gp_Ax2 plane_axis(plane_origin, kept_direction);
    gp_Vec corner_shift(plane_axis.XDirection());
    corner_shift *= -extent;
    gp_Vec y_shift(plane_axis.YDirection());
    y_shift *= -extent;
    corner_shift += y_shift;
    const gp_Pnt tool_corner = plane_origin.Translated(corner_shift);
    const gp_Ax2 tool_axis(tool_corner, kept_direction, plane_axis.XDirection());
    const TopoDS_Shape half_space_tool =
        BRepPrimAPI_MakeBox(tool_axis, 2.0 * extent, 2.0 * extent, 2.0 * extent).Shape();

    BRepAlgoAPI_Common common(shape, half_space_tool);
    common.Build();
    if (!common.IsDone()) {
        throw EvalError("clip-plane boolean intersection failed");
    }
    TopoDS_Shape result = common.Shape();
    if (result.IsNull() || (shape_has_solid(shape) && !shape_has_solid(result))) {
        throw EvalError("clip-plane removed all geometry: selected half-space keeps no solid");
    }
    return result;
}

std::vector<int> resolve_edge_target_indexes(
    const std::string& part_id,
    const TopoDS_Shape& shape,
    const std::vector<std::string>& requested_target_ids
) {
    TopTools_IndexedMapOfShape edge_map;
    TopExp::MapShapes(shape, TopAbs_EDGE, edge_map);
    std::vector<std::string> edge_target_ids;
    std::vector<std::string> edge_stable_ids;
    std::map<std::string, int> stable_counts;
    edge_target_ids.reserve(edge_map.Extent());
    edge_stable_ids.reserve(edge_map.Extent());
    for (int edge_ordinal = 1; edge_ordinal <= edge_map.Extent(); ++edge_ordinal) {
        int edge_index = edge_ordinal - 1;
        TopoDS_Edge edge = TopoDS::Edge(edge_map.FindKey(edge_ordinal));
        std::string target_id = edge_target_id(part_id, edge_index, edge);
        std::string stable_id = stable_edge_target_id(target_id);
        edge_target_ids.push_back(target_id);
        edge_stable_ids.push_back(stable_id);
        stable_counts[stable_id] += 1;
    }

    std::vector<int> matched_indexes;
    std::vector<std::string> matched_target_ids;
    for (const std::string& requested_target_id : requested_target_ids) {
        bool matched = false;
        for (std::size_t candidate_index = 0; candidate_index < edge_target_ids.size(); ++candidate_index) {
            if (edge_target_ids[candidate_index] != requested_target_id) {
                continue;
            }
            if (std::find(matched_indexes.begin(), matched_indexes.end(), static_cast<int>(candidate_index)) ==
                matched_indexes.end()) {
                matched_indexes.push_back(static_cast<int>(candidate_index));
            }
            matched_target_ids.push_back(requested_target_id);
            matched = true;
            break;
        }
        if (matched) {
            continue;
        }
        std::string requested_stable_id = stable_edge_target_id(requested_target_id);
        if (stable_counts[requested_stable_id] > 1) {
            throw EvalError(
                std::string("edge selector ambiguously matched stable edge target: ") +
                requested_target_id
            );
        }
        for (std::size_t candidate_index = 0; candidate_index < edge_stable_ids.size(); ++candidate_index) {
            if (edge_stable_ids[candidate_index] != requested_stable_id) {
                continue;
            }
            if (std::find(matched_indexes.begin(), matched_indexes.end(), static_cast<int>(candidate_index)) ==
                matched_indexes.end()) {
                matched_indexes.push_back(static_cast<int>(candidate_index));
            }
            matched_target_ids.push_back(requested_target_id);
            matched = true;
            break;
        }
        if (!matched) {
            throw EvalError(
                std::string("edge selector did not match target ids: ") + requested_target_id
            );
        }
    }
    if (matched_target_ids.size() != requested_target_ids.size()) {
        throw EvalError("edge selector ambiguously matched stable edge target");
    }
    if (matched_indexes.empty()) {
        throw EvalError("edge selector did not match target ids");
    }
    return matched_indexes;
}

std::vector<int> resolve_edge_clauses(
    const TopoDS_Shape& shape,
    const std::vector<SelectorClause>& clauses
) {
    if (clauses.empty()) {
        throw EvalError("edge selector clauses cannot be empty");
    }

    Bnd_Box shape_box;
    BRepBndLib::Add(shape, shape_box);
    double xmin = 0.0;
    double ymin = 0.0;
    double zmin = 0.0;
    double xmax = 0.0;
    double ymax = 0.0;
    double zmax = 0.0;
    shape_box.Get(xmin, ymin, zmin, xmax, ymax, zmax);
    double tol = std::max(
        xmax - xmin,
        std::max(ymax - ymin, std::max(zmax - zmin, 1.0))
    ) * 1.0e-6;

    std::vector<int> matched_indexes;
    TopTools_IndexedMapOfShape edge_map;
    TopExp::MapShapes(shape, TopAbs_EDGE, edge_map);
    for (int edge_ordinal = 1; edge_ordinal <= edge_map.Extent(); ++edge_ordinal) {
        int edge_index = edge_ordinal - 1;
        TopoDS_Edge edge = TopoDS::Edge(edge_map.FindKey(edge_ordinal));
        Bnd_Box edge_box;
        BRepBndLib::Add(edge, edge_box);
        double edge_xmin = 0.0;
        double edge_ymin = 0.0;
        double edge_zmin = 0.0;
        double edge_xmax = 0.0;
        double edge_ymax = 0.0;
        double edge_zmax = 0.0;
        edge_box.Get(edge_xmin, edge_ymin, edge_zmin, edge_xmax, edge_ymax, edge_zmax);

        bool matches = true;
        for (const SelectorClause& clause : clauses) {
            switch (clause.type) {
                case SelectorClauseType::Axis: {
                    if (!clause.axis.has_value()) {
                        throw EvalError("edge axis selector missing axis");
                    }
                    double x_span = edge_xmax - edge_xmin;
                    double y_span = edge_ymax - edge_ymin;
                    double z_span = edge_zmax - edge_zmin;
                    if (*clause.axis == SelectorAxis::X) {
                        matches = matches && x_span > tol && y_span <= tol && z_span <= tol;
                    } else if (*clause.axis == SelectorAxis::Y) {
                        matches = matches && y_span > tol && x_span <= tol && z_span <= tol;
                    } else {
                        matches = matches && z_span > tol && x_span <= tol && y_span <= tol;
                    }
                    break;
                }
                case SelectorClauseType::Boundary: {
                    if (!clause.axis.has_value() || !clause.bound.has_value()) {
                        throw EvalError("edge boundary selector missing axis or bound");
                    }
                    double shape_min = selector_axis_min(*clause.axis, xmin, ymin, zmin);
                    double shape_max = selector_axis_max(*clause.axis, xmax, ymax, zmax);
                    double edge_min = selector_axis_min(*clause.axis, edge_xmin, edge_ymin, edge_zmin);
                    double edge_max = selector_axis_max(*clause.axis, edge_xmax, edge_ymax, edge_zmax);
                    double shape_bound = *clause.bound == SelectorBound::Min ? shape_min : shape_max;
                    matches = matches &&
                        std::abs(edge_min - shape_bound) <= tol &&
                        std::abs(edge_max - shape_bound) <= tol;
                    break;
                }
                default:
                    throw EvalError("unsupported edge selector clause for fillet/chamfer");
            }
            if (!matches) {
                break;
            }
        }
        if (matches) {
            matched_indexes.push_back(edge_index);
        }
    }

    if (matched_indexes.empty()) {
        throw EvalError("edge selector matched no edges");
    }
    return matched_indexes;
}

std::vector<TopoDS_Face> resolve_face_targets(
    const std::string& part_id,
    const TopoDS_Shape& shape,
    const std::vector<std::string>& requested_target_ids
) {
    std::vector<TopoDS_Face> faces;
    std::vector<std::string> face_target_ids;
    std::vector<std::string> face_stable_ids;
    std::map<std::string, int> stable_counts;
    int face_index = 0;
    for (TopExp_Explorer explorer(shape, TopAbs_FACE); explorer.More(); explorer.Next(), ++face_index) {
        TopoDS_Face face = TopoDS::Face(explorer.Current());
        std::string target_id = face_target_id(part_id, face_index, face);
        std::string stable_id = stable_face_target_id(target_id);
        faces.push_back(face);
        face_target_ids.push_back(target_id);
        face_stable_ids.push_back(stable_id);
        stable_counts[stable_id] += 1;
    }

    std::vector<TopoDS_Face> matched_faces;
    std::vector<int> matched_indexes;
    std::vector<std::string> matched_target_ids;
    for (const std::string& requested_target_id : requested_target_ids) {
        bool matched = false;
        for (std::size_t candidate_index = 0; candidate_index < face_target_ids.size(); ++candidate_index) {
            if (face_target_ids[candidate_index] != requested_target_id) {
                continue;
            }
            if (std::find(matched_indexes.begin(), matched_indexes.end(), static_cast<int>(candidate_index)) ==
                matched_indexes.end()) {
                matched_faces.push_back(faces[candidate_index]);
                matched_indexes.push_back(static_cast<int>(candidate_index));
            }
            matched_target_ids.push_back(requested_target_id);
            matched = true;
            break;
        }
        if (matched) {
            continue;
        }
        std::string requested_stable_id = stable_face_target_id(requested_target_id);
        if (stable_counts[requested_stable_id] > 1) {
            throw EvalError(
                std::string("face selector ambiguously matched stable face target: ") +
                requested_target_id
            );
        }
        for (std::size_t candidate_index = 0; candidate_index < face_stable_ids.size(); ++candidate_index) {
            if (face_stable_ids[candidate_index] != requested_stable_id) {
                continue;
            }
            if (std::find(matched_indexes.begin(), matched_indexes.end(), static_cast<int>(candidate_index)) ==
                matched_indexes.end()) {
                matched_faces.push_back(faces[candidate_index]);
                matched_indexes.push_back(static_cast<int>(candidate_index));
            }
            matched_target_ids.push_back(requested_target_id);
            matched = true;
            break;
        }
        if (!matched) {
            throw EvalError(
                std::string("face selector did not match target ids: ") + requested_target_id
            );
        }
    }
    if (matched_target_ids.size() != requested_target_ids.size()) {
        throw EvalError("face selector ambiguously matched stable face target");
    }
    if (matched_faces.empty()) {
        throw EvalError("face selector did not match target ids");
    }
    return matched_faces;
}

double selector_axis_min(
    SelectorAxis axis,
    double xmin,
    double ymin,
    double zmin
) {
    switch (axis) {
        case SelectorAxis::X:
            return xmin;
        case SelectorAxis::Y:
            return ymin;
        case SelectorAxis::Z:
            return zmin;
    }
    return 0.0;
}

double selector_axis_max(
    SelectorAxis axis,
    double xmax,
    double ymax,
    double zmax
) {
    switch (axis) {
        case SelectorAxis::X:
            return xmax;
        case SelectorAxis::Y:
            return ymax;
        case SelectorAxis::Z:
            return zmax;
    }
    return 0.0;
}

std::vector<TopoDS_Face> resolve_face_clauses(
    const TopoDS_Shape& shape,
    const std::vector<SelectorClause>& clauses
) {
    if (clauses.empty()) {
        throw EvalError("face selector clauses cannot be empty");
    }

    Bnd_Box shape_box;
    BRepBndLib::Add(shape, shape_box);
    double xmin = 0.0;
    double ymin = 0.0;
    double zmin = 0.0;
    double xmax = 0.0;
    double ymax = 0.0;
    double zmax = 0.0;
    shape_box.Get(xmin, ymin, zmin, xmax, ymax, zmax);
    double tol = std::max(
        xmax - xmin,
        std::max(ymax - ymin, std::max(zmax - zmin, 1.0))
    ) * 1.0e-6;
    constexpr double area_tol = 1.0e-6;

    std::vector<TopoDS_Face> faces;
    std::vector<double> face_areas;
    std::vector<int> candidate_indexes;

    for (TopExp_Explorer explorer(shape, TopAbs_FACE); explorer.More(); explorer.Next()) {
        TopoDS_Face face = TopoDS::Face(explorer.Current());
        BRepAdaptor_Surface surface(face);
        bool is_planar = surface.GetType() == GeomAbs_Plane;

        Bnd_Box face_box;
        BRepBndLib::Add(face, face_box);
        double face_xmin = 0.0;
        double face_ymin = 0.0;
        double face_zmin = 0.0;
        double face_xmax = 0.0;
        double face_ymax = 0.0;
        double face_zmax = 0.0;
        face_box.Get(face_xmin, face_ymin, face_zmin, face_xmax, face_ymax, face_zmax);

        GProp_GProps props;
        BRepGProp::SurfaceProperties(face, props);
        double area = props.Mass();

        bool matches = true;
        for (const SelectorClause& clause : clauses) {
            switch (clause.type) {
                case SelectorClauseType::Boundary: {
                    if (!clause.axis.has_value() || !clause.bound.has_value()) {
                        throw EvalError("face boundary selector missing axis or bound");
                    }
                    double shape_min = selector_axis_min(*clause.axis, xmin, ymin, zmin);
                    double shape_max = selector_axis_max(*clause.axis, xmax, ymax, zmax);
                    double face_min = selector_axis_min(*clause.axis, face_xmin, face_ymin, face_zmin);
                    double face_max = selector_axis_max(*clause.axis, face_xmax, face_ymax, face_zmax);
                    double shape_bound = *clause.bound == SelectorBound::Min ? shape_min : shape_max;
                    matches = matches &&
                        std::abs(face_min - shape_bound) <= tol &&
                        std::abs(face_max - shape_bound) <= tol;
                    break;
                }
                case SelectorClauseType::Planar:
                    matches = matches && is_planar;
                    break;
                case SelectorClauseType::Normal: {
                    if (!clause.axis.has_value()) {
                        throw EvalError("face normal selector missing axis");
                    }
                    double face_min = selector_axis_min(*clause.axis, face_xmin, face_ymin, face_zmin);
                    double face_max = selector_axis_max(*clause.axis, face_xmax, face_ymax, face_zmax);
                    matches = matches && is_planar && (face_max - face_min) <= tol;
                    break;
                }
                case SelectorClauseType::Area:
                    break;
                default:
                    throw EvalError("unsupported face selector clause for shell");
            }
            if (!matches) {
                break;
            }
        }

        if (matches) {
            faces.push_back(face);
            face_areas.push_back(area);
            candidate_indexes.push_back(static_cast<int>(faces.size()) - 1);
        }
    }

    if (candidate_indexes.empty()) {
        throw EvalError("face selector matched no faces");
    }

    for (const SelectorClause& clause : clauses) {
        if (clause.type != SelectorClauseType::Area) {
            continue;
        }
        if (!clause.rank.has_value()) {
            throw EvalError("face area selector missing rank");
        }
        double target_area = face_areas[static_cast<std::size_t>(candidate_indexes.front())];
        for (int candidate_index : candidate_indexes) {
            double area = face_areas[static_cast<std::size_t>(candidate_index)];
            if (*clause.rank == SelectorAreaRank::Min) {
                target_area = std::min(target_area, area);
            } else {
                target_area = std::max(target_area, area);
            }
        }
        std::vector<int> filtered_indexes;
        for (int candidate_index : candidate_indexes) {
            double area = face_areas[static_cast<std::size_t>(candidate_index)];
            if (std::abs(area - target_area) <= area_tol) {
                filtered_indexes.push_back(candidate_index);
            }
        }
        candidate_indexes = std::move(filtered_indexes);
        if (candidate_indexes.empty()) {
            throw EvalError("face selector matched no faces");
        }
    }

    std::vector<TopoDS_Face> matched_faces;
    matched_faces.reserve(candidate_indexes.size());
    for (int candidate_index : candidate_indexes) {
        matched_faces.push_back(faces[static_cast<std::size_t>(candidate_index)]);
    }
    return matched_faces;
}

std::optional<double> optional_number_keyword(const Command& command, const std::string& name) {
    for (const auto& keyword : command.keywords) {
        if (keyword.name == name && keyword.kind == Keyword::Kind::Arg
            && keyword.value.kind == Arg::Kind::Number) {
            return keyword.value.number_value;
        }
    }
    return std::nullopt;
}

TopoDS_Shape fillet_shape(
    const std::string& part_id,
    const TopoDS_Shape& shape,
    double radius,
    std::optional<double> radius2,
    const std::optional<SelectorPayload>& selector
) {
    std::vector<TopoDS_Edge> edges;
    if (!selector.has_value()) {
        TopTools_IndexedMapOfShape edge_map;
        TopExp::MapShapes(shape, TopAbs_EDGE, edge_map);
        for (int edge_ordinal = 1; edge_ordinal <= edge_map.Extent(); ++edge_ordinal) {
            edges.push_back(TopoDS::Edge(edge_map.FindKey(edge_ordinal)));
        }
    } else {
        std::vector<int> matched_indexes;
        if (selector->type == SelectorPayloadType::TargetIds) {
            matched_indexes = resolve_edge_target_indexes(part_id, shape, selector->target_ids);
        } else if (selector->type == SelectorPayloadType::Clauses) {
            matched_indexes = resolve_edge_clauses(shape, selector->clauses);
        } else {
            throw EvalError("fillet `:edges` selector payload unsupported");
        }
        TopTools_IndexedMapOfShape edge_map;
        TopExp::MapShapes(shape, TopAbs_EDGE, edge_map);
        for (int edge_ordinal = 1; edge_ordinal <= edge_map.Extent(); ++edge_ordinal) {
            int edge_index = edge_ordinal - 1;
            if (std::find(matched_indexes.begin(), matched_indexes.end(), edge_index) == matched_indexes.end()) {
                continue;
            }
            edges.push_back(TopoDS::Edge(edge_map.FindKey(edge_ordinal)));
        }
    }
    if (edges.empty()) {
        throw EvalError("fillet found no edges");
    }

    auto try_build = [&](double attempt_radius, std::optional<double> attempt_radius2)
        -> std::optional<TopoDS_Shape> {
        try {
            BRepFilletAPI_MakeFillet fillet(shape);
            for (const TopoDS_Edge& edge : edges) {
                if (attempt_radius2.has_value()) {
                    fillet.Add(attempt_radius, *attempt_radius2, edge);
                } else {
                    fillet.Add(attempt_radius, edge);
                }
            }
            fillet.Build();
            if (!fillet.IsDone()) {
                return std::nullopt;
            }
            TopoDS_Shape result = fillet.Shape();
            if (result.IsNull()) {
                return std::nullopt;
            }
            return result;
        } catch (const Standard_Failure&) {
            return std::nullopt;
        }
    };

    if (std::optional<TopoDS_Shape> result = try_build(radius, radius2)) {
        return *result;
    }
    std::optional<double> retry_radius2 = radius2.has_value()
        ? std::optional<double>(*radius2 * 0.5)
        : std::nullopt;
    if (std::optional<TopoDS_Shape> result = try_build(radius * 0.5, retry_radius2)) {
        return *result;
    }
    return shape;
}

TopoDS_Shape chamfer_shape(
    const std::string& part_id,
    const TopoDS_Shape& shape,
    double distance,
    const std::optional<SelectorPayload>& selector
) {
    BRepFilletAPI_MakeChamfer chamfer(shape);
    if (!selector.has_value()) {
        TopTools_IndexedMapOfShape edge_map;
        TopExp::MapShapes(shape, TopAbs_EDGE, edge_map);
        int edge_count = 0;
        for (int edge_ordinal = 1; edge_ordinal <= edge_map.Extent(); ++edge_ordinal) {
            chamfer.Add(distance, TopoDS::Edge(edge_map.FindKey(edge_ordinal)));
            ++edge_count;
        }
        if (edge_count == 0) {
            throw EvalError("chamfer found no edges");
        }
    } else {
        std::vector<int> matched_indexes;
        if (selector->type == SelectorPayloadType::TargetIds) {
            matched_indexes = resolve_edge_target_indexes(part_id, shape, selector->target_ids);
        } else if (selector->type == SelectorPayloadType::Clauses) {
            matched_indexes = resolve_edge_clauses(shape, selector->clauses);
        } else {
            throw EvalError("chamfer `:edges` selector payload unsupported");
        }
        TopTools_IndexedMapOfShape edge_map;
        TopExp::MapShapes(shape, TopAbs_EDGE, edge_map);
        for (int edge_ordinal = 1; edge_ordinal <= edge_map.Extent(); ++edge_ordinal) {
            int edge_index = edge_ordinal - 1;
            if (std::find(matched_indexes.begin(), matched_indexes.end(), edge_index) == matched_indexes.end()) {
                continue;
            }
            chamfer.Add(distance, TopoDS::Edge(edge_map.FindKey(edge_ordinal)));
        }
    }
    return chamfer.Shape();
}

TopoDS_Shape shell_shape(
    const std::string& part_id,
    const TopoDS_Shape& shape,
    double thickness,
    const std::optional<SelectorPayload>& selector
) {
    double offset = -std::abs(thickness);
    if (!selector.has_value()) {
        TopTools_ListOfShape closing_faces;
        double top_z = -1.0e100;
        for (TopExp_Explorer face_explorer(shape, TopAbs_FACE); face_explorer.More(); face_explorer.Next()) {
            TopoDS_Face face = TopoDS::Face(face_explorer.Current());
            BRepAdaptor_Surface surface(face);
            if (surface.GetType() != GeomAbs_Plane) {
                continue;
            }
            Bnd_Box face_box;
            BRepBndLib::Add(face, face_box);
            Standard_Real xmin, ymin, zmin, xmax, ymax, zmax;
            face_box.Get(xmin, ymin, zmin, xmax, ymax, zmax);
            if (zmax > top_z + 1.0e-7) {
                closing_faces.Clear();
                top_z = zmax;
            }
            if (std::abs(zmax - top_z) <= 1.0e-7) {
                closing_faces.Append(face);
            }
        }
        if (closing_faces.IsEmpty()) {
            BRepOffsetAPI_MakeOffsetShape inner_offset;
            inner_offset.PerformByJoin(
                shape,
                offset,
                0.05,
                BRepOffset_Skin,
                false,
                false,
                GeomAbs_Intersection,
                true
            );
            return BRepAlgoAPI_Cut(shape, inner_offset.Shape()).Shape();
        }
        BRepOffsetAPI_MakeThickSolid shell;
        shell.MakeThickSolidByJoin(
            shape,
            closing_faces,
            offset,
            0.05,
            BRepOffset_Skin,
            false,
            false,
            GeomAbs_Intersection,
            true
        );
        return shell.Shape();
    }
    std::vector<TopoDS_Face> matched_faces;
    if (selector->type == SelectorPayloadType::TargetIds) {
        if (selector->target_ids.empty()) {
            throw EvalError("shell `:faces` target ids cannot be empty");
        }
        matched_faces = resolve_face_targets(part_id, shape, selector->target_ids);
    } else if (selector->type == SelectorPayloadType::Clauses) {
        matched_faces = resolve_face_clauses(shape, selector->clauses);
    } else {
        throw EvalError("shell `:faces` selector payload unsupported");
    }
    TopTools_ListOfShape closing_faces;
    for (const auto& face : matched_faces) {
        closing_faces.Append(face);
    }
    BRepOffsetAPI_MakeThickSolid shell;
    shell.MakeThickSolidByJoin(
        shape,
        closing_faces,
        offset,
        0.05,
        BRepOffset_Skin,
        false,
        false,
        GeomAbs_Intersection,
        true
    );
    return shell.Shape();
}

TopoDS_Shape compound_shapes(const std::vector<TopoDS_Shape>& shapes) {
    BRep_Builder builder;
    TopoDS_Compound compound;
    builder.MakeCompound(compound);
    for (const auto& shape : shapes) {
        builder.Add(compound, shape);
    }
    return compound;
}

TopoDS_Shape translate_shape(const TopoDS_Shape& shape, double x, double y, double z) {
    gp_Trsf trsf;
    trsf.SetTranslation(gp_Vec(x, y, z));
    return BRepBuilderAPI_Transform(shape, trsf, true).Shape();
}

TopoDS_Shape rotate_shape(const TopoDS_Shape& shape, double x_degrees, double y_degrees, double z_degrees) {
    gp_Trsf trsf_x;
    trsf_x.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(1, 0, 0)), x_degrees * M_PI / 180.0);
    TopoDS_Shape after_x = BRepBuilderAPI_Transform(shape, trsf_x, true).Shape();
    gp_Trsf trsf_y;
    trsf_y.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 1, 0)), y_degrees * M_PI / 180.0);
    TopoDS_Shape after_y = BRepBuilderAPI_Transform(after_x, trsf_y, true).Shape();
    gp_Trsf trsf_z;
    trsf_z.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), z_degrees * M_PI / 180.0);
    return BRepBuilderAPI_Transform(after_y, trsf_z, true).Shape();
}

TopoDS_Shape scale_shape(const TopoDS_Shape& shape, double x, double y, double z) {
    gp_GTrsf gtrsf;
    gtrsf.SetValue(1, 1, x);
    gtrsf.SetValue(2, 2, y);
    gtrsf.SetValue(3, 3, z);
    return BRepBuilderAPI_GTransform(shape, gtrsf, true).Shape();
}

TopoDS_Shape mirror_shape(const TopoDS_Shape& shape, const std::string& axis, double offset) {
    gp_Pnt point;
    gp_Dir normal;
    std::string lowered = axis;
    std::transform(lowered.begin(), lowered.end(), lowered.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    if (lowered == "x") {
        point = gp_Pnt(offset, 0, 0);
        normal = gp_Dir(1, 0, 0);
    } else if (lowered == "y") {
        point = gp_Pnt(0, offset, 0);
        normal = gp_Dir(0, 1, 0);
    } else if (lowered == "z") {
        point = gp_Pnt(0, 0, offset);
        normal = gp_Dir(0, 0, 1);
    } else {
        throw EvalError("mirror unsupported axis `" + axis + "`");
    }
    gp_Trsf trsf;
    trsf.SetMirror(gp_Ax2(point, normal));
    return BRepBuilderAPI_Transform(shape, trsf, true).Shape();
}

TopoDS_Shape make_path_wire(const std::vector<std::array<double, 3>>& points) {
    BRepBuilderAPI_MakePolygon path;
    for (const auto& point : points) {
        path.Add(gp_Pnt(point[0], point[1], point[2]));
    }
    return path.Wire();
}

TopoDS_Shape make_helix_path_wire(double radius, double pitch, double height, bool lefthand,
                                  std::optional<double> top_radius_arg = std::nullopt) {
    if (!std::isfinite(radius) || radius <= 0.0) {
        throw EvalError("helix-path radius must be positive");
    }
    if (!std::isfinite(pitch) || pitch <= 0.0) {
        throw EvalError("helix-path pitch must be positive");
    }
    if (!std::isfinite(height) || height <= 0.0) {
        throw EvalError("helix-path height must be positive");
    }
    const double top_radius = top_radius_arg.value_or(radius);
    if (!std::isfinite(top_radius) || top_radius <= 0.0) {
        throw EvalError("helix-path top radius must be positive");
    }
    const double turns = height / pitch;
    double end_angle = (lefthand ? -1.0 : 1.0) * 6.28318530717958647692 * turns;
    double end_v = height;
    Handle(Geom_Surface) surface;
    if (top_radius == radius) {
        surface = new Geom_CylindricalSurface(
            gp_Ax3(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), radius);
    } else {
        const double semi_angle = std::atan2(top_radius - radius, height);
        end_angle /= std::cos(semi_angle);
        end_v = height / std::cos(semi_angle);
        surface = new Geom_ConicalSurface(
            gp_Ax3(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), semi_angle, radius);
    }
    Handle(Geom2d_TrimmedCurve) curve2d =
        GCE2d_MakeSegment(gp_Pnt2d(0, 0), gp_Pnt2d(end_angle, end_v)).Value();
    TopoDS_Edge edge = BRepBuilderAPI_MakeEdge(curve2d, surface).Edge();
    // The edge so far only carries a pcurve on the cylinder; sweeping (and other
    // 3D consumers) need an explicit 3D curve or MakePipeShell throws
    // Standard_NullObject. Build it the way build123d's Edge.make_helix does.
    BRepLib::BuildCurves3d(edge);
    return BRepBuilderAPI_MakeWire(edge).Wire();
}

TopoDS_Shape make_bezier_path_wire(const std::vector<std::array<double, 3>>& points) {
    if (points.size() < 4 || (points.size() - 1) % 3 != 0) {
        throw EvalError("bezier-path expects 3n+1 control points");
    }
    // Flatten cubic Bézier segments to a polyline of linear edges.
    // Rationale: constructing `Geom_BezierCurve` in this translation unit
    // duplicates its typeinfo against the OCCT dylib, which corrupts
    // `dynamic_cast` and C++ catch-by-type dispatch across the whole runner.
    // The sweep implementation handles this sampled representation explicitly.
    constexpr int SAMPLES = 16;
    BRepBuilderAPI_MakeWire wire_builder;
    gp_Pnt prev;
    bool have_prev = false;
    for (std::size_t start = 0; start < points.size() - 1; start += 3) {
        const auto& p0 = points[start];
        const auto& p1 = points[start + 1];
        const auto& p2 = points[start + 2];
        const auto& p3 = points[start + 3];
        if (!have_prev) {
            prev = gp_Pnt(p0[0], p0[1], p0[2]);
            have_prev = true;
        }
        for (int step = 1; step <= SAMPLES; ++step) {
            double t = static_cast<double>(step) / SAMPLES;
            double mt = 1.0 - t;
            double a = mt * mt * mt;
            double b = 3.0 * mt * mt * t;
            double c = 3.0 * mt * t * t;
            double d = t * t * t;
            gp_Pnt next(
                a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
                a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1],
                a * p0[2] + b * p1[2] + c * p2[2] + d * p3[2]);
            wire_builder.Add(BRepBuilderAPI_MakeEdge(prev, next).Edge());
            prev = next;
        }
    }
    return wire_builder.Wire();
}

struct BsplineArgs {
    std::vector<std::array<double, 2>> points;
    bool closed = false;
    std::optional<std::vector<std::array<double, 2>>> tangents;
    std::optional<std::vector<double>> tangent_scalars;
};

BsplineArgs bspline_args(const Command& command) {
    BsplineArgs result;
    result.points = require_point2_list(command.args, 0, "bspline", 2);
    if (command.args.size() > 1) {
        result.closed = require_bool_arg(command.args, 1, "bspline");
    }
    for (const auto& keyword : command.keywords) {
        if (keyword.kind != Keyword::Kind::Arg) {
            throw EvalError("bspline keywords expect arg values only");
        }
        if (keyword.name == "closed") {
            if (keyword.value.kind != Arg::Kind::Boolean) {
                throw EvalError("bspline :closed expects boolean");
            }
            result.closed = keyword.value.bool_value;
            continue;
        }
        if (keyword.name == "tangents") {
            result.tangents = require_point2_list_arg(keyword.value, "bspline :tangents", 2);
            continue;
        }
        if (keyword.name == "tangent_scalars" || keyword.name == "tangent-scalars") {
            result.tangent_scalars = require_number_list_arg(keyword.value, "bspline :tangent-scalars");
            continue;
        }
        throw EvalError("bspline does not recognize `:" + keyword.name + "`");
    }
    if (result.tangents.has_value() &&
        result.tangents->size() != 2 &&
        result.tangents->size() != result.points.size()) {
        throw EvalError("bspline :tangents expects 2 entries or one per point");
    }
    if (result.tangent_scalars.has_value() &&
        result.tangent_scalars->size() != 2 &&
        result.tangent_scalars->size() != result.points.size()) {
        throw EvalError("bspline :tangent-scalars expects 2 entries or one per point");
    }
    if (result.points.size() < 3 && !result.tangents.has_value()) {
        throw EvalError("bspline requires at least three points unless tangents are supplied");
    }
    return result;
}

TopoDS_Shape make_bspline_shape(const BsplineArgs& args) {
    BRepBuilderAPI_MakeWire wire_builder;
    const auto& first = args.points.front();
    const auto& last = args.points.back();
    if (args.tangents.has_value()) {
        const auto& first_tangent = args.tangents->front();
        const auto& last_tangent = args.tangents->back();
        const double first_scale = args.tangent_scalars.has_value() && !args.tangent_scalars->empty()
            ? args.tangent_scalars->front()
            : 1.0;
        const double last_scale = args.tangent_scalars.has_value() && !args.tangent_scalars->empty()
            ? args.tangent_scalars->back()
            : 1.0;
        std::array<std::array<double, 2>, 4> bezier_poles{
            first,
            std::array<double, 2>{first[0] + first_tangent[0] * first_scale, first[1] + first_tangent[1] * first_scale},
            std::array<double, 2>{last[0] - last_tangent[0] * last_scale, last[1] - last_tangent[1] * last_scale},
            last,
        };
        TColgp_Array1OfPnt poles(1, 4);
        for (std::size_t index = 0; index < bezier_poles.size(); ++index) {
            poles.SetValue(static_cast<Standard_Integer>(index + 1), gp_Pnt(bezier_poles[index][0], bezier_poles[index][1], 0));
        }
        Handle(Geom_BezierCurve) curve = new Geom_BezierCurve(poles);
        wire_builder.Add(BRepBuilderAPI_MakeEdge(curve).Edge());
    } else {
        TColgp_Array1OfPnt poles(1, static_cast<Standard_Integer>(args.points.size()));
        for (std::size_t index = 0; index < args.points.size(); ++index) {
            poles.SetValue(static_cast<Standard_Integer>(index + 1), gp_Pnt(args.points[index][0], args.points[index][1], 0));
        }
        GeomAPI_PointsToBSpline bspline_builder(poles, 3, 8, GeomAbs_C2, 1.0e-4);
        Handle(Geom_BSplineCurve) curve = bspline_builder.Curve();
        wire_builder.Add(BRepBuilderAPI_MakeEdge(curve).Edge());
    }
    if (args.closed && distance2(first, last) > 1.0e-9) {
        wire_builder.Add(
            BRepBuilderAPI_MakeEdge(gp_Pnt(last[0], last[1], 0), gp_Pnt(first[0], first[1], 0)).Edge());
    }
    TopoDS_Wire wire = wire_builder.Wire();
    if (args.closed) {
        return BRepBuilderAPI_MakeFace(wire).Shape();
    }
    return wire;
}

TopoDS_Shape linear_array_shape(const TopoDS_Shape& shape, std::size_t count, double dx, double dy, double dz) {
    std::vector<TopoDS_Shape> items;
    items.reserve(count);
    for (std::size_t index = 0; index < count; ++index) {
        items.push_back(translate_shape(shape, dx * static_cast<double>(index), dy * static_cast<double>(index),
                                        dz * static_cast<double>(index)));
    }
    return compound_shapes(items);
}

TopoDS_Shape radial_array_shape(const TopoDS_Shape& shape, std::size_t count, double step_degrees, double radius) {
    std::vector<TopoDS_Shape> items;
    items.reserve(count);
    for (std::size_t index = 0; index < count; ++index) {
        gp_Trsf translate;
        translate.SetTranslation(gp_Vec(radius, 0, 0));
        gp_Trsf rotate;
        rotate.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)),
                           step_degrees * static_cast<double>(index) * M_PI / 180.0);
        rotate.Multiply(translate);
        items.push_back(BRepBuilderAPI_Transform(shape, rotate, true).Shape());
    }
    return compound_shapes(items);
}

TopoDS_Shape grid_array_shape(const TopoDS_Shape& shape, std::size_t rows, std::size_t cols, double dx, double dy) {
    std::vector<TopoDS_Shape> items;
    items.reserve(rows * cols);
    for (std::size_t row = 0; row < rows; ++row) {
        for (std::size_t col = 0; col < cols; ++col) {
            items.push_back(translate_shape(shape, dx * static_cast<double>(col), dy * static_cast<double>(row), 0));
        }
    }
    return compound_shapes(items);
}

TopoDS_Shape arc_array_shape(
    const TopoDS_Shape& shape,
    std::size_t count,
    double radius,
    double start_degrees,
    double end_degrees
) {
    std::vector<TopoDS_Shape> items;
    items.reserve(count);
    double denominator = static_cast<double>(std::max<std::size_t>(1, count - 1));
    for (std::size_t index = 0; index < count; ++index) {
        double angle = (start_degrees + (end_degrees - start_degrees) * static_cast<double>(index) / denominator) *
                       M_PI / 180.0;
        gp_Trsf translate;
        translate.SetTranslation(gp_Vec(radius, 0, 0));
        gp_Trsf rotate;
        rotate.SetRotation(gp_Ax1(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), angle);
        rotate.Multiply(translate);
        items.push_back(BRepBuilderAPI_Transform(shape, rotate, true).Shape());
    }
    return compound_shapes(items);
}

gp_Trsf make_frame_transform(const gp_Pnt& origin, const gp_Vec& x_hint, const gp_Vec& normal, const std::string& op) {
    gp_Vec z = normal;
    if (z.Magnitude() <= 1.0e-9) {
        throw EvalError(op + " expects a non-zero normal/tangent");
    }
    z.Normalize();
    if (x_hint.Magnitude() <= 1.0e-9) {
        throw EvalError(op + " expects a non-zero x/up vector");
    }
    gp_Vec x = x_hint - z.Multiplied(x_hint.Dot(z));
    if (x.Magnitude() <= 1.0e-9) {
        throw EvalError(op + " expects x/up vector not parallel to normal/tangent");
    }
    x.Normalize();
    gp_Vec y = z.Crossed(x);
    if (y.Magnitude() <= 1.0e-9) {
        throw EvalError(op + " failed to build frame basis");
    }
    y.Normalize();
    x = y.Crossed(z);
    x.Normalize();

    gp_Trsf frame;
    frame.SetValues(
        x.X(), y.X(), z.X(), origin.X(),
        x.Y(), y.Y(), z.Y(), origin.Y(),
        x.Z(), y.Z(), z.Z(), origin.Z()
    );
    return frame;
}

gp_Trsf make_plane_frame(const PlaneArgs& args) {
    return make_frame_transform(
        gp_Pnt(args.origin[0], args.origin[1], args.origin[2]),
        gp_Vec(args.x_axis[0], args.x_axis[1], args.x_axis[2]),
        gp_Vec(args.normal[0], args.normal[1], args.normal[2]),
        "plane"
    );
}

gp_Trsf make_location_frame(const gp_Trsf* base) {
    if (base) {
        return *base;
    }
    return gp_Trsf();
}

void apply_location_transform(
    gp_Trsf& frame,
    const std::array<double, 3>& offset,
    const std::array<double, 3>& rotate_degrees
) {
    gp_Trsf offset_transform;
    offset_transform.SetTranslation(gp_Vec(offset[0], offset[1], offset[2]));
    frame.Multiply(offset_transform);
    const std::array<gp_Dir, 3> axes = {gp_Dir(1, 0, 0), gp_Dir(0, 1, 0), gp_Dir(0, 0, 1)};
    for (std::size_t index = 0; index < axes.size(); ++index) {
        gp_Trsf rotation;
        rotation.SetRotation(
            gp_Ax1(gp_Pnt(0, 0, 0), axes[index]),
            rotate_degrees[index] * M_PI / 180.0
        );
        frame.Multiply(rotation);
    }
}

double path_frame_anchor_arg(const Arg& arg) {
    if (arg.kind == Arg::Kind::Number) {
        return std::min(1.0, std::max(0.0, arg.number_value));
    }
    if ((arg.kind == Arg::Kind::Symbol || arg.kind == Arg::Kind::Text) && arg.text_value == "start") {
        return 0.0;
    }
    if ((arg.kind == Arg::Kind::Symbol || arg.kind == Arg::Kind::Text) && arg.text_value == "end") {
        return 1.0;
    }
    throw EvalError("path-frame :at expects `start`, `end`, or a numeric 0..1 anchor");
}

struct PathFrameArgs {
    std::uint64_t path_ref = 0;
    double at = 1.0;
    std::array<double, 3> up{0.0, 0.0, 1.0};
};

PathFrameArgs path_frame_args(const Command& command) {
    if (command.args.size() != 1 || command.args[0].kind != Arg::Kind::Ref) {
        throw EvalError("path-frame expects one path reference");
    }
    PathFrameArgs result;
    result.path_ref = command.args[0].ref_value;
    for (const auto& keyword : command.keywords) {
        if (keyword.kind != Keyword::Kind::Arg) {
            throw EvalError("path-frame keywords expect arg values only");
        }
        if (keyword.name == "at") {
            result.at = path_frame_anchor_arg(keyword.value);
            continue;
        }
        if (keyword.name == "up") {
            if (keyword.value.kind != Arg::Kind::Point3) {
                throw EvalError("path-frame :up expects a 3D vector");
            }
            result.up = keyword.value.point3_value;
            continue;
        }
        throw EvalError("path-frame does not recognize `:" + keyword.name + "`");
    }
    return result;
}

gp_Trsf make_path_frame(const TopoDS_Shape& path, double at, std::array<double, 3> up) {
    std::vector<TopoDS_Edge> edges;
    std::vector<double> edge_lengths;
    double total_length = 0.0;
    for (TopExp_Explorer explorer(path, TopAbs_EDGE); explorer.More(); explorer.Next()) {
        TopoDS_Edge edge = TopoDS::Edge(explorer.Current());
        GProp_GProps props;
        BRepGProp::LinearProperties(edge, props);
        double length = std::max(0.0, props.Mass());
        edges.push_back(edge);
        edge_lengths.push_back(length);
        total_length += length;
    }
    if (edges.empty() || total_length <= 1.0e-9) {
        throw EvalError("path-frame expects a path with at least one edge");
    }

    double target_length = std::min(1.0, std::max(0.0, at)) * total_length;
    std::size_t edge_index = edges.size() - 1;
    double local_t = 1.0;
    double walked_length = 0.0;
    for (std::size_t candidate = 0; candidate < edges.size(); ++candidate) {
        double length = edge_lengths[candidate];
        if (target_length <= walked_length + length || candidate + 1 == edges.size()) {
            edge_index = candidate;
            local_t = length <= 1.0e-9 ? 0.0 : (target_length - walked_length) / length;
            local_t = std::min(1.0, std::max(0.0, local_t));
            break;
        }
        walked_length += length;
    }

    TopoDS_Edge edge = edges[edge_index];
    BRepAdaptor_Curve curve(edge);
    gp_Pnt origin;
    gp_Vec derivative;
    double first = curve.FirstParameter();
    double last = curve.LastParameter();
    curve.D1(first + (last - first) * local_t, origin, derivative);
    if (derivative.Magnitude() <= 1.0e-9) {
        throw EvalError("path-frame got a zero-length tangent");
    }

    gp_Vec tangent = derivative;
    tangent.Normalize();
    gp_Vec up_vec(up[0], up[1], up[2]);
    gp_Vec x_hint = up_vec - tangent.Multiplied(up_vec.Dot(tangent));
    if (x_hint.Magnitude() <= 1.0e-9) {
        gp_Vec fallback(0, 1, 0);
        x_hint = fallback - tangent.Multiplied(fallback.Dot(tangent));
    }
    return make_frame_transform(origin, x_hint, tangent, "path-frame");
}

TopoDS_Shape place_shape(const gp_Trsf& frame, const TopoDS_Shape& shape) {
    return BRepBuilderAPI_Transform(shape, frame, true).Shape();
}

const TopoDS_Shape& lookup_shape(
    const std::map<std::uint64_t, SlotValue>& slots,
    std::uint64_t slot,
    const std::string& op
) {
    auto it = slots.find(slot);
    if (it == slots.end()) {
        throw EvalError(op + " references unknown slot");
    }
    if (it->second.kind != SlotValue::Kind::Shape) {
        throw EvalError(op + " expects a shape slot");
    }
    return it->second.shape;
}

const gp_Trsf& lookup_frame(
    const std::map<std::uint64_t, SlotValue>& slots,
    std::uint64_t slot,
    const std::string& op
) {
    auto it = slots.find(slot);
    if (it == slots.end()) {
        throw EvalError(op + " references unknown slot");
    }
    if (it->second.kind != SlotValue::Kind::Frame) {
        throw EvalError(op + " expects a frame slot");
    }
    return it->second.frame;
}

void require_manifold_status(const manifold::Manifold& value, const std::string& op) {
    if (value.Status() != manifold::Manifold::Error::NoError) {
        throw EvalError(op + " produced invalid Manifold status " +
                        std::to_string(static_cast<int>(value.Status())));
    }
}

const manifold::Manifold& lookup_manifold(
    const std::map<std::uint64_t, SlotValue>& slots,
    std::uint64_t slot,
    const std::string& op
) {
    auto it = slots.find(slot);
    if (it == slots.end()) {
        throw EvalError(op + " references unknown slot");
    }
    if (it->second.kind != SlotValue::Kind::Manifold) {
        throw EvalError(op + " expects a Manifold slot");
    }
    return it->second.manifold;
}

manifold::Manifold make_indexed_manifold(const Command& command, ExecutionContext& context) {
    const std::string op = "import-indexed-mesh";
    if (command.args.size() != 3 || command.args[0].kind != Arg::Kind::List ||
        command.args[1].kind != Arg::Kind::List ||
        (command.args[2].kind != Arg::Kind::Text && command.args[2].kind != Arg::Kind::Symbol) ||
        command.args[2].text_value.empty()) {
        throw EvalError(op + " expects vertices, triangles, and contentDigest");
    }
    StageExecutionTimer stage_timer(context, "validate");
    manifold::MeshGL64 mesh;
    mesh.numProp = 3;
    mesh.vertProperties.reserve(command.args[0].list_value.size() * 3);
    for (const Arg& vertex : command.args[0].list_value) {
        const auto point = require_point3_arg(vertex, op);
        for (double coordinate : point) {
            if (!std::isfinite(coordinate)) {
                throw EvalError(op + " vertex must be finite");
            }
            mesh.vertProperties.push_back(coordinate);
        }
    }
    if (mesh.vertProperties.empty()) {
        throw EvalError(op + " requires at least one vertex");
    }
    using MeshIndex = typename decltype(mesh.triVerts)::value_type;
    mesh.triVerts.reserve(command.args[1].list_value.size() * 3);
    for (const Arg& triangle : command.args[1].list_value) {
        if (triangle.kind != Arg::Kind::List || triangle.list_value.size() != 3) {
            throw EvalError(op + " triangles must contain exactly three indices");
        }
        for (const Arg& raw_index : triangle.list_value) {
            if (raw_index.kind != Arg::Kind::Number || !std::isfinite(raw_index.number_value) ||
                raw_index.number_value < 0.0 || std::floor(raw_index.number_value) != raw_index.number_value ||
                raw_index.number_value > static_cast<double>(std::numeric_limits<MeshIndex>::max()) ||
                raw_index.number_value >= static_cast<double>(mesh.NumVert())) {
                throw EvalError(op + " triangle index is out of bounds");
            }
            mesh.triVerts.push_back(static_cast<MeshIndex>(raw_index.number_value));
        }
    }
    if (mesh.triVerts.empty()) {
        throw EvalError(op + " requires at least one triangle");
    }
    manifold::Manifold result(mesh);
    require_manifold_status(result, op);
    return result;
}

manifold::Manifold manifold_from_occt_shape(
    const TopoDS_Shape& shape, const std::string& op, ExecutionContext& context
) {
    StageExecutionTimer stage_timer(context, "mesh");
    BRepMesh_IncrementalMesh mesher(shape, 0.04, Standard_False, 0.1, Standard_True);
    (void)mesher;
    manifold::MeshGL64 mesh;
    mesh.numProp = 3;
    std::map<std::string, std::uint64_t> vertex_ids;
    auto vertex_id = [&](const gp_Pnt& point) {
        std::ostringstream key;
        key << std::setprecision(17) << point.X() << ':' << point.Y() << ':' << point.Z();
        auto [it, inserted] = vertex_ids.emplace(key.str(), vertex_ids.size());
        if (inserted) {
            mesh.vertProperties.push_back(point.X());
            mesh.vertProperties.push_back(point.Y());
            mesh.vertProperties.push_back(point.Z());
        }
        return it->second;
    };
    for (TopExp_Explorer explorer(shape, TopAbs_FACE); explorer.More(); explorer.Next()) {
        const TopoDS_Face face = TopoDS::Face(explorer.Current());
        TopLoc_Location location;
        const Handle(Poly_Triangulation) triangulation = BRep_Tool::Triangulation(face, location);
        if (triangulation.IsNull()) continue;
        const gp_Trsf transform = location.Transformation();
        for (Standard_Integer index = 1; index <= triangulation->NbTriangles(); ++index) {
            Standard_Integer a, b, c;
            triangulation->Triangle(index).Get(a, b, c);
            std::uint64_t ia = vertex_id(triangulation->Node(a).Transformed(transform));
            std::uint64_t ib = vertex_id(triangulation->Node(b).Transformed(transform));
            std::uint64_t ic = vertex_id(triangulation->Node(c).Transformed(transform));
            if (face.Orientation() == TopAbs_REVERSED) std::swap(ib, ic);
            mesh.triVerts.push_back(ia);
            mesh.triVerts.push_back(ib);
            mesh.triVerts.push_back(ic);
        }
    }
    if (mesh.triVerts.empty()) throw EvalError(op + " OCCT operand produced no triangles");
    manifold::Manifold result(mesh);
    require_manifold_status(result, op + " OCCT tessellation");
    return result;
}

TopoDS_Shape tessellated_step_shape_from_manifold(const manifold::Manifold& value) {
    require_manifold_status(value, "tessellated STEP conversion");
    const manifold::MeshGL64 mesh = value.GetMeshGL64();
    if (mesh.numProp < 3 || mesh.triVerts.empty() || mesh.triVerts.size() % 3 != 0) {
        throw EvalError("tessellated STEP conversion received invalid indexed geometry");
    }
    if (mesh.NumVert() > static_cast<std::size_t>(std::numeric_limits<Standard_Integer>::max()) ||
        mesh.NumTri() > static_cast<std::size_t>(std::numeric_limits<Standard_Integer>::max())) {
        throw EvalError("tessellated STEP conversion exceeds OCCT index limits");
    }
    Handle(Poly_Triangulation) triangulation = new Poly_Triangulation(
        static_cast<Standard_Integer>(mesh.NumVert()),
        static_cast<Standard_Integer>(mesh.NumTri()), Standard_False, Standard_False);
    for (std::size_t index = 0; index < mesh.NumVert(); ++index) {
        const std::size_t offset = index * mesh.numProp;
        triangulation->SetNode(static_cast<Standard_Integer>(index + 1), gp_Pnt(
            mesh.vertProperties[offset], mesh.vertProperties[offset + 1],
            mesh.vertProperties[offset + 2]));
    }
    for (std::size_t triangle = 0; triangle < mesh.NumTri(); ++triangle) {
        const std::size_t offset = triangle * 3;
        const auto index = [&](std::size_t side) {
            const std::uint64_t value = mesh.triVerts[offset + side];
            if (value >= mesh.NumVert()) {
                throw EvalError("tessellated STEP conversion triangle index is out of bounds");
            }
            return static_cast<Standard_Integer>(value + 1);
        };
        triangulation->SetTriangle(static_cast<Standard_Integer>(triangle + 1),
            Poly_Triangle(index(0), index(1), index(2)));
    }
    triangulation->Deflection(0.04);  // Matches canonical STL chord error below.
    TopoDS_Face face;
    BRep_Builder builder;
    builder.MakeFace(face, triangulation);
    return face;
}

const SlotValue& lookup_geometry_slot(
    const std::map<std::uint64_t, SlotValue>& slots,
    std::uint64_t slot,
    const std::string& op
) {
    auto found = slots.find(slot);
    if (found == slots.end()) throw EvalError(op + " references unknown slot");
    if (found->second.kind == SlotValue::Kind::Frame) {
        throw EvalError(op + " expects shape or Manifold geometry");
    }
    return found->second;
}

SlotValue union_geometry_values(
    const std::vector<SlotValue>& values,
    ExecutionContext& context,
    bool force_mesh = false
) {
    if (values.empty()) throw EvalError("union requires at least one geometry operand");
    if (values.size() == 1) return values.front();
    const bool mesh_domain = force_mesh ||
        std::any_of(values.begin(), values.end(), [](const SlotValue& value) {
            return value.kind == SlotValue::Kind::Manifold;
        });
    if (!mesh_domain) {
        std::vector<TopoDS_Shape> shapes;
        shapes.reserve(values.size());
        for (const SlotValue& value : values) shapes.push_back(value.shape);
        return SlotValue::shape_value(fuse_shapes(shapes, context));
    }
    StageExecutionTimer stage_timer(context, "boolean");
    std::vector<manifold::Manifold> operands;
    operands.reserve(values.size());
    for (const SlotValue& value : values) {
        operands.push_back(value.kind == SlotValue::Kind::Manifold
            ? value.manifold
            : manifold_from_occt_shape(value.shape, "union", context));
    }
    manifold::Manifold result = manifold::Manifold::BatchBoolean(
        operands, manifold::OpType::Add);
    require_manifold_status(result, "union");
    {
        std::lock_guard<std::mutex> lock(context.mutex);
        ++context.mesh_boolean_count;
    }
    return SlotValue::manifold_value(result);
}

void require_geometry_representation(
    const SlotValue& value,
    GeometryRepresentation expected,
    const std::string& label
) {
    const bool mesh = value.kind == SlotValue::Kind::Manifold;
    if (value.kind == SlotValue::Kind::Frame ||
        mesh != (expected == GeometryRepresentation::MeshDomain)) {
        throw EvalError(
            label + " produced representation `" +
            (mesh ? "meshDomain" : "analyticBrep") + "`, expected `" +
            geometry_representation_name(expected) + "`");
    }
}

SlotValue evaluate_command(
    const Command& command,
    const std::map<std::uint64_t, SlotValue>& slots,
    const std::string& part_id,
    bool force_mesh_booleans,
    ExecutionContext& context
) {
    const std::string op = command.op;
    auto get_ref_shape = [&](std::size_t index) -> const TopoDS_Shape& {
        if (index >= command.args.size()) {
            throw EvalError(op + " missing shape reference");
        }
        const Arg& arg = command.args[index];
        if (arg.kind != Arg::Kind::Ref) {
            throw EvalError(op + " expects shape reference");
        }
        return lookup_shape(slots, arg.ref_value, op);
    };
    auto get_ref_frame = [&](std::size_t index) -> const gp_Trsf& {
        if (index >= command.args.size()) {
            throw EvalError(op + " missing frame reference");
        }
        const Arg& arg = command.args[index];
        if (arg.kind != Arg::Kind::Ref) {
            throw EvalError(op + " expects frame reference");
        }
        return lookup_frame(slots, arg.ref_value, op);
    };
    auto get_ref_slot = [&](std::size_t index) -> const SlotValue& {
        if (index >= command.args.size()) {
            throw EvalError(op + " missing geometry reference");
        }
        const Arg& arg = command.args[index];
        if (arg.kind != Arg::Kind::Ref) {
            throw EvalError(op + " expects geometry reference");
        }
        auto input = slots.find(arg.ref_value);
        if (input == slots.end()) {
            throw EvalError(op + " references unknown slot");
        }
        if (input->second.kind == SlotValue::Kind::Frame) {
            throw EvalError(op + " expects shape or Manifold geometry");
        }
        return input->second;
    };

    if (!command.keywords.empty() && op != "box" && op != "sphere" && op != "cylinder" &&
        op != "cone" && op != "torus" && op != "wedge" && op != "profile" &&
        op != "extrude" && op != "plane" && op != "clip-box" && op != "clip-plane" && op != "fillet" &&
        op != "chamfer" && op != "shell" && op != "bspline" && op != "sweep" &&
        op != "draft" && op != "path-frame" && op != "location") {
        throw EvalError(op + " keywords unsupported yet");
    }

    if (op == "box") {
        BoxArgs args = box_args(command);
        return SlotValue::shape_value(make_box(args.width, args.depth, args.height, args.align));
    }
    if (op == "sphere") {
        SphereArgs args = sphere_args(command);
        return SlotValue::shape_value(make_sphere(args.radius, args.align));
    }
    if (op == "cylinder") {
        CylinderArgs args = cylinder_args(command);
        return SlotValue::shape_value(make_cylinder(args.radius, args.height, args.align));
    }
    if (op == "cone") {
        ConeArgs args = cone_args(command);
        return SlotValue::shape_value(
            make_cone(args.radius1, args.radius2, args.height, args.align)
        );
    }
    if (op == "torus") {
        TorusArgs args = torus_args(command);
        return SlotValue::shape_value(make_torus(args.major, args.minor, args.align));
    }
    if (op == "wedge") {
        WedgeArgs args = wedge_args(command);
        return SlotValue::shape_value(make_wedge(args.dims, args.align));
    }
    if (op == "rectangle") {
        const double width = require_number_arg(command.args, 0, op);
        const double height = require_number_arg(command.args, 1, op);
        return make_polygon_face({
            {-width / 2.0, -height / 2.0},
            {width / 2.0, -height / 2.0},
            {width / 2.0, height / 2.0},
            {-width / 2.0, height / 2.0},
        });
    }
    if (op == "ellipse") {
        return SlotValue::shape_value(make_ellipse_face(
            require_number_arg(command.args, 0, op),
            require_number_arg(command.args, 1, op)
        ));
    }
    if (op == "slot-overall") {
        return SlotValue::shape_value(make_slot_face(
            require_number_arg(command.args, 0, op),
            require_number_arg(command.args, 1, op)
        ));
    }
    if (op == "slot-arc") {
        return SlotValue::shape_value(make_slot_arc_face(
            require_number_arg(command.args, 0, op),
            require_number_arg(command.args, 1, op),
            require_number_arg(command.args, 2, op),
            require_number_arg(command.args, 3, op)
        ));
    }
    if (op == "circle") {
        return make_circle_face(require_number_arg(command.args, 0, op));
    }
    if (op == "rounded-rect") {
        return make_rounded_rect_face(require_number_arg(command.args, 0, op),
                                      require_number_arg(command.args, 1, op),
                                      require_number_arg(command.args, 2, op));
    }
    if (op == "rounded-polygon") {
        return make_rounded_polygon_face(require_point2_list(command.args, 0, op),
                                         require_number_arg(command.args, 1, op));
    }
    if (op == "polygon") {
        if (command.args.empty() || command.args[0].kind != Arg::Kind::List) {
            throw EvalError(op + " expects a list of points");
        }
        std::vector<std::array<double, 2>> points;
        for (const Arg& arg : command.args[0].list_value) {
            points.push_back(require_point2_arg(arg, op));
        }
        return make_polygon_face(points);
    }
    if (op == "profile") {
        ProfileRefs refs = profile_refs(command);
        std::vector<TopoDS_Shape> outer_shapes;
        outer_shapes.reserve(refs.outer.size());
        for (std::uint64_t ref : refs.outer) {
            outer_shapes.push_back(lookup_shape(slots, ref, op));
        }
        if (refs.soup) {
            return make_faces_from_wire_soup(outer_shapes);
        }
        std::vector<TopoDS_Shape> hole_shapes;
        hole_shapes.reserve(refs.holes.size());
        for (std::uint64_t ref : refs.holes) {
            hole_shapes.push_back(lookup_shape(slots, ref, op));
        }
        return make_profile_face(outer_shapes, hole_shapes);
    }
    if (op == "make-face") {
        if (command.args.size() != 1) {
            throw EvalError(op + " expects exactly one wire reference");
        }
        return make_face_from_shape(get_ref_shape(0), op);
    }
    if (op == "import-stl") {
        if (command.args.size() != 1 ||
            (command.args[0].kind != Arg::Kind::Text && command.args[0].kind != Arg::Kind::Symbol)) {
            throw EvalError(op + " expects a file path");
        }
        TopoDS_Shape shape;
        StlAPI_Reader reader;
        StageExecutionTimer stage_timer(context, "import");
        if (!reader.Read(shape, command.args[0].text_value.c_str())) {
            throw EvalError(op + " could not read STL file");
        }
        return shape;
    }
    if (op == "import-step") {
        // OpenSpec native-step-component-import, Direct OCCT ImportStep.
        // Locked STEP bytes load through STEPControl_Reader and must pass every
        // validation gate before a shape slot is published. This path MUST NOT
        // fall back to FreeCAD, STL conversion, solidify, or implicit fuse —
        // STEP transfer already yields BRep topology, so solidify would be
        // hidden repair and a fuse would change STEP product/part structure.
        if (command.args.size() != 1 ||
            (command.args[0].kind != Arg::Kind::Text && command.args[0].kind != Arg::Kind::Symbol)) {
            throw EvalError(op + " expects a file path");
        }
        TopoDS_Shape shape;
        {
            StageExecutionTimer import_timer(context, "import");
            STEPControl_Reader reader;
            if (reader.ReadFile(command.args[0].text_value.c_str()) != IFSelect_RetDone) {
                throw EvalError(op + " could not read STEP file (ReadFile did not return IFSelect_RetDone)");
            }
            if (reader.TransferRoots() <= 0) {
                throw EvalError(op + " transferred zero roots");
            }
            shape = reader.OneShape();
            if (shape.IsNull()) {
                throw EvalError(op + " produced a null shape");
            }
        }
        {
            // Validate only the transferred BRep. Do not sew, solidify, heal,
            // or fuse it: a locked STEP asset must preserve its native topology.
            StageExecutionTimer validation_timer(context, "validate");
            bool has_solid = (shape.ShapeType() == TopAbs_SOLID) ||
                             (shape.ShapeType() == TopAbs_COMPSOLID) ||
                             TopExp_Explorer(shape, TopAbs_SOLID).More();
            if (!has_solid) {
                throw EvalError(op + " produced no solid (shell-only payloads are rejected without healing)");
            }
            if (!BRepCheck_Analyzer(shape).IsValid()) {
                throw EvalError(op + " failed BRep validity check");
            }
        }
        return shape;
    }
    if (op == "import-indexed-mesh") {
        return SlotValue::manifold_value(make_indexed_manifold(command, context));
    }
    if (op == "solidify") {
        if (command.args.size() != 1) {
            throw EvalError(op + " expects exactly one shape reference");
        }
        StageExecutionTimer stage_timer(context, "solidify");
        if (command.args[0].kind == Arg::Kind::Ref) {
            auto input = slots.find(command.args[0].ref_value);
            if (input != slots.end() && input->second.kind == SlotValue::Kind::Manifold) {
                return SlotValue::manifold_value(input->second.manifold);
            }
        }
        return solidify_shape(get_ref_shape(0));
    }
    if (op == "extrude") {
        bool symmetric = false;
        for (const auto& keyword : command.keywords) {
            if (keyword.name != "symmetric" || keyword.value.kind != Arg::Kind::Boolean) {
                throw EvalError(op + " only supports boolean :symmetric");
            }
            symmetric = keyword.value.bool_value;
        }
        return extrude_shape(
            get_ref_shape(0), require_number_arg(command.args, 1, op), symmetric
        );
    }
    if (op == "revolve") {
        return revolve_shape(get_ref_shape(0), require_number_arg(command.args, 1, op));
    }
    if (op == "loft") {
        if (command.args.size() < 3) {
            throw EvalError(op + " expects distance and at least two profile references");
        }
        std::vector<TopoDS_Shape> profiles;
        for (std::size_t index = 1; index < command.args.size(); ++index) {
            profiles.push_back(get_ref_shape(index));
        }
        return loft_shapes(require_number_arg(command.args, 0, op), profiles);
    }
    if (op == "sweep") {
        bool frenet = false;
        for (const auto& keyword : command.keywords) {
            if (keyword.name == "frenet" && keyword.value.kind == Arg::Kind::Boolean) {
                frenet = keyword.value.bool_value;
            }
        }
        return sweep_shape(get_ref_shape(0), get_ref_shape(1), frenet);
    }
    if (op == "twist") {
        return twist_shape(get_ref_shape(2), require_number_arg(command.args, 0, op),
                           require_number_arg(command.args, 1, op));
    }
    if (op == "taper") {
        if (command.args.size() == 3) {
            double scale = require_number_arg(command.args, 1, op);
            return taper_shape(get_ref_shape(2), require_number_arg(command.args, 0, op), scale, scale);
        }
        if (command.args.size() == 4) {
            return taper_shape(get_ref_shape(3), require_number_arg(command.args, 0, op),
                               require_number_arg(command.args, 1, op),
                               require_number_arg(command.args, 2, op));
        }
        throw EvalError(op + " expects height, scale, profile or height, scale-x, scale-y, profile");
    }
    if (op == "draft") {
        double neutral_z = 0.0;
        for (const auto& keyword : command.keywords) {
            if ((keyword.name == "neutral-z" || keyword.name == "neutral_z") &&
                keyword.value.kind == Arg::Kind::Number) {
                neutral_z = keyword.value.number_value;
            }
        }
        return draft_shape(get_ref_shape(1), require_number_arg(command.args, 0, op), neutral_z);
    }
    if (op == "path") {
        return make_path_wire(require_point3_sequence(command.args, op));
    }
    if (op == "helix-path") {
        return make_helix_path_wire(require_number_arg(command.args, 0, op),
                                    require_number_arg(command.args, 1, op),
                                    require_number_arg(command.args, 2, op),
                                    require_bool_arg(command.args, 3, op),
                                    command.args.size() > 4
                                        ? std::optional<double>(require_number_arg(command.args, 4, op))
                                        : std::nullopt);
    }
    if (op == "bezier-path") {
        return make_bezier_path_wire(require_point3_sequence(command.args, op));
    }
    if (op == "plane") {
        return make_plane_frame(plane_args(command));
    }
    if (op == "location") {
        if (command.args.size() > 1) {
            throw EvalError(op + " expects zero or one frame reference");
        }
        gp_Trsf frame = command.args.empty()
            ? make_location_frame(nullptr)
            : make_location_frame(&get_ref_frame(0));
        std::array<double, 3> offset{0.0, 0.0, 0.0};
        std::array<double, 3> rotate{0.0, 0.0, 0.0};
        for (const Keyword& keyword : command.keywords) {
            if (keyword.kind != Keyword::Kind::Arg) {
                throw EvalError(op + " keywords expect arg values only");
            }
            if (keyword.name == "offset") {
                offset = require_point3_like_arg(keyword.value, "location :offset");
                continue;
            }
            if (keyword.name == "rotate") {
                rotate = require_point3_like_arg(keyword.value, "location :rotate");
                continue;
            }
            throw EvalError(op + " does not recognize `:" + keyword.name + "`");
        }
        apply_location_transform(frame, offset, rotate);
        return frame;
    }
    if (op == "path-frame") {
        PathFrameArgs args = path_frame_args(command);
        return make_path_frame(lookup_shape(slots, args.path_ref, op), args.at, args.up);
    }
    if (op == "place") {
        if (command.args.size() != 2) {
            throw EvalError(op + " expects frame and shape references");
        }
        return place_shape(get_ref_frame(0), get_ref_shape(1));
    }
    if (op == "clip-thread-envelope") {
        if (command.args.size() != 4) {
            throw EvalError(
                "clip-thread-envelope expects bottom radius, top radius, length, and shape");
        }
        return SlotValue::shape_value(clip_thread_envelope_shape(
            get_ref_shape(3),
            require_number_arg(command.args, 0, op),
            require_number_arg(command.args, 1, op),
            require_number_arg(command.args, 2, op)));
    }
    if (op == "bspline") {
        return make_bspline_shape(bspline_args(command));
    }
    if (op == "hull") {
        std::vector<Arg> refs = require_ref_list(command.args, op);
        std::vector<TopoDS_Shape> hull_inputs;
        hull_inputs.reserve(refs.size());
        for (const Arg& arg : refs) {
            hull_inputs.push_back(lookup_shape(slots, arg.ref_value, op));
        }
        return convex_hull_shapes(hull_inputs);
    }
    if (op == "union" || op == "difference" || op == "intersection" || op == "compound") {
        std::vector<Arg> refs = require_ref_list(command.args, op);
        if (op == "compound") {
            std::vector<TopoDS_Shape> shapes_to_compound;
            for (const Arg& arg : refs) {
                shapes_to_compound.push_back(lookup_shape(slots, arg.ref_value, op));
            }
            return compound_shapes(shapes_to_compound);
        }
        bool has_manifold = force_mesh_booleans;
        for (const Arg& arg : refs) {
            auto input = slots.find(arg.ref_value);
            if (input == slots.end()) throw EvalError(op + " references unknown slot");
            has_manifold = has_manifold || input->second.kind == SlotValue::Kind::Manifold;
            if (input->second.kind == SlotValue::Kind::Frame) {
                throw EvalError(op + " expects shape or Manifold operands");
            }
        }
        if (has_manifold) {
            if (op == "union") {
                std::vector<SlotValue> values;
                values.reserve(refs.size());
                for (const Arg& arg : refs) values.push_back(slots.at(arg.ref_value));
                return union_geometry_values(values, context, true);
            }
            StageExecutionTimer stage_timer(context, "boolean");
            auto input_manifold = [&](const Arg& arg) {
                const SlotValue& input = slots.at(arg.ref_value);
                return input.kind == SlotValue::Kind::Manifold
                    ? input.manifold
                    : manifold_from_occt_shape(input.shape, op, context);
            };
            manifold::Manifold result = input_manifold(refs.front());
            for (std::size_t index = 1; index < refs.size(); ++index) {
                manifold::Manifold next = input_manifold(refs[index]);
                if (op == "union") result = result + next;
                else if (op == "difference") result = result - next;
                else result = result ^ next;
                require_manifold_status(result, op);
            }
            {
                std::lock_guard<std::mutex> lock(context.mutex);
                ++context.mesh_boolean_count;
            }
            return SlotValue::manifold_value(result);
        }
        if (op == "union") {
            std::vector<TopoDS_Shape> shapes_to_fuse;
            shapes_to_fuse.reserve(refs.size());
            for (const Arg& arg : refs) {
                shapes_to_fuse.push_back(lookup_shape(slots, arg.ref_value, op));
            }
            return fuse_shapes(shapes_to_fuse, context);
        }
        if (op == "difference") {
            const TopoDS_Shape& head = lookup_shape(slots, refs.front().ref_value, op);
            std::vector<TopoDS_Shape> tools;
            tools.reserve(refs.size() - 1);
            for (std::size_t index = 1; index < refs.size(); ++index) {
                tools.push_back(lookup_shape(slots, refs[index].ref_value, op));
            }
            return cut_shapes(head, tools, context);
        }
        TopoDS_Shape result = lookup_shape(slots, refs.front().ref_value, op);
        for (std::size_t index = 1; index < refs.size(); ++index) {
            const TopoDS_Shape& next = lookup_shape(slots, refs[index].ref_value, op);
            result = common_shapes(result, next, context);
        }
        return result;
    }
    if (op == "translate") {
        const double x = require_number_arg(command.args, 0, op);
        const double y = require_number_arg(command.args, 1, op);
        const double z = require_number_arg(command.args, 2, op);
        const SlotValue& input = get_ref_slot(3);
        if (input.kind == SlotValue::Kind::Manifold) {
            manifold::Manifold result = input.manifold.Translate({x, y, z});
            require_manifold_status(result, op);
            return SlotValue::manifold_value(result);
        }
        return translate_shape(input.shape, x, y, z);
    }
    if (op == "rotate") {
        const double x = require_number_arg(command.args, 0, op);
        const double y = require_number_arg(command.args, 1, op);
        const double z = require_number_arg(command.args, 2, op);
        const SlotValue& input = get_ref_slot(3);
        if (input.kind == SlotValue::Kind::Manifold) {
            manifold::Manifold result = input.manifold.Rotate(x, y, z);
            require_manifold_status(result, op);
            return SlotValue::manifold_value(result);
        }
        return rotate_shape(input.shape, x, y, z);
    }
    if (op == "scale") {
        double x = 1.0;
        double y = 1.0;
        double z = 1.0;
        std::size_t input_index = 0;
        if (command.args.size() == 2) {
            x = require_number_arg(command.args, 0, op);
            y = x;
            z = x;
            input_index = 1;
        } else if (command.args.size() == 3) {
            x = require_number_arg(command.args, 0, op);
            y = require_number_arg(command.args, 1, op);
            input_index = 2;
        } else if (command.args.size() == 4) {
            x = require_number_arg(command.args, 0, op);
            y = require_number_arg(command.args, 1, op);
            z = require_number_arg(command.args, 2, op);
            input_index = 3;
        } else {
            throw EvalError(op + " expects one to three factors and a shape");
        }
        const SlotValue& input = get_ref_slot(input_index);
        if (input.kind == SlotValue::Kind::Manifold) {
            manifold::Manifold result = input.manifold.Scale({x, y, z});
            require_manifold_status(result, op);
            return SlotValue::manifold_value(result);
        }
        return scale_shape(input.shape, x, y, z);
    }
    if (op == "mirror") {
        if (command.args.size() != 3 || command.args[0].kind == Arg::Kind::Number) {
            throw EvalError(op + " expects axis, offset, shape");
        }
        const Arg& axis = command.args[0];
        if (axis.kind != Arg::Kind::Text && axis.kind != Arg::Kind::Symbol) {
            throw EvalError(op + " expects text/symbol axis");
        }
        return mirror_shape(get_ref_shape(2), axis.text_value, require_number_arg(command.args, 1, op));
    }
    if (op == "linear-array") {
        return linear_array_shape(get_ref_shape(4), require_count_arg(command.args, 0, op),
                                  require_number_arg(command.args, 1, op),
                                  require_number_arg(command.args, 2, op),
                                  require_number_arg(command.args, 3, op));
    }
    if (op == "radial-array") {
        return radial_array_shape(get_ref_shape(3), require_count_arg(command.args, 0, op),
                                  require_number_arg(command.args, 1, op),
                                  require_number_arg(command.args, 2, op));
    }
    if (op == "grid-array") {
        return grid_array_shape(get_ref_shape(4), require_count_arg(command.args, 0, op),
                                require_count_arg(command.args, 1, op),
                                require_number_arg(command.args, 2, op),
                                require_number_arg(command.args, 3, op));
    }
    if (op == "arc-array") {
        return arc_array_shape(get_ref_shape(4), require_count_arg(command.args, 0, op),
                               require_number_arg(command.args, 1, op),
                               require_number_arg(command.args, 2, op),
                               require_number_arg(command.args, 3, op));
    }
    if (op == "offset") {
        return offset_shape(get_ref_shape(1), require_number_arg(command.args, 0, op));
    }
    if (op == "clip-box") {
        ClipBoxArgs args = clip_box_args(command);
        return clip_box_shape(lookup_shape(slots, args.shape_ref, op), args.x, args.y, args.z);
    }
    if (op == "clip-plane") {
        ClipPlaneArgs args = clip_plane_args(command);
        return clip_plane_shape(
            lookup_shape(slots, args.shape_ref, op),
            args.origin,
            args.normal,
            args.keep_positive);
    }
    if (op == "fillet") {
        std::optional<double> to_radius = optional_number_keyword(command, "to-radius");
        if (!to_radius.has_value()) {
            to_radius = optional_number_keyword(command, "to_radius");
        }
        return fillet_shape(
            part_id,
            get_ref_shape(1),
            require_number_arg(command.args, 0, op),
            to_radius,
            exact_edge_selector(command, op)
        );
    }
    if (op == "chamfer") {
        return chamfer_shape(
            part_id,
            get_ref_shape(1),
            require_number_arg(command.args, 0, op),
            exact_edge_selector(command, op)
        );
    }
    if (op == "shell") {
        return shell_shape(
            part_id,
            get_ref_shape(1),
            require_number_arg(command.args, 0, op),
            exact_face_selector(command, op)
        );
    }

    throw EvalError("unsupported direct OCCT op `" + op + "`");
}

void collect_arg_refs(const Arg& arg, std::vector<std::uint64_t>& refs) {
    if (arg.kind == Arg::Kind::Ref) {
        refs.push_back(arg.ref_value);
        return;
    }
    if (arg.kind == Arg::Kind::List) {
        for (const Arg& item : arg.list_value) collect_arg_refs(item, refs);
    }
}

std::vector<std::uint64_t> command_refs(const Command& command) {
    std::vector<std::uint64_t> refs;
    for (const Arg& arg : command.args) collect_arg_refs(arg, refs);
    for (const Keyword& keyword : command.keywords) collect_arg_refs(keyword.value, refs);
    std::sort(refs.begin(), refs.end());
    refs.erase(std::unique(refs.begin(), refs.end()), refs.end());
    return refs;
}

// Only these operations are admitted to overlap.  They either construct a
// fresh value or make a documented copy; all unknown/effectful OCCT calls run
// behind the exclusive barrier.  This is deliberately conservative.
bool command_has_proven_immutable_inputs(const Command& command) {
    static const std::set<std::string> kSafe = {
        "box", "sphere", "cylinder", "cone", "torus", "wedge", "rectangle",
        "ellipse", "slot-overall", "slot-arc", "circle", "rounded-rect",
        "rounded-polygon", "polygon", "path", "helix-path", "bezier-path",
        "plane", "location", "bspline", "translate", "rotate", "scale", "mirror",
        "linear-array", "radial-array", "grid-array", "arc-array"
    };
    return kSafe.find(command.op) != kSafe.end();
}

struct DagNode {
    std::size_t part_index = 0;
    std::size_t command_index = 0;
    std::vector<std::size_t> dependents;
    std::size_t unmet_dependencies = 0;
    bool exclusive = true;
};

struct CompletedDagNode {
    std::size_t node_index = 0;
    SlotValue value;
    std::vector<std::pair<std::string, SlotValue>> partial_boolean_outputs;
};

std::string partial_group_id(
    std::size_t part_index, std::uint64_t parent_output, std::uint32_t ordinal
) {
    return std::to_string(part_index) + ":" + std::to_string(parent_output) + ":" +
        std::to_string(ordinal);
}

void record_partial_group_recompute(
    ExecutionContext& context,
    const std::string& part_id,
    std::uint64_t parent_output,
    const std::string& key
) {
    std::lock_guard<std::mutex> lock(context.mutex);
    const auto evidence = std::find_if(
        context.partial_boolean_group_evidence.begin(),
        context.partial_boolean_group_evidence.end(),
        [&](const PartialBooleanGroupEvidence& item) {
            return item.part_id == part_id && item.parent_output == parent_output &&
                item.key == key;
        });
    if (evidence != context.partial_boolean_group_evidence.end()) {
        ++evidence->recompute_count;
    }
}

std::uint32_t configured_worker_budget() {
    const char* raw = std::getenv("ECKY_DIRECT_OCCT_WORKERS");
    if (!raw || !*raw) return std::max(1u, std::thread::hardware_concurrency());
    try {
        unsigned long parsed = std::stoul(raw);
        return static_cast<std::uint32_t>(std::clamp<unsigned long>(parsed, 1, 64));
    } catch (...) {
        throw ParseError("ECKY_DIRECT_OCCT_WORKERS must be an integer from 1 to 64");
    }
}

ParallelPolicy configured_parallel_policy() {
    const char* raw = std::getenv("ECKY_DIRECT_OCCT_PARALLEL_POLICY");
    if (!raw || !*raw || std::string(raw) == "adaptive") return ParallelPolicy::Adaptive;
    if (std::string(raw) == "outer-only") return ParallelPolicy::OuterOnly;
    throw ParseError(
        "ECKY_DIRECT_OCCT_PARALLEL_POLICY must be `outer-only` or `adaptive`");
}

void configure_mesh_parallelism_budget(std::size_t final_part_count, ExecutionContext& context) {
    if (final_part_count == 0) throw EvalError("part mesh scheduler needs at least one final part");
    if (context.worker_budget == 1) {
        context.mesh_outer_worker_budget = 1;
        context.mesh_pool_budget = 1;
        context.mesh_launcher_budget = 1;
        context.peak_total_allocated_cpu_units = std::max(
            context.peak_total_allocated_cpu_units, 1u);
        return;
    }
    // OSD's default pool counts its caller. M outer mesh callers plus P-1
    // pool workers stay within N total workers. K limits each mesher launcher
    // so concurrently tessellated final parts split that single pool fairly.
    context.mesh_outer_worker_budget = static_cast<std::uint32_t>(std::min<std::size_t>(
        final_part_count, std::max(1u, context.worker_budget / 4)));
    context.mesh_pool_budget =
        context.worker_budget - context.mesh_outer_worker_budget + 1;
    context.mesh_launcher_budget = 1 +
        (context.mesh_pool_budget - 1) / context.mesh_outer_worker_budget;
    context.peak_total_allocated_cpu_units = std::max(
        context.peak_total_allocated_cpu_units, context.worker_budget);
    OSD_Parallel::SetUseOcctThreads(Standard_True);
    if (!OSD_Parallel::ToUseOcctThreads()) {
        throw EvalError("Direct OCCT runtime cannot enable its shared OSD thread pool");
    }
    const Handle(OSD_ThreadPool)& pool =
        OSD_ThreadPool::DefaultPool(static_cast<int>(context.mesh_pool_budget));
    pool->SetNbDefaultThreadsToLaunch(static_cast<int>(context.mesh_launcher_budget));
}

std::optional<fs::path> selective_cache_root() {
    const char* raw = std::getenv("ECKY_DIRECT_OCCT_CACHE_DIR");
    if (!raw || !*raw) return std::nullopt;
    fs::path root(raw);
    fs::create_directories(root);
    return root;
}

constexpr char kSelectiveCacheSchema[] = "direct-occt-selective-geometry-v4";
constexpr char kDirectOcctRunnerAbi[] = "direct-occt-runner-abi-4";
std::string direct_occt_runner_abi_digest() {
    return "sha256:" + ecky::sha256_hex(kDirectOcctRunnerAbi);
}

std::string canonical_number(double value) {
    try {
        return ecky::canonical_f64(value);
    } catch (const std::invalid_argument&) {
        throw EvalError("runner cache identity rejects non-finite resolved numbers");
    }
}

std::string canonical_semantic_arg(
    const Arg& arg,
    const std::map<std::uint64_t, std::string>& dependencies
) {
    std::ostringstream out;
    out << "kind=" << static_cast<int>(arg.kind) << ':';
    switch (arg.kind) {
        case Arg::Kind::Number:
            out << canonical_number(arg.number_value);
            break;
        case Arg::Kind::Boolean: out << (arg.bool_value ? '1' : '0'); break;
        case Arg::Kind::Text:
        case Arg::Kind::Symbol: out << arg.text_value; break;
        case Arg::Kind::Point2:
            out << canonical_number(arg.point2_value[0]) << ',' << canonical_number(arg.point2_value[1]);
            break;
        case Arg::Kind::Point3:
            out << canonical_number(arg.point3_value[0]) << ',' << canonical_number(arg.point3_value[1])
                << ',' << canonical_number(arg.point3_value[2]);
            break;
        case Arg::Kind::List:
            out << '[';
            for (const Arg& item : arg.list_value) out << canonical_semantic_arg(item, dependencies);
            out << ']';
            break;
        case Arg::Kind::Ref: {
            auto found = dependencies.find(arg.ref_value);
            if (found == dependencies.end()) throw EvalError("cache identity references unknown slot");
            out << "dependency";
            break;
        }
        case Arg::Kind::Param: throw EvalError("runner cache identity requires resolved args");
    }
    return out.str();
}

void append_dependency_identities(
    const Arg& arg,
    const std::map<std::uint64_t, std::string>& dependencies,
    std::vector<std::string>& result
) {
    if (arg.kind == Arg::Kind::Ref) {
        auto found = dependencies.find(arg.ref_value);
        if (found == dependencies.end()) throw EvalError("cache identity references unknown slot");
        result.push_back(found->second);
    } else if (arg.kind == Arg::Kind::List) {
        for (const Arg& item : arg.list_value) append_dependency_identities(item, dependencies, result);
    }
}

std::string canonical_selector_payload(const SelectorPayload& payload) {
    std::ostringstream out;
    out << "selector:" << static_cast<int>(payload.type) << ':' << static_cast<int>(payload.kind) << ':';
    for (const std::string& id : payload.target_ids) out << id.size() << ':' << id << ';';
    for (const SelectorClause& clause : payload.clauses) {
        out << static_cast<int>(clause.type) << ':'
            << (clause.axis ? std::to_string(static_cast<int>(*clause.axis)) : "-") << ':'
            << (clause.bound ? std::to_string(static_cast<int>(*clause.bound)) : "-") << ':'
            << (clause.rank ? std::to_string(static_cast<int>(*clause.rank)) : "-") << ';';
    }
    return out.str();
}

std::string read_binary_file(const fs::path& path) {
    std::ifstream input(path, std::ios::binary);
    if (!input) throw IoError("cannot read execution identity input `" + path.string() + "`");
    std::ostringstream bytes;
    bytes << input.rdbuf();
    if (!input.good() && !input.eof()) throw IoError("cannot finish execution identity input `" + path.string() + "`");
    return bytes.str();
}

ecky::ExecutionIdentityInput execution_identity_base(const std::string& op, const ExecutionContext& context) {
    if (context.runner_binary_digest.empty()) {
        throw EvalError("runner binary digest unavailable for selective cache identity");
    }
    ecky::ExecutionIdentityInput input;
    input.cache_schema = kSelectiveCacheSchema;
    input.runner_abi = direct_occt_runner_abi_digest();
    input.runner_binary_digest = context.runner_binary_digest;
    input.occt_runtime = std::string("occt=") + OCC_VERSION_COMPLETE;
    input.tolerance_policy = "boolean-fuzzy=1e-5;stl-weld=1e-6";
    input.tessellation_policy = "linear=0.04;angular=clamp(linear*5+0.005,0.005,0.1);part-mesh-v2";
    input.op = op;
    return input;
}

std::string command_cache_key(
    const Command& command,
    const std::map<std::uint64_t, std::string>& dependencies,
    const ExecutionContext& context
) {
    ecky::ExecutionIdentityInput input = execution_identity_base(command.op, context);
    for (std::size_t index = 0; index < command.args.size(); ++index) {
        const Arg& arg = command.args[index];
        const bool imported_payload = (command.op == "import-stl" || command.op == "import-step") && index == 0 &&
            (arg.kind == Arg::Kind::Text || arg.kind == Arg::Kind::Symbol);
        input.resolved_args.push_back(imported_payload ? "import-payload" : canonical_semantic_arg(arg, dependencies));
        if (imported_payload) input.import_payloads.push_back(read_binary_file(arg.text_value));
        append_dependency_identities(arg, dependencies, input.ordered_dependency_identities);
    }
    for (const Keyword& keyword : command.keywords) {
        input.normalized_keywords.push_back(
            "keyword:" + keyword.name + ':' + std::to_string(static_cast<int>(keyword.kind)) + ':' +
            canonical_semantic_arg(keyword.value, dependencies));
        append_dependency_identities(keyword.value, dependencies, input.ordered_dependency_identities);
        if (keyword.selector_payload.has_value()) {
            input.selectors.push_back(canonical_selector_payload(*keyword.selector_payload));
        }
    }
    return ecky::execution_identity(input);
}

std::string partial_boolean_group_cache_key(
    const Command& command,
    const PartialBooleanGroupPlan& group,
    const std::map<std::uint64_t, std::string>& dependencies,
    const ExecutionContext& context
) {
    ecky::ExecutionIdentityInput input = execution_identity_base("partial-union-v2", context);
    input.resolved_args.push_back("key:" + group.key);
    input.resolved_args.push_back(
        std::string("representation:") + geometry_representation_name(group.representation));
    input.resolved_args.push_back("operation:" + group.operation);
    input.resolved_args.push_back("version:" + std::to_string(group.version));
    for (std::uint32_t index : group.input_indices) {
        if (index >= command.args.size()) throw EvalError("partial union input index out of range");
        const Arg& arg = command.args[index];
        input.resolved_args.push_back(canonical_semantic_arg(arg, dependencies));
        append_dependency_identities(arg, dependencies, input.ordered_dependency_identities);
    }
    return ecky::execution_identity(input);
}

std::string part_cache_key(const Part& part, const ExecutionContext& context) {
    std::map<std::uint64_t, std::string> signatures;
    for (const Command& command : part.commands) {
        signatures.emplace(command.output, command_cache_key(command, signatures, context));
    }
    auto root = signatures.find(part.root);
    if (root == signatures.end()) throw EvalError("cache identity missing part root");
    ecky::ExecutionIdentityInput input = execution_identity_base("part-root", context);
    input.resolved_args = {
        std::string("representation:") + geometry_representation_name(part.representation),
        "export:step+part-mesh-stl",
        "root:" + root->second,
    };
    input.ordered_dependency_identities.push_back(root->second);
    return ecky::execution_identity(input);
}

std::string authored_part_cache_key(
    const Part& part,
    const std::vector<std::string>& command_cache_keys,
    const ExecutionContext& context
) {
    ecky::ExecutionIdentityInput input = execution_identity_base("part-authored-slots", context);
    input.resolved_args.push_back(part_cache_key(part, context));
    for (const Part::AuthoredBinding& binding : part.authored_bindings) {
        const auto command = std::find_if(
            part.commands.begin(), part.commands.end(), [&](const Command& candidate) {
                return candidate.output == binding.slot;
            });
        if (command == part.commands.end()) {
            throw EvalError("cache identity missing authored binding producer");
        }
        const std::size_t command_index = static_cast<std::size_t>(
            std::distance(part.commands.begin(), command));
        input.ordered_dependency_identities.push_back(command_cache_keys.at(command_index));
    }
    return ecky::execution_identity(input);
}

bool command_cache_admitted(const Command& command) {
    static const std::set<std::string> kExpensive = {
        "union", "difference", "intersection", "fillet", "chamfer", "shell", "loft",
        "sweep", "solidify", "import-stl", "import-step", "hull"
    };
    return kExpensive.find(command.op) != kExpensive.end();
}

std::string file_digest(const fs::path& path) {
    return "sha256:" + ecky::sha256_hex(read_binary_file(path));
}

std::vector<std::string> part_command_cache_keys(
    const Part& part, const ExecutionContext& context
) {
    std::map<std::uint64_t, std::string> dependencies;
    std::vector<std::string> keys;
    keys.reserve(part.commands.size());
    for (const Command& command : part.commands) {
        const std::string key = command_cache_key(command, dependencies, context);
        keys.push_back(key);
        dependencies.emplace(command.output, key);
    }
    return keys;
}

std::string topology_fragment_cache_key(
    const Part& part,
    const ExecutionContext& context,
    double boundary_linear_deflection,
    double boundary_angular_deflection,
    const std::optional<std::string>& source_geometry_digest_override
) {
    ecky::ExecutionIdentityInput input = execution_identity_base(
        "part-topology-json-v1", context);
    input.resolved_args = {
        "part-id:" + part.part_id,
        "label:" + part.label,
        "representation:" + std::string(geometry_representation_name(part.representation)),
        "linear-deflection:" + ecky::canonical_f64(boundary_linear_deflection),
        "angular-deflection:" + ecky::canonical_f64(boundary_angular_deflection),
        "source-geometry-digest:" + source_geometry_digest_override.value_or("computed"),
    };
    input.ordered_dependency_identities.push_back(part_cache_key(part, context));
    if (!part.authored_bindings.empty()) {
        const std::vector<std::string> command_keys = part_command_cache_keys(part, context);
        input.ordered_dependency_identities.push_back(
            authored_part_cache_key(part, command_keys, context));
        for (const Part::AuthoredBinding& binding : part.authored_bindings) {
            input.resolved_args.push_back("authored-binding:" + binding.name);
        }
    }
    return ecky::execution_identity(input);
}

std::optional<CachedTopologyFragment> read_cached_topology_fragment(
    const fs::path& root,
    const std::string& key,
    ExecutionContext& context
) {
    {
        std::lock_guard<std::mutex> lock(context.mutex);
        ++context.cache_read_count;
    }
    const fs::path cache_dir = root / "topology-parts";
    const fs::path artifact = cache_dir / (key + ".json");
    const fs::path metadata = cache_dir / (key + ".meta");
    try {
        std::ifstream meta(metadata);
        std::string schema;
        std::string stored_key;
        std::string stored_representation;
        std::string stored_digest;
        std::string stored_size;
        std::string source_geometry_digest;
        if (!(meta >> schema >> stored_key >> stored_representation >> stored_digest >>
              stored_size >> source_geometry_digest) ||
            schema != kSelectiveCacheSchema || stored_key != key ||
            stored_representation != "topologyJson" ||
            !fs::is_regular_file(artifact) || file_digest(artifact) != stored_digest ||
            std::to_string(fs::file_size(artifact)) != stored_size) {
            fs::remove(artifact);
            fs::remove(metadata);
            std::lock_guard<std::mutex> lock(context.mutex);
            ++context.cache_rejection_count;
            return std::nullopt;
        }
        const std::string json = read_binary_file(artifact);
        const std::string digest_field =
            "\"sourceGeometryDigest\":" + quote_json_string(source_geometry_digest);
        if (json.size() < 2 || json.front() != '{' || json.back() != '}' ||
            json.find(digest_field) == std::string::npos) {
            fs::remove(artifact);
            fs::remove(metadata);
            std::lock_guard<std::mutex> lock(context.mutex);
            ++context.cache_rejection_count;
            return std::nullopt;
        }
        const auto now = fs::file_time_type::clock::now();
        std::error_code touch_error;
        fs::last_write_time(artifact, now, touch_error);
        fs::last_write_time(metadata, now, touch_error);
        return CachedTopologyFragment{json, source_geometry_digest};
    } catch (...) {
        std::error_code ignored;
        fs::remove(artifact, ignored);
        fs::remove(metadata, ignored);
        std::lock_guard<std::mutex> lock(context.mutex);
        ++context.cache_rejection_count;
        return std::nullopt;
    }
}

void write_cached_topology_fragment(
    const fs::path& root,
    const std::string& key,
    const CachedTopologyFragment& fragment,
    ExecutionContext& context
) {
    const fs::path cache_dir = root / "topology-parts";
    fs::create_directories(cache_dir);
    const fs::path artifact = cache_dir / (key + ".json");
    const fs::path metadata = cache_dir / (key + ".meta");
    {
        std::ofstream output(artifact, std::ios::binary);
        output.write(fragment.json.data(), static_cast<std::streamsize>(fragment.json.size()));
        if (!output.good()) throw IoError("failed to stage topology cache artifact");
    }
    const std::string digest = file_digest(artifact);
    {
        std::ofstream meta(metadata);
        meta << kSelectiveCacheSchema << ' ' << key << " topologyJson " << digest << ' '
             << fs::file_size(artifact) << ' ' << fragment.source_geometry_digest << '\n';
        if (!meta.good()) throw IoError("failed to stage topology cache metadata");
    }
    std::lock_guard<std::mutex> lock(context.mutex);
    ++context.cache_write_count;
}

std::optional<TopoDS_Shape> read_cached_shape(
    const fs::path& root, const std::string& kind, const std::string& key, ExecutionContext& context
) {
    {
        std::lock_guard<std::mutex> lock(context.mutex);
        ++context.cache_read_count;
    }
    const fs::path cache_dir = root / kind;
    const fs::path artifact = cache_dir / (key + ".brepbin");
    const fs::path metadata = cache_dir / (key + ".meta");
    try {
        std::ifstream meta(metadata);
        std::string schema, stored_key, stored_representation, stored_digest, stored_size;
        if (!(meta >> schema >> stored_key >> stored_representation >> stored_digest >> stored_size) ||
            schema != kSelectiveCacheSchema || stored_key != key ||
            stored_representation != "analyticBrep" ||
            !fs::is_regular_file(artifact) || file_digest(artifact) != stored_digest ||
            std::to_string(fs::file_size(artifact)) != stored_size) {
            fs::remove(artifact);
            fs::remove(metadata);
            {
                std::lock_guard<std::mutex> lock(context.mutex);
                ++context.cache_rejection_count;
            }
            return std::nullopt;
        }
        TopoDS_Shape shape;
        // The writer admits only BRepCheck-valid shapes. On read, metadata size
        // plus SHA-256 protects those bytes and BinTools proves decodability.
        // Re-running deep BRepCheck here turns an unchanged multipart render
        // into a full-model traversal and defeats selective recomputation.
        if (!BinTools::Read(shape, artifact.string().c_str()) || shape.IsNull()) {
            fs::remove(artifact);
            fs::remove(metadata);
            {
                std::lock_guard<std::mutex> lock(context.mutex);
                ++context.cache_rejection_count;
            }
            return std::nullopt;
        }
        const auto now = fs::file_time_type::clock::now();
        std::error_code touch_error;
        fs::last_write_time(artifact, now, touch_error);
        fs::last_write_time(metadata, now, touch_error);
        return shape;
    } catch (...) {
        std::error_code ignored;
        fs::remove(artifact, ignored);
        fs::remove(metadata, ignored);
        {
            std::lock_guard<std::mutex> lock(context.mutex);
            ++context.cache_rejection_count;
        }
        return std::nullopt;
    }
}

std::uintmax_t configured_cache_budget_bytes() {
    const char* raw = std::getenv("ECKY_DIRECT_OCCT_CACHE_BYTES");
    if (!raw || !*raw) return 512ULL * 1024ULL * 1024ULL;
    try { return std::stoull(raw); }
    catch (...) { throw ParseError("ECKY_DIRECT_OCCT_CACHE_BYTES must be an unsigned integer"); }
}

void evict_cache_to_budget(const fs::path& root) {
    struct Entry { fs::path artifact; fs::path metadata; fs::file_time_type used; std::uintmax_t bytes; };
    std::vector<Entry> entries;
    std::uintmax_t total = 0;
    for (const char* kind : {
             "parts", "commands", "authored-parts", "partial-booleans", "part-meshes",
             "topology-parts"}) {
        const fs::path dir = root / kind;
        if (!fs::is_directory(dir)) continue;
        for (const auto& item : fs::directory_iterator(dir)) {
            const std::string extension = item.path().extension().string();
            if (extension != ".brepbin" && extension != ".meshbin" &&
                extension != ".partmesh" && extension != ".json") {
                continue;
            }
            const fs::path metadata = item.path().parent_path() /
                (item.path().stem().string() + ".meta");
            std::error_code error;
            const std::uintmax_t bytes = fs::file_size(item.path(), error) +
                (fs::exists(metadata) ? fs::file_size(metadata, error) : 0);
            entries.push_back({item.path(), metadata, fs::last_write_time(item.path(), error), bytes});
            total += bytes;
        }
    }
    std::sort(entries.begin(), entries.end(), [](const Entry& left, const Entry& right) {
        return left.used < right.used;
    });
    const std::uintmax_t budget = configured_cache_budget_bytes();
    for (const Entry& entry : entries) {
        if (total <= budget) break;
        std::error_code ignored;
        fs::remove(entry.artifact, ignored);
        fs::remove(entry.metadata, ignored);
        total -= entry.bytes;
    }
}

void write_cached_shape(
    const fs::path& root, const std::string& kind, const std::string& key, const TopoDS_Shape& shape,
    ExecutionContext& context
) {
    if (shape.IsNull() || !BRepCheck_Analyzer(shape).IsValid()) return;
    const fs::path cache_dir = root / kind;
    fs::create_directories(cache_dir);
    const fs::path artifact = cache_dir / (key + ".brepbin");
    const fs::path metadata = cache_dir / (key + ".meta");
    const std::string suffix = ".tmp-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count());
    const fs::path artifact_tmp = artifact.string() + suffix;
    const fs::path metadata_tmp = metadata.string() + suffix;
    if (!BinTools::Write(shape, artifact_tmp.string().c_str())) return;
    const std::string digest = file_digest(artifact_tmp);
    {
        std::ofstream meta(metadata_tmp);
        if (!meta) { fs::remove(artifact_tmp); return; }
        meta << kSelectiveCacheSchema << ' ' << key << " analyticBrep " << digest << ' '
             << fs::file_size(artifact_tmp) << '\n';
    }
    std::error_code error;
    fs::rename(artifact_tmp, artifact, error);
    if (error) { fs::remove(artifact_tmp); fs::remove(metadata_tmp); return; }
    fs::rename(metadata_tmp, metadata, error);
    if (error) { fs::remove(metadata_tmp); fs::remove(artifact); return; }
    std::lock_guard<std::mutex> lock(context.mutex);
    ++context.cache_write_count;
}

std::string serialize_cached_manifold(const manifold::Manifold& value) {
    require_manifold_status(value, "mesh cache write");
    const manifold::MeshGL64 mesh = value.GetMeshGL64();
    if (mesh.numProp < 3 || mesh.vertProperties.empty() || mesh.triVerts.empty() ||
        mesh.vertProperties.size() % static_cast<std::size_t>(mesh.numProp) != 0 ||
        mesh.triVerts.size() % 3 != 0) {
        throw EvalError("mesh cache write received invalid indexed geometry");
    }
    std::string bytes("ECKYMESH", 8);
    const auto append_u64 = [&](std::uint64_t value_to_append) {
        for (int offset = 0; offset < 8; ++offset) {
            bytes.push_back(static_cast<char>((value_to_append >> (offset * 8)) & 0xff));
        }
    };
    append_u64(static_cast<std::uint64_t>(mesh.numProp));
    append_u64(static_cast<std::uint64_t>(mesh.vertProperties.size()));
    append_u64(static_cast<std::uint64_t>(mesh.triVerts.size()));
    for (double value_to_append : mesh.vertProperties) {
        std::uint64_t bits = 0;
        static_assert(sizeof(bits) == sizeof(value_to_append));
        std::memcpy(&bits, &value_to_append, sizeof(bits));
        append_u64(bits);
    }
    for (const auto index : mesh.triVerts) {
        append_u64(static_cast<std::uint64_t>(index));
    }
    return bytes;
}

manifold::Manifold deserialize_cached_manifold(const std::string& bytes) {
    if (bytes.size() < 32 || bytes.compare(0, 8, "ECKYMESH") != 0) {
        throw EvalError("mesh cache artifact has invalid header");
    }
    std::size_t cursor = 8;
    const auto read_u64 = [&]() {
        if (cursor + 8 > bytes.size()) throw EvalError("mesh cache artifact is truncated");
        std::uint64_t value = 0;
        for (int offset = 0; offset < 8; ++offset) {
            value |= static_cast<std::uint64_t>(
                static_cast<unsigned char>(bytes[cursor + offset])) << (offset * 8);
        }
        cursor += 8;
        return value;
    };
    const std::uint64_t num_prop = read_u64();
    const std::uint64_t vertex_property_count = read_u64();
    const std::uint64_t triangle_index_count = read_u64();
    if (num_prop < 3 || num_prop > 64 || vertex_property_count == 0 ||
        vertex_property_count % num_prop != 0 || triangle_index_count == 0 ||
        triangle_index_count % 3 != 0 ||
        vertex_property_count > (bytes.size() - cursor) / 8 ||
        triangle_index_count > (bytes.size() - cursor) / 8 - vertex_property_count ||
        cursor + (vertex_property_count + triangle_index_count) * 8 != bytes.size()) {
        throw EvalError("mesh cache artifact has invalid dimensions");
    }
    manifold::MeshGL64 mesh;
    mesh.numProp = static_cast<int>(num_prop);
    mesh.vertProperties.reserve(static_cast<std::size_t>(vertex_property_count));
    for (std::uint64_t index = 0; index < vertex_property_count; ++index) {
        const std::uint64_t bits = read_u64();
        double value = 0.0;
        std::memcpy(&value, &bits, sizeof(value));
        if (!std::isfinite(value)) throw EvalError("mesh cache vertex is non-finite");
        mesh.vertProperties.push_back(value);
    }
    using MeshIndex = typename decltype(mesh.triVerts)::value_type;
    mesh.triVerts.reserve(static_cast<std::size_t>(triangle_index_count));
    for (std::uint64_t index = 0; index < triangle_index_count; ++index) {
        const std::uint64_t value = read_u64();
        if (value >= mesh.NumVert() || value > std::numeric_limits<MeshIndex>::max()) {
            throw EvalError("mesh cache triangle index is out of bounds");
        }
        mesh.triVerts.push_back(static_cast<MeshIndex>(value));
    }
    manifold::Manifold result(mesh);
    require_manifold_status(result, "mesh cache read");
    return result;
}

std::optional<manifold::Manifold> read_cached_manifold(
    const fs::path& root,
    const std::string& kind,
    const std::string& key,
    ExecutionContext& context
) {
    {
        std::lock_guard<std::mutex> lock(context.mutex);
        ++context.cache_read_count;
    }
    const fs::path cache_dir = root / kind;
    const fs::path artifact = cache_dir / (key + ".meshbin");
    const fs::path metadata = cache_dir / (key + ".meta");
    try {
        std::ifstream meta(metadata);
        std::string schema, stored_key, stored_representation, stored_digest, stored_size;
        if (!(meta >> schema >> stored_key >> stored_representation >> stored_digest >> stored_size) ||
            schema != kSelectiveCacheSchema || stored_key != key ||
            stored_representation != "meshDomain" || !fs::is_regular_file(artifact) ||
            file_digest(artifact) != stored_digest ||
            std::to_string(fs::file_size(artifact)) != stored_size) {
            throw EvalError("mesh cache metadata mismatch");
        }
        manifold::Manifold result = deserialize_cached_manifold(read_binary_file(artifact));
        const auto now = fs::file_time_type::clock::now();
        std::error_code touch_error;
        fs::last_write_time(artifact, now, touch_error);
        fs::last_write_time(metadata, now, touch_error);
        return result;
    } catch (...) {
        std::error_code ignored;
        fs::remove(artifact, ignored);
        fs::remove(metadata, ignored);
        std::lock_guard<std::mutex> lock(context.mutex);
        ++context.cache_rejection_count;
        return std::nullopt;
    }
}

void write_cached_manifold(
    const fs::path& root,
    const std::string& kind,
    const std::string& key,
    const manifold::Manifold& value,
    ExecutionContext& context
) {
    const std::string bytes = serialize_cached_manifold(value);
    const fs::path cache_dir = root / kind;
    fs::create_directories(cache_dir);
    const fs::path artifact = cache_dir / (key + ".meshbin");
    const fs::path metadata = cache_dir / (key + ".meta");
    const std::string suffix = ".tmp-" + std::to_string(
        std::chrono::steady_clock::now().time_since_epoch().count());
    const fs::path artifact_tmp = artifact.string() + suffix;
    const fs::path metadata_tmp = metadata.string() + suffix;
    {
        std::ofstream output(artifact_tmp, std::ios::binary);
        if (!output) return;
        output.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
        if (!output.good()) {
            fs::remove(artifact_tmp);
            return;
        }
    }
    const std::string digest = file_digest(artifact_tmp);
    {
        std::ofstream meta(metadata_tmp);
        if (!meta) {
            fs::remove(artifact_tmp);
            return;
        }
        meta << kSelectiveCacheSchema << ' ' << key << " meshDomain " << digest << ' '
             << fs::file_size(artifact_tmp) << '\n';
    }
    std::error_code error;
    fs::rename(artifact_tmp, artifact, error);
    if (error) {
        fs::remove(artifact_tmp);
        fs::remove(metadata_tmp);
        return;
    }
    fs::rename(metadata_tmp, metadata, error);
    if (error) {
        fs::remove(metadata_tmp);
        fs::remove(artifact);
        return;
    }
    std::lock_guard<std::mutex> lock(context.mutex);
    ++context.cache_write_count;
}

std::optional<SlotValue> read_cached_geometry(
    const fs::path& root,
    const std::string& kind,
    const std::string& key,
    GeometryRepresentation representation,
    ExecutionContext& context
) {
    if (representation == GeometryRepresentation::MeshDomain) {
        if (auto value = read_cached_manifold(root, kind, key, context)) {
            return SlotValue::manifold_value(*value);
        }
        return std::nullopt;
    }
    if (auto value = read_cached_shape(root, kind, key, context)) {
        return SlotValue::shape_value(*value);
    }
    return std::nullopt;
}

void write_cached_geometry(
    const fs::path& root,
    const std::string& kind,
    const std::string& key,
    const SlotValue& value,
    ExecutionContext& context
) {
    if (value.kind == SlotValue::Kind::Manifold) {
        write_cached_manifold(root, kind, key, value.manifold, context);
    } else if (value.kind == SlotValue::Kind::Shape) {
        write_cached_shape(root, kind, key, value.shape, context);
    } else {
        throw EvalError("geometry cache cannot store a frame slot");
    }
}

std::vector<ShapeRecord> evaluate_plan(
    const Plan& plan,
    const std::optional<fs::path>& cache_root,
    ecky::RenderCacheTransaction* cache_transaction,
    ExecutionContext& context
) {
    if (plan.parts.empty()) throw EvalError("plan needs at least one part");
    if (cache_root.has_value() != (cache_transaction != nullptr)) {
        throw EvalError("selective cache transaction must match cache configuration");
    }
    {
        std::lock_guard<std::mutex> lock(context.mutex);
        for (const Part& part : plan.parts) {
            context.part_representations[part.part_id] = part.representation;
        }
    }
    std::vector<std::string> cache_keys(plan.parts.size());
    std::vector<std::optional<ShapeRecord>> cached_parts(plan.parts.size());
    std::vector<std::vector<std::string>> command_cache_keys(plan.parts.size());
    std::vector<std::map<std::uint64_t, SlotValue>> cached_command_slots(plan.parts.size());
    std::map<std::string, std::string> partial_group_cache_keys;
    std::map<std::string, SlotValue> cached_partial_groups;
    if (cache_root.has_value()) {
        for (std::size_t part_index = 0; part_index < plan.parts.size(); ++part_index) {
            const Part& part = plan.parts[part_index];
            cache_keys[part_index] = part_cache_key(part, context);
            std::optional<SlotValue> cached_part = read_cached_geometry(
                *cache_root, "parts", cache_keys[part_index],
                part.representation, context);
            std::map<std::uint64_t, std::string> fingerprints;
            command_cache_keys[part_index].reserve(part.commands.size());
            for (const Command& command : part.commands) {
                const std::string key = command_cache_key(command, fingerprints, context);
                command_cache_keys[part_index].push_back(key);
                for (const PartialBooleanGroupPlan& group :
                     context.parallel_policy == ParallelPolicy::Adaptive
                         ? plan.partial_boolean_groups
                         : std::vector<PartialBooleanGroupPlan>{}) {
                    if (group.part_key != part.part_id || group.parent_output != command.output) continue;
                    const std::string group_id = partial_group_id(part_index, command.output, group.ordinal);
                    const std::string group_key = partial_boolean_group_cache_key(
                        command, group, fingerprints, context);
                    partial_group_cache_keys.emplace(group_id, group_key);
                    if (!cached_part.has_value()) {
                        if (auto value = read_cached_geometry(
                                *cache_root, "partial-booleans", group_key,
                                group.representation, context)) {
                            cached_partial_groups.emplace(group_id, std::move(*value));
                            std::lock_guard<std::mutex> lock(context.mutex);
                            ++context.partial_boolean_cache_hit_count;
                            context.partial_boolean_group_evidence.push_back(
                                {part.part_id, command.output, group.key, true, 0});
                        } else {
                            std::lock_guard<std::mutex> lock(context.mutex);
                            ++context.partial_boolean_cache_miss_count;
                            context.partial_boolean_group_evidence.push_back(
                                {part.part_id, command.output, group.key, false, 0});
                        }
                    }
                }
                fingerprints.emplace(command.output, key);
                if (!cached_part.has_value() && command_cache_admitted(command)) {
                    const std::string command_id = part.part_id + ":" + std::to_string(command.output);
                    if (auto shape = read_cached_shape(*cache_root, "commands", key, context)) {
                        cached_command_slots[part_index].emplace(command.output, SlotValue::shape_value(*shape));
                        context.command_cache_evidence.push_back({command_id, true, true});
                    } else {
                        context.command_cache_evidence.push_back({command_id, false, true});
                    }
                }
            }
            if (cached_part.has_value()) {
                ShapeRecord record;
                record.part_id = part.part_id;
                record.label = part.label;
                if (part.authored_bindings.empty()) {
                    if (cached_part->kind == SlotValue::Kind::Manifold) {
                        record.kind = ShapeRecord::Kind::Manifold;
                        record.manifold = std::move(cached_part->manifold);
                    } else {
                        record.shape = std::move(cached_part->shape);
                    }
                    cached_parts[part_index] = std::move(record);
                } else if (auto authored_payload = read_cached_shape(
                               *cache_root, "authored-parts",
                               authored_part_cache_key(
                                   part, command_cache_keys[part_index], context),
                               context)) {
                    TopoDS_Iterator children(*authored_payload);
                    if (children.More()) {
                        record.shape = children.Value();
                        children.Next();
                        bool complete = true;
                        for (const Part::AuthoredBinding& binding : part.authored_bindings) {
                            if (!children.More()) {
                                complete = false;
                                break;
                            }
                            record.authored_bindings.emplace(binding.name, children.Value());
                            children.Next();
                        }
                        if (complete && !children.More()) {
                            cached_parts[part_index] = std::move(record);
                        }
                    }
                }
            }
            {
                std::lock_guard<std::mutex> lock(context.mutex);
                context.part_cache_hits[part.part_id] = cached_parts[part_index].has_value();
            }
        }
    }

    const auto execution_refs = [&](std::size_t part_index, const Command& command) {
        std::vector<std::uint64_t> refs;
        std::set<std::uint32_t> cached_group_indices;
        if (context.parallel_policy == ParallelPolicy::Adaptive &&
            command.op == "union" && command.args.size() == 4) {
            for (const PartialBooleanGroupPlan& group : plan.partial_boolean_groups) {
                if (group.part_key != plan.parts[part_index].part_id ||
                    group.parent_output != command.output) {
                    continue;
                }
                const std::string group_id = partial_group_id(
                    part_index, command.output, group.ordinal);
                if (cached_partial_groups.find(group_id) != cached_partial_groups.end()) {
                    cached_group_indices.insert(
                        group.input_indices.begin(), group.input_indices.end());
                }
            }
        }
        for (std::size_t index = 0; index < command.args.size(); ++index) {
            if (cached_group_indices.find(static_cast<std::uint32_t>(index)) !=
                cached_group_indices.end()) {
                continue;
            }
            collect_arg_refs(command.args[index], refs);
        }
        for (const Keyword& keyword : command.keywords) collect_arg_refs(keyword.value, refs);
        return refs;
    };

    std::vector<DagNode> nodes;
    std::vector<std::map<std::uint64_t, std::size_t>> producers(plan.parts.size());
    for (std::size_t part_index = 0; part_index < plan.parts.size(); ++part_index) {
        const Part& part = plan.parts[part_index];
        if (cached_parts[part_index].has_value()) continue;
        for (std::size_t command_index = 0; command_index < part.commands.size(); ++command_index) {
            const Command& command = part.commands[command_index];
            if (cached_command_slots[part_index].find(command.output) != cached_command_slots[part_index].end()) {
                continue;
            }
            if (!producers[part_index].emplace(command.output, nodes.size()).second) {
                throw EvalError("duplicate output slot in part `" + part.part_id + "`");
            }
            DagNode node;
            node.part_index = part_index;
            node.command_index = command_index;
            node.exclusive = !command_has_proven_immutable_inputs(command);
            nodes.push_back(std::move(node));
        }
    }
    for (std::size_t node_index = 0; node_index < nodes.size(); ++node_index) {
        DagNode& node = nodes[node_index];
        const Command& command = plan.parts[node.part_index].commands[node.command_index];
        for (std::uint64_t ref : execution_refs(node.part_index, command)) {
            auto producer = producers[node.part_index].find(ref);
            if (producer == producers[node.part_index].end()) {
                if (cached_command_slots[node.part_index].find(ref) != cached_command_slots[node.part_index].end()) {
                    continue;
                }
                throw EvalError(command.op + " references unknown slot " + std::to_string(ref));
            }
            ++node.unmet_dependencies;
            nodes[producer->second].dependents.push_back(node_index);
        }
    }

    // A command-cache hit supplies its output before scheduling. Walk backward
    // only from uncached part roots, stopping at supplied slots, so upstream
    // work stays dormant unless some other missed root consumes it.
    std::vector<bool> required(nodes.size(), false);
    std::vector<std::size_t> pending;
    for (std::size_t part_index = 0; part_index < plan.parts.size(); ++part_index) {
        if (cached_parts[part_index].has_value()) continue;
        auto root = producers[part_index].find(plan.parts[part_index].root);
        if (root != producers[part_index].end()) pending.push_back(root->second);
        for (const Part::AuthoredBinding& binding : plan.parts[part_index].authored_bindings) {
            auto producer = producers[part_index].find(binding.slot);
            if (producer != producers[part_index].end()) pending.push_back(producer->second);
        }
    }
    while (!pending.empty()) {
        const std::size_t node_index = pending.back();
        pending.pop_back();
        if (required[node_index]) continue;
        required[node_index] = true;
        const DagNode& node = nodes[node_index];
        const Command& command = plan.parts[node.part_index].commands[node.command_index];
        for (std::uint64_t ref : execution_refs(node.part_index, command)) {
            auto producer = producers[node.part_index].find(ref);
            if (producer != producers[node.part_index].end()) pending.push_back(producer->second);
        }
    }
    for (DagNode& node : nodes) {
        node.unmet_dependencies = 0;
        node.dependents.clear();
    }
    std::size_t required_count = 0;
    for (std::size_t node_index = 0; node_index < nodes.size(); ++node_index) {
        if (!required[node_index]) continue;
        ++required_count;
        DagNode& node = nodes[node_index];
        const Command& command = plan.parts[node.part_index].commands[node.command_index];
        for (std::uint64_t ref : execution_refs(node.part_index, command)) {
            auto producer = producers[node.part_index].find(ref);
            if (producer != producers[node.part_index].end() && required[producer->second]) {
                ++node.unmet_dependencies;
                nodes[producer->second].dependents.push_back(node_index);
            }
        }
    }

    // Slot values remain immutable while consumed. Count required consumers up
    // front so a non-root value is released immediately after its last reader.
    // This keeps broad DAGs from retaining every intermediate until export.
    std::vector<std::map<std::uint64_t, std::size_t>> remaining_uses(plan.parts.size());
    for (std::size_t node_index = 0; node_index < nodes.size(); ++node_index) {
        if (!required[node_index]) continue;
        const DagNode& node = nodes[node_index];
        const Command& command = plan.parts[node.part_index].commands[node.command_index];
        for (std::uint64_t ref : execution_refs(node.part_index, command)) {
            ++remaining_uses[node.part_index][ref];
        }
    }
    for (std::size_t part_index = 0; part_index < plan.parts.size(); ++part_index) {
        for (const Part::AuthoredBinding& binding : plan.parts[part_index].authored_bindings) {
            ++remaining_uses[part_index][binding.slot];
        }
    }

    std::vector<std::map<std::uint64_t, SlotValue>> slots = std::move(cached_command_slots);
    for (std::size_t part_index = 0; part_index < slots.size(); ++part_index) {
        for (auto value = slots[part_index].begin(); value != slots[part_index].end();) {
            if (value->first != plan.parts[part_index].root &&
                remaining_uses[part_index][value->first] == 0) {
                value = slots[part_index].erase(value);
                std::lock_guard<std::mutex> lock(context.mutex);
                ++context.released_slot_count;
            } else {
                ++value;
            }
        }
    }
    std::set<std::size_t> ready;
    for (std::size_t index = 0; index < nodes.size(); ++index) {
        if (required[index] && nodes[index].unmet_dependencies == 0) ready.insert(index);
    }
    std::size_t completed_count = 0;
    while (!ready.empty()) {
        std::vector<std::size_t> batch;
        const std::size_t first = *ready.begin();
        if (nodes[first].exclusive) {
            batch.push_back(first);
        } else {
            for (std::size_t candidate : ready) {
                if (nodes[candidate].exclusive) continue;
                batch.push_back(candidate);
                if (batch.size() == context.worker_budget) break;
            }
            if (batch.empty()) batch.push_back(first);
        }
        for (std::size_t index : batch) ready.erase(index);
        {
            std::lock_guard<std::mutex> lock(context.mutex);
            context.active_dag_nodes = static_cast<std::uint32_t>(batch.size());
            context.peak_dag_concurrency = std::max(
                context.peak_dag_concurrency, context.active_dag_nodes);
            context.peak_total_allocated_cpu_units = std::max(
                context.peak_total_allocated_cpu_units, context.active_dag_nodes);
        }
        std::vector<std::future<CompletedDagNode>> running;
        running.reserve(batch.size());
        for (std::size_t node_index : batch) {
            const DagNode& node = nodes[node_index];
            const Part& part = plan.parts[node.part_index];
            const Command& command = part.commands[node.command_index];
            const std::map<std::uint64_t, SlotValue> inputs = slots[node.part_index];
            std::vector<PartialBooleanGroupPlan> task_groups;
            std::map<std::uint32_t, SlotValue> task_cached_groups;
            for (const PartialBooleanGroupPlan& group :
                 cache_root.has_value() && context.parallel_policy == ParallelPolicy::Adaptive
                     ? plan.partial_boolean_groups
                     : std::vector<PartialBooleanGroupPlan>{}) {
                if (group.part_key != part.part_id || group.parent_output != command.output) continue;
                task_groups.push_back(group);
                const std::string group_id = partial_group_id(node.part_index, command.output, group.ordinal);
                if (auto hit = cached_partial_groups.find(group_id); hit != cached_partial_groups.end()) {
                    task_cached_groups.emplace(group.ordinal, hit->second);
                }
            }
            running.push_back(std::async(std::launch::async, [&, node_index, inputs, task_groups, task_cached_groups]() {
                const DagNode& task = nodes[node_index];
                const Part& task_part = plan.parts[task.part_index];
                const Command& task_command = task_part.commands[task.command_index];
                try {
                    const auto command_started_at = std::chrono::steady_clock::now();
                    SlotValue value;
                    std::vector<std::pair<std::string, SlotValue>> partial_outputs;
                    if (task_command.op == "union" && task_groups.size() == 2 &&
                        task_command.args.size() == 4) {
                        const bool hybrid = std::any_of(
                            task_groups.begin(), task_groups.end(),
                            [](const PartialBooleanGroupPlan& group) {
                                return group.representation ==
                                    GeometryRepresentation::MeshDomain;
                            });
                        if (task_cached_groups.size() == 2) {
                            value = union_geometry_values(
                                {task_cached_groups.at(0), task_cached_groups.at(1)}, context);
                        } else if (task_cached_groups.size() == 1) {
                            const std::uint32_t hit_ordinal = task_cached_groups.begin()->first;
                            const std::uint32_t dirty_ordinal = hit_ordinal == 0 ? 1 : 0;
                            const PartialBooleanGroupPlan& dirty_group = *std::find_if(
                                task_groups.begin(), task_groups.end(), [&](const auto& group) {
                                    return group.ordinal == dirty_ordinal;
                                });
                            std::vector<SlotValue> dirty_operands;
                            for (std::uint32_t input_index : dirty_group.input_indices) {
                                dirty_operands.push_back(lookup_geometry_slot(
                                    inputs,
                                    task_command.args[input_index].ref_value,
                                    "union"));
                            }
                            SlotValue dirty = union_geometry_values(dirty_operands, context);
                            require_geometry_representation(
                                dirty, dirty_group.representation,
                                "partial Boolean group `" + dirty_group.key + "`");
                            record_partial_group_recompute(
                                context, task_part.part_id, task_command.output, dirty_group.key);
                            const std::string dirty_id = partial_group_id(
                                task.part_index, task_command.output, dirty_ordinal);
                            partial_outputs.emplace_back(
                                partial_group_cache_keys.at(dirty_id), dirty);
                            value = hit_ordinal == 0
                                ? union_geometry_values(
                                    {task_cached_groups.begin()->second, dirty}, context)
                                : union_geometry_values(
                                    {dirty, task_cached_groups.begin()->second}, context);
                        } else if (hybrid) {
                            std::map<std::uint32_t, SlotValue> group_results;
                            for (const PartialBooleanGroupPlan& group : task_groups) {
                                std::vector<SlotValue> group_operands;
                                for (std::uint32_t input_index : group.input_indices) {
                                    group_operands.push_back(lookup_geometry_slot(
                                        inputs,
                                        task_command.args[input_index].ref_value,
                                        "union"));
                                }
                                SlotValue group_result = union_geometry_values(
                                    group_operands, context);
                                require_geometry_representation(
                                    group_result, group.representation,
                                    "partial Boolean group `" + group.key + "`");
                                record_partial_group_recompute(
                                    context, task_part.part_id, task_command.output, group.key);
                                const std::string group_id = partial_group_id(
                                    task.part_index, task_command.output, group.ordinal);
                                partial_outputs.emplace_back(
                                    partial_group_cache_keys.at(group_id), group_result);
                                group_results.emplace(group.ordinal, std::move(group_result));
                            }
                            value = union_geometry_values(
                                {group_results.at(0), group_results.at(1)}, context);
                        } else {
                            std::vector<TopoDS_Shape> operands;
                            for (const Arg& arg : task_command.args) {
                                operands.push_back(lookup_shape(inputs, arg.ref_value, "union"));
                            }
                            {
                                std::lock_guard<std::mutex> lock(context.mutex);
                                ++context.four_way_intersection_count;
                            }
                            PreparedUnionResult prepared = prepare_four_way_union(
                                operands, task_groups, context);
                            value = SlotValue::shape_value(prepared.full);
                            for (const PartialBooleanGroupPlan& group : task_groups) {
                                record_partial_group_recompute(
                                    context, task_part.part_id, task_command.output, group.key);
                                const std::string group_id = partial_group_id(
                                    task.part_index, task_command.output, group.ordinal);
                                partial_outputs.emplace_back(
                                    partial_group_cache_keys.at(group_id),
                                    SlotValue::shape_value(prepared.groups.at(group.ordinal)));
                            }
                        }
                    } else {
                        const bool force_mesh_booleans =
                            task_part.representation == GeometryRepresentation::MeshDomain &&
                            std::none_of(
                                plan.partial_boolean_groups.begin(),
                                plan.partial_boolean_groups.end(),
                                [&](const PartialBooleanGroupPlan& group) {
                                    return group.part_key == task_part.part_id;
                                });
                        value = evaluate_command(
                            task_command,
                            inputs,
                            task_part.part_id,
                            force_mesh_booleans,
                            context);
                    }
                    if (task_command.op == "union" || task_command.op == "difference" ||
                        task_command.op == "intersection") {
                        const auto elapsed_ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                            std::chrono::steady_clock::now() - command_started_at).count();
                        std::lock_guard<std::mutex> lock(context.mutex);
                        context.command_timing_evidence.push_back(CommandTimingEvidence{
                            task_part.part_id + ":" + std::to_string(task_command.output),
                            task_command.op,
                            static_cast<std::uint64_t>(std::max<std::int64_t>(0, elapsed_ms)),
                        });
                    }
                    return CompletedDagNode{
                        node_index, std::move(value), std::move(partial_outputs)};
                } catch (const Standard_Failure& error) {
                    const char* raw = error.GetMessageString();
                    throw OcctRuntimeError(
                        "Direct OCCT part `" + task_part.part_id + "` command #" +
                        std::to_string(task.command_index) + " output slot " +
                        std::to_string(task_command.output) + " op `" + task_command.op +
                        "` failed: " + (raw ? raw : "unknown OCCT failure"));
                }
            }));
        }
        for (auto& future : running) {
            CompletedDagNode result = future.get();
            const DagNode& node = nodes[result.node_index];
            const Part& part = plan.parts[node.part_index];
            const Command& command = part.commands[node.command_index];
            if (cache_root.has_value() && command_cache_admitted(command) &&
                result.value.kind == SlotValue::Kind::Shape) {
                write_cached_shape(
                    cache_transaction->staging_root(), "commands",
                    command_cache_keys[node.part_index][node.command_index], result.value.shape, context);
            }
            if (cache_root.has_value()) {
                for (const auto& [key, value] : result.partial_boolean_outputs) {
                    write_cached_geometry(
                        cache_transaction->staging_root(), "partial-booleans",
                        key, value, context);
                    std::lock_guard<std::mutex> lock(context.mutex);
                    ++context.partial_boolean_cache_write_count;
                }
            }
            slots[node.part_index].emplace(command.output, std::move(result.value));
            {
                std::lock_guard<std::mutex> lock(context.mutex);
                ++context.part_executed_commands[part.part_id];
                context.part_executed_command_ids[part.part_id].push_back(
                    part.part_id + ":" + std::to_string(command.output));
            }
            for (std::uint64_t ref : execution_refs(node.part_index, command)) {
                auto remaining = remaining_uses[node.part_index].find(ref);
                if (remaining == remaining_uses[node.part_index].end() || remaining->second == 0) {
                    throw EvalError("slot consumer count underflow for slot " + std::to_string(ref));
                }
                --remaining->second;
                if (remaining->second == 0 && ref != part.root) {
                    auto value = slots[node.part_index].find(ref);
                    if (value != slots[node.part_index].end()) {
                        slots[node.part_index].erase(value);
                        std::lock_guard<std::mutex> lock(context.mutex);
                        ++context.released_slot_count;
                    }
                }
            }
            ++completed_count;
            for (std::size_t dependent : node.dependents) {
                if (--nodes[dependent].unmet_dependencies == 0) ready.insert(dependent);
            }
        }
    }
    if (completed_count != required_count) throw EvalError("command graph contains a dependency cycle");

    std::vector<ShapeRecord> parts;
    parts.reserve(plan.parts.size());
    for (std::size_t part_index = 0; part_index < plan.parts.size(); ++part_index) {
        const Part& part = plan.parts[part_index];
        if (cached_parts[part_index].has_value()) {
            parts.push_back(std::move(*cached_parts[part_index]));
            continue;
        }
        auto root = slots[part_index].find(part.root);
        if (root == slots[part_index].end()) throw EvalError("missing root shape for part `" + part.part_id + "`");
        if (root->second.kind == SlotValue::Kind::Frame) {
            throw EvalError("root slot for part `" + part.part_id + "` is not geometry");
        }
        require_geometry_representation(
            root->second, part.representation, "part `" + part.part_id + "` root");
        ShapeRecord record;
        record.part_id = part.part_id;
        record.label = part.label;
        if (root->second.kind == SlotValue::Kind::Manifold) {
            record.kind = ShapeRecord::Kind::Manifold;
            record.manifold = root->second.manifold;
        } else {
            record.shape = root->second.shape;
        }
        for (const Part::AuthoredBinding& binding : part.authored_bindings) {
            auto authored = slots[part_index].find(binding.slot);
            if (authored == slots[part_index].end()) {
                throw EvalError(
                    "missing authored binding slot " + std::to_string(binding.slot) +
                    " (`" + binding.name + "`) in part `" + part.part_id + "`");
            }
            if (authored->second.kind != SlotValue::Kind::Shape) {
                throw EvalError(
                    "authored binding `" + binding.name + "` in part `" + part.part_id +
                    "` is not analytic BRep geometry");
            }
            record.authored_bindings.emplace(binding.name, authored->second.shape);
        }
        if (cache_root.has_value() && !part.authored_bindings.empty()) {
            TopoDS_Compound authored_payload;
            BRep_Builder builder;
            builder.MakeCompound(authored_payload);
            builder.Add(authored_payload, root->second.shape);
            for (const Part::AuthoredBinding& binding : part.authored_bindings) {
                builder.Add(authored_payload, slots[part_index].at(binding.slot).shape);
            }
            write_cached_shape(
                cache_transaction->staging_root(), "authored-parts",
                authored_part_cache_key(part, command_cache_keys[part_index], context),
                authored_payload, context);
        }
        if (cache_root.has_value()) {
            write_cached_geometry(
                cache_transaction->staging_root(), "parts", cache_keys[part_index],
                root->second, context);
        }
        parts.push_back(std::move(record));
    }
    return parts;
}

void write_step_file(const fs::path& path, const std::vector<TopoDS_Shape>& shapes) {
    if (shapes.empty()) throw IoError("cannot write STEP without shapes");
    // OCCT 7.9 corrupts a TCollection_AsciiString while destroying an AP242
    // writer that translated a triangulation-only face. This runner handles
    // exactly one plan, so keep writer state process-scoped and let the OS
    // reclaim it at exit. Peak memory remains bounded by the render guard.
    auto* writer = new STEPControl_Writer();
    auto* parameters = new DESTEP_Parameters();
    parameters->WriteSchema = DESTEP_Parameters::WriteMode_StepSchema_AP242DIS;
    parameters->WriteTessellated = DESTEP_Parameters::RWMode_Tessellated_OnNoBRep;
    const TopoDS_Shape export_shape = shapes.size() == 1
        ? shapes.front()
        : compound_shapes(shapes);
    if (writer->Transfer(export_shape, STEPControl_AsIs, *parameters) != IFSelect_RetDone) {
        throw IoError("failed to transfer STEP shape");
    }
    if (writer->Write(path.string().c_str()) != IFSelect_RetDone) {
        throw IoError("failed to write STEP");
    }
    std::ifstream input(path, std::ios::binary);
    std::string bytes((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
    const std::string timestamp_prefix = "FILE_NAME('Open CASCADE Shape Model','";
    const std::size_t timestamp_start = bytes.find(timestamp_prefix);
    if (timestamp_start == std::string::npos) {
        throw IoError("failed to locate STEP timestamp for deterministic export");
    }
    const std::size_t value_start = timestamp_start + timestamp_prefix.size();
    const std::size_t value_end = bytes.find('\'', value_start);
    if (value_end == std::string::npos) {
        throw IoError("failed to parse STEP timestamp for deterministic export");
    }
    bytes.replace(value_start, value_end - value_start, "1970-01-01T00:00:00");
    input.close();
    std::ofstream output(path, std::ios::binary | std::ios::trunc);
    output.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    if (!output) throw IoError("failed to normalize STEP timestamp");
}

// Preview/export STL tessellation quality. Use FreeCAD-style adaptive angular
// deflection so curved surfaces stay smooth at small scales and avoid
// over-tessellating large parts. Linear deflection stays fixed.
static constexpr double kStlLinearDeflection = 0.04;  // mm chord error
static constexpr double kStlAngularDeflectionMin = 0.005;
static constexpr double kStlAngularDeflectionMax = 0.1;

static constexpr double stl_angular_deflection_for_linear(double linear) {
    const double adaptive = linear * 5.0 + 0.005;
    return std::clamp(adaptive, kStlAngularDeflectionMin, kStlAngularDeflectionMax);
}

// Weld tolerance for STL vertices. Boolean rebuilds and transform round-trips
// leave duplicated boundary topology whose tessellations drift a few double
// ULPs apart (e.g. a shared glyph-outline vertex written as two coordinates
// straddling an f32 rounding boundary). Downstream manifold checks compare
// exact bits, so even 1e-16 drift reads as a crack. Welding at 1e-6 mm is two
// orders below the 0.04 mm chord error and far below any modelled clearance.
constexpr double kStlWeldTolerance = 1.0e-6;

// Snap a point to the coordinates of a previously seen point within the weld
// tolerance (spatial hash over grid cells, checking neighbor cells so pairs
// straddling a cell boundary still merge).
class StlVertexWelder {
public:
    gp_Pnt weld(const gp_Pnt& point) {
        const std::int64_t cx = cell(point.X());
        const std::int64_t cy = cell(point.Y());
        const std::int64_t cz = cell(point.Z());
        for (std::int64_t dx = -1; dx <= 1; ++dx) {
            for (std::int64_t dy = -1; dy <= 1; ++dy) {
                for (std::int64_t dz = -1; dz <= 1; ++dz) {
                    auto bucket = buckets_.find(key(cx + dx, cy + dy, cz + dz));
                    if (bucket == buckets_.end()) {
                        continue;
                    }
                    for (const gp_Pnt& candidate : bucket->second) {
                        if (point.SquareDistance(candidate) <=
                            kStlWeldTolerance * kStlWeldTolerance) {
                            return candidate;
                        }
                    }
                }
            }
        }
        gp_Pnt snapped(
            point.X() == 0.0 ? 0.0 : point.X(),
            point.Y() == 0.0 ? 0.0 : point.Y(),
            point.Z() == 0.0 ? 0.0 : point.Z());
        buckets_[key(cx, cy, cz)].push_back(snapped);
        return snapped;
    }

private:
    static std::int64_t cell(double value) {
        return static_cast<std::int64_t>(std::floor(value / kStlWeldTolerance));
    }

    static std::string key(std::int64_t x, std::int64_t y, std::int64_t z) {
        return std::to_string(x) + ":" + std::to_string(y) + ":" + std::to_string(z);
    }

    std::map<std::string, std::vector<gp_Pnt>> buckets_;
};

constexpr std::size_t kPartMeshReservationBytes = 256ULL * 1024ULL * 1024ULL;
constexpr std::size_t kDefaultPartMeshMemoryBudgetBytes = 512ULL * 1024ULL * 1024ULL;

std::size_t configured_part_mesh_memory_budget_bytes() {
    const char* raw = std::getenv("ECKY_DIRECT_OCCT_MESH_MEMORY_BYTES");
    if (!raw || !*raw) return kDefaultPartMeshMemoryBudgetBytes;
    try {
        const unsigned long long bytes = std::stoull(raw);
        if (bytes < kPartMeshReservationBytes) {
            throw ParseError("ECKY_DIRECT_OCCT_MESH_MEMORY_BYTES must admit one 256 MiB mesh reservation");
        }
        return static_cast<std::size_t>(bytes);
    } catch (const ParseError&) {
        throw;
    } catch (...) {
        throw ParseError("ECKY_DIRECT_OCCT_MESH_MEMORY_BYTES must be an unsigned integer");
    }
}

std::string part_mesh_runtime_policy_identity(const ExecutionContext& context) {
    if (context.runner_binary_digest.empty()) {
        throw EvalError("runner binary digest unavailable for part mesh identity");
    }
    return std::string(ecky::kPartMeshPolicyIdentity) + "|cache=" + kSelectiveCacheSchema +
        "|abi=" + direct_occt_runner_abi_digest() + "|binary=" + context.runner_binary_digest +
        "|occt=" + OCC_VERSION_COMPLETE;
}

ecky::PartMesh tessellate_brep_part_mesh(
    const TopoDS_Shape& shape, const std::string& part_id, ExecutionContext& context
) {
    StageExecutionTimer stage_timer(context, "mesh");
    const double angular_deflection = stl_angular_deflection_for_linear(kStlLinearDeflection);
    // BRepMesh stores triangulation on face TShapes. Parts may be instances of
    // one root, so give every concurrent job private topology while retaining
    // read-only analytic geometry and dropping any prior triangulation.
    BRepBuilderAPI_Copy private_topology(shape, Standard_False, Standard_False);
    const TopoDS_Shape mesh_shape = private_topology.Shape();
    if (mesh_shape.IsNull()) throw EvalError("failed to create private topology for part mesh");
    // The outer mesh scheduler and this inner mesher share the process-wide
    // OSD pool configured from one total worker budget.
    const Standard_Boolean mesher_parallel =
        context.worker_budget > 1 ? Standard_True : Standard_False;
    BRepMesh_IncrementalMesh mesher(
        mesh_shape, kStlLinearDeflection, Standard_False, angular_deflection, mesher_parallel);
    (void)mesher;

    ecky::PartMesh mesh;
    mesh.part_id = part_id;
    StlVertexWelder welder;
    for (TopExp_Explorer face_explorer(mesh_shape, TopAbs_FACE); face_explorer.More(); face_explorer.Next()) {
        TopoDS_Face face = TopoDS::Face(face_explorer.Current());
        TopLoc_Location location;
        Handle(Poly_Triangulation) triangulation = BRep_Tool::Triangulation(face, location);
        if (triangulation.IsNull()) continue;
        const gp_Trsf transform = location.Transformation();
        for (Standard_Integer triangle_index = 1;
             triangle_index <= triangulation->NbTriangles();
             ++triangle_index) {
            Standard_Integer n1 = 0;
            Standard_Integer n2 = 0;
            Standard_Integer n3 = 0;
            triangulation->Triangle(triangle_index).Get(n1, n2, n3);
            gp_Pnt p1 = welder.weld(triangulation->Node(n1).Transformed(transform));
            gp_Pnt p2 = welder.weld(triangulation->Node(n2).Transformed(transform));
            gp_Pnt p3 = welder.weld(triangulation->Node(n3).Transformed(transform));
            if (face.Orientation() == TopAbs_REVERSED) std::swap(p2, p3);
            const gp_Vec normal = gp_Vec(p1, p2).Crossed(gp_Vec(p1, p3));
            if (normal.SquareMagnitude() <= 1.0e-18) continue;
            ecky::MeshTriangle triangle;
            triangle.vertices[0] = {p1.X(), p1.Y(), p1.Z()};
            triangle.vertices[1] = {p2.X(), p2.Y(), p2.Z()};
            triangle.vertices[2] = {p3.X(), p3.Y(), p3.Z()};
            mesh.triangles.push_back(triangle);
        }
    }
    if (mesh.triangles.empty()) {
        throw IoError("failed to build part mesh: shape produced no triangulated faces");
    }
    if (mesh.resident_bytes() > kPartMeshReservationBytes) {
        throw EvalError("part mesh exceeded the 256 MiB bounded mesh reservation; split the part or raise the reservation policy");
    }
    return mesh;
}

ecky::PartMesh manifold_part_mesh(
    const manifold::Manifold& value,
    const std::string& part_id,
    ExecutionContext& context
) {
    StageExecutionTimer stage_timer(context, "mesh");
    require_manifold_status(value, "part mesh");
    const manifold::MeshGL64 source = value.GetMeshGL64();
    if (source.numProp < 3 || source.triVerts.empty() || source.triVerts.size() % 3 != 0) {
        throw EvalError("Manifold part produced invalid indexed mesh");
    }
    ecky::PartMesh mesh;
    mesh.part_id = part_id;
    mesh.triangles.reserve(source.NumTri());
    const auto point = [&](std::uint64_t index) {
        if (index >= source.NumVert()) {
            throw EvalError("Manifold part triangle index is out of bounds");
        }
        const std::size_t offset = static_cast<std::size_t>(index) * source.numProp;
        return ecky::MeshPoint{
            source.vertProperties[offset],
            source.vertProperties[offset + 1],
            source.vertProperties[offset + 2]};
    };
    for (std::size_t triangle = 0; triangle < source.NumTri(); ++triangle) {
        mesh.triangles.push_back({{
            point(source.triVerts[triangle * 3]),
            point(source.triVerts[triangle * 3 + 1]),
            point(source.triVerts[triangle * 3 + 2]),
        }});
    }
    if (mesh.resident_bytes() > kPartMeshReservationBytes) {
        throw EvalError("Manifold part mesh exceeded the bounded mesh reservation");
    }
    return mesh;
}

void write_part_mesh_stl_file(const fs::path& path, const std::vector<const ecky::PartMesh*>& meshes) {
    std::uint64_t triangle_count = 0;
    for (const ecky::PartMesh* mesh : meshes) {
        triangle_count += mesh->triangles.size();
    }
    if (triangle_count == 0 || triangle_count > std::numeric_limits<std::uint32_t>::max()) {
        throw IoError("failed to write STL: invalid part mesh triangle count");
    }
    std::ofstream out(path, std::ios::binary);
    if (!out) throw IoError("failed to write STL");
    const std::string header(80, '\0');
    out.write(header.data(), 80);
    const std::uint32_t count = static_cast<std::uint32_t>(triangle_count);
    out.write(reinterpret_cast<const char*>(&count), 4);
    const auto write_float = [&](double value) {
        const float float_value = static_cast<float>(value);
        out.write(reinterpret_cast<const char*>(&float_value), 4);
    };
    for (const ecky::PartMesh* mesh : meshes) {
        for (const ecky::MeshTriangle& triangle : mesh->triangles) {
            const ecky::MeshPoint& p1 = triangle.vertices[0];
            const ecky::MeshPoint& p2 = triangle.vertices[1];
            const ecky::MeshPoint& p3 = triangle.vertices[2];
            double nx = (p2.y - p1.y) * (p3.z - p1.z) - (p2.z - p1.z) * (p3.y - p1.y);
            double ny = (p2.z - p1.z) * (p3.x - p1.x) - (p2.x - p1.x) * (p3.z - p1.z);
            double nz = (p2.x - p1.x) * (p3.y - p1.y) - (p2.y - p1.y) * (p3.x - p1.x);
            const double magnitude = std::sqrt(nx * nx + ny * ny + nz * nz);
            if (magnitude > 0.0) { nx /= magnitude; ny /= magnitude; nz /= magnitude; }
            write_float(nx); write_float(ny); write_float(nz);
            write_float(p1.x); write_float(p1.y); write_float(p1.z);
            write_float(p2.x); write_float(p2.y); write_float(p2.z);
            write_float(p3.x); write_float(p3.y); write_float(p3.z);
            const std::uint16_t attribute = 0;
            out.write(reinterpret_cast<const char*>(&attribute), 2);
        }
    }
    if (!out.good()) throw IoError("failed to write STL: I/O error after writing part mesh triangles");
}

std::vector<ecky::PartMesh> build_part_meshes(
    const Plan& plan,
    const std::vector<ShapeRecord>& parts,
    const std::optional<fs::path>& cache_root,
    ecky::RenderCacheTransaction* cache_transaction,
    ExecutionContext& context
) {
    struct InFlightMesh {
        std::size_t part_index = 0;
        std::string identity;
        std::future<ecky::PartMesh> result;
    };

    std::vector<std::optional<ecky::PartMesh>> meshes(parts.size());
    std::vector<std::string> identities(parts.size());
    std::unique_ptr<ecky::PartMeshCache> cache;
    std::unique_ptr<ecky::PartMeshCache> staged_cache;
    if (cache_root.has_value()) {
        cache = std::make_unique<ecky::PartMeshCache>(*cache_root / "part-meshes");
        staged_cache = std::make_unique<ecky::PartMeshCache>(
            cache_transaction->staging_root() / "part-meshes");
    }
    const std::string policy_identity = part_mesh_runtime_policy_identity(context);
    for (std::size_t index = 0; index < parts.size(); ++index) {
        identities[index] = ecky::part_mesh_identity(part_cache_key(plan.parts[index], context), policy_identity);
        if (!cache) continue;
        if (auto hit = cache->read(identities[index])) {
            meshes[index] = std::move(*hit);
            std::lock_guard<std::mutex> lock(context.mutex);
            ++context.mesh_cache_hit_count;
            context.part_mesh_evidence[parts[index].part_id] = {
                identities[index], static_cast<std::uint64_t>(meshes[index]->triangles.size())};
        }
    }

    ecky::PartMeshMemoryBudget memory_budget(configured_part_mesh_memory_budget_bytes());
    std::vector<InFlightMesh> in_flight;
    std::size_t next_part = 0;
    while (next_part < parts.size() || !in_flight.empty()) {
        while (next_part < parts.size() &&
               in_flight.size() < context.mesh_outer_worker_budget) {
            const std::size_t index = next_part++;
            if (meshes[index].has_value()) continue;
            auto lease = memory_budget.try_reserve(kPartMeshReservationBytes);
            if (!lease.has_value()) {
                --next_part;
                break;
            }
            const ShapeRecord part = parts[index];
            in_flight.push_back({
                index,
                identities[index],
                std::async(std::launch::async, [part, &context, lease = std::move(*lease)]() mutable {
                    return part.kind == ShapeRecord::Kind::Manifold
                        ? manifold_part_mesh(part.manifold, part.part_id, context)
                        : tessellate_brep_part_mesh(part.shape, part.part_id, context);
                })
            });
        }
        if (in_flight.empty()) {
            if (next_part == parts.size()) break;
            throw EvalError("part mesh scheduler cannot admit one mesh under the configured memory budget");
        }
        InFlightMesh completed = std::move(in_flight.front());
        in_flight.erase(in_flight.begin());
        ecky::PartMesh mesh = completed.result.get();
        if (staged_cache) {
            staged_cache->write(completed.identity, mesh);
            std::lock_guard<std::mutex> lock(context.mutex);
            ++context.cache_write_count;
        }
        meshes[completed.part_index] = std::move(mesh);
        std::lock_guard<std::mutex> lock(context.mutex);
        ++context.mesh_build_count;
        context.part_mesh_evidence[parts[completed.part_index].part_id] = {
            completed.identity, static_cast<std::uint64_t>(meshes[completed.part_index]->triangles.size())};
    }

    std::vector<ecky::PartMesh> result;
    result.reserve(parts.size());
    for (std::optional<ecky::PartMesh>& mesh : meshes) result.push_back(std::move(*mesh));
    return result;
}

void write_stl_file(const fs::path& path, const TopoDS_Shape& shape, ExecutionContext& context) {
    StageExecutionTimer stage_timer(context, "mesh");
    const double angular_deflection = stl_angular_deflection_for_linear(kStlLinearDeflection);
    BRepMesh_IncrementalMesh mesh(
        shape, kStlLinearDeflection, Standard_False, angular_deflection, Standard_True);
    // Collect triangles first (welded, degenerate-skipped), then write as binary
    // STL. Binary format is required because downstream multipart export
    // (3MF / zip) parses the binary triangle-count header; ASCII STL makes the
    // count field read as garbage and the parser fails with "failed to fill
    // whole buffer".
    struct Triangle { gp_Pnt p1, p2, p3; };
    std::vector<Triangle> triangles;
    StlVertexWelder welder;
    for (TopExp_Explorer face_explorer(shape, TopAbs_FACE); face_explorer.More(); face_explorer.Next()) {
        TopoDS_Face face = TopoDS::Face(face_explorer.Current());
        TopLoc_Location location;
        Handle(Poly_Triangulation) triangulation = BRep_Tool::Triangulation(face, location);
        if (triangulation.IsNull()) {
            continue;
        }
        gp_Trsf transform = location.Transformation();
        for (Standard_Integer triangle_index = 1;
             triangle_index <= triangulation->NbTriangles();
             ++triangle_index) {
            Standard_Integer n1 = 0;
            Standard_Integer n2 = 0;
            Standard_Integer n3 = 0;
            triangulation->Triangle(triangle_index).Get(n1, n2, n3);
            gp_Pnt p1 = welder.weld(triangulation->Node(n1).Transformed(transform));
            gp_Pnt p2 = welder.weld(triangulation->Node(n2).Transformed(transform));
            gp_Pnt p3 = welder.weld(triangulation->Node(n3).Transformed(transform));
            if (face.Orientation() == TopAbs_REVERSED) {
                std::swap(p2, p3);
            }
            gp_Vec edge_a(p1, p2);
            gp_Vec edge_b(p1, p3);
            gp_Vec normal = edge_a.Crossed(edge_b);
            if (normal.SquareMagnitude() <= 1.0e-18) {
                continue;
            }
            triangles.push_back({p1, p2, p3});
        }
    }
    if (triangles.empty()) {
        throw IoError("failed to write STL: shape produced no triangulated faces");
    }
    std::ofstream out(path, std::ios::binary);
    if (!out) {
        throw IoError("failed to write STL");
    }
    // 80-byte header (blank) + 4-byte little-endian triangle count + 50 bytes
    // per triangle (12 normal + 3*12 vertices + 2 attribute).
    std::string header(80, '\0');
    out.write(header.data(), 80);
    std::uint32_t count = static_cast<std::uint32_t>(triangles.size());
    out.write(reinterpret_cast<const char*>(&count), 4);
    auto write_float = [&](float value) {
        out.write(reinterpret_cast<const char*>(&value), 4);
    };
    for (const auto& tri : triangles) {
        gp_Vec edge_a(tri.p1, tri.p2);
        gp_Vec edge_b(tri.p1, tri.p3);
        gp_Vec normal = edge_a.Crossed(edge_b);
        if (normal.SquareMagnitude() > 1.0e-18) {
            normal.Normalize();
        }
        write_float(static_cast<float>(normal.X()));
        write_float(static_cast<float>(normal.Y()));
        write_float(static_cast<float>(normal.Z()));
        write_float(static_cast<float>(tri.p1.X()));
        write_float(static_cast<float>(tri.p1.Y()));
        write_float(static_cast<float>(tri.p1.Z()));
        write_float(static_cast<float>(tri.p2.X()));
        write_float(static_cast<float>(tri.p2.Y()));
        write_float(static_cast<float>(tri.p2.Z()));
        write_float(static_cast<float>(tri.p3.X()));
        write_float(static_cast<float>(tri.p3.Y()));
        write_float(static_cast<float>(tri.p3.Z()));
        std::uint16_t attr = 0;
        out.write(reinterpret_cast<const char*>(&attr), 2);
    }
    if (!out.good()) {
        throw IoError("failed to write STL: I/O error after writing triangles");
    }
}

std::string read_text_file(const fs::path& path) {
    std::ifstream input(path);
    if (!input) {
        throw ParseError("failed to open plan file");
    }
    std::ostringstream buffer;
    buffer << input.rdbuf();
    return buffer.str();
}

void write_error_json(
    const std::string& klass,
    const std::string& code,
    const std::string& message,
    const std::string& details
) {
    std::cerr << "{\"class\":";
    std::cerr << quote_json_string(klass);
    std::cerr << ",\"code\":";
    std::cerr << quote_json_string(code);
    std::cerr << ",\"message\":";
    std::cerr << quote_json_string(message);
    std::cerr << ",\"details\":";
    std::cerr << quote_json_string(details);
    std::cerr << "}" << std::endl;
}

void run_occt_export_stage(
    const std::string& stage,
    const std::function<void()>& action,
    ExecutionContext& context
) {
    context.set_stage(stage);
    try {
        action();
    } catch (const Standard_Failure& error) {
        const char* raw_message = error.GetMessageString();
        const std::string message = raw_message ? raw_message : "unknown OCCT failure";
        throw OcctRuntimeError(
            "Direct OCCT export stage `" + stage + "` failed: " + message
        );
    }
}

int run(int argc, char** argv, ExecutionContext& context) {
    context.worker_budget = configured_worker_budget();
    context.parallel_policy = configured_parallel_policy();
    context.report_started_at = std::chrono::steady_clock::now();
    context.set_stage("parse-arguments");
    fs::path plan_path;
    fs::path out_dir;
    for (int index = 1; index < argc; ++index) {
        std::string arg = argv[index];
        if (arg == "--help") {
            std::cout << "direct-occt-runner --plan PLAN --out DIR\n";
            return 0;
        }
        if (arg == "--version") {
            std::cout << "direct-occt-runner 0.1.0\n";
            return 0;
        }
        if (arg == "--plan" && index + 1 < argc) {
            plan_path = argv[++index];
            continue;
        }
        if (arg == "--out" && index + 1 < argc) {
            out_dir = argv[++index];
            continue;
        }
        throw ParseError("usage: direct-occt-runner --plan PLAN --out DIR");
    }

    if (plan_path.empty() || out_dir.empty()) {
        throw ParseError("usage: direct-occt-runner --plan PLAN --out DIR");
    }

    std::error_code runner_path_error;
    const fs::path runner_path = fs::canonical(fs::path(argv[0]), runner_path_error);
    if (runner_path_error) {
        throw IoError("cannot resolve running Direct OCCT binary for cache identity: " +
                      runner_path_error.message());
    }
    context.runner_binary_digest = "sha256:" + ecky::sha256_hex(read_binary_file(runner_path));

    context.set_stage("read-plan");
    std::string plan_text = read_text_file(plan_path);
    context.set_stage("parse-plan");
    yyjson_read_err json_error;
    std::unique_ptr<yyjson_doc, decltype(&yyjson_doc_free)> document(
        yyjson_read_opts(plan_text.data(), plan_text.size(), YYJSON_READ_NOFLAG, nullptr, &json_error),
        yyjson_doc_free
    );
    if (!document) {
        throw ParseError(
            "plan JSON parse failed at byte " + std::to_string(json_error.pos) + ": " +
            std::string(json_error.msg ? json_error.msg : "unknown parse error")
        );
    }
    const Plan plan = parse_plan(yyjson_doc_get_root(document.get()));
    configure_mesh_parallelism_budget(plan.parts.size(), context);
    const std::optional<fs::path> cache_root = selective_cache_root();
    std::optional<ecky::RenderCacheTransaction> cache_transaction;
    if (cache_root.has_value()) cache_transaction.emplace(*cache_root);
    context.set_stage("evaluate-plan");
    const std::vector<ShapeRecord> parts = evaluate_plan(
        plan, cache_root, cache_transaction ? &*cache_transaction : nullptr, context);

    fs::create_directories(out_dir);
    const fs::path step_path = out_dir / "model.step";
    const fs::path stl_path = out_dir / "model.stl";
    const fs::path topology_path = out_dir / "topology.json";
    const fs::path analysis_boundary_path = out_dir / "analysis-boundary.json";
    const fs::path stage_report_path = out_dir / "stage-report.json";

    const bool mesh_only = std::all_of(parts.begin(), parts.end(), [](const ShapeRecord& part) {
        return part.kind == ShapeRecord::Kind::Manifold;
    });

    if (!mesh_only) {
        context.set_stage("assemble-export-shape");
        std::vector<TopoDS_Shape> export_shapes;
        export_shapes.reserve(parts.size());
        for (const ShapeRecord& part : parts) {
            if (part.kind == ShapeRecord::Kind::Manifold) {
                export_shapes.push_back(tessellated_step_shape_from_manifold(part.manifold));
                std::lock_guard<std::mutex> lock(context.mutex);
                ++context.tessellated_step_part_count;
            } else {
                export_shapes.push_back(part.shape);
            }
        }
        run_occt_export_stage("write-step", [&]() {
            StageExecutionTimer stage_timer(context, "export");
            write_step_file(step_path, export_shapes);
        }, context);
    }

    // Every part contributes one immutable triangle stream. Analytic BRep parts
    // tessellate through the bounded/cacheable OCCT path; mesh-domain parts copy
    // their canonical Manifold triangles without a remesh or cross-part union.
    std::vector<ecky::PartMesh> part_meshes = build_part_meshes(
        plan, parts, cache_root, cache_transaction ? &*cache_transaction : nullptr, context);
    std::vector<const ecky::PartMesh*> preview_meshes;
    preview_meshes.reserve(part_meshes.size());
    for (const ecky::PartMesh& mesh : part_meshes) preview_meshes.push_back(&mesh);
    {
        std::lock_guard<std::mutex> lock(context.mutex);
        context.preview_facet_count = 0;
        for (const ecky::PartMesh& mesh : part_meshes) {
            context.preview_facet_count += mesh.triangles.size();
        }
    }
    run_occt_export_stage("write-model-stl", [&]() {
        StageExecutionTimer stage_timer(context, "export");
        write_part_mesh_stl_file(stl_path, preview_meshes);
    }, context);
// Write per-part binary STL files so multipart export (3MF / zip) has
// distinct geometry per part instead of duplicating the merged mesh. Same
// adaptive tessellation policy as the merged preview/export path.
    if (parts.size() > 1) {
        const fs::path parts_dir = out_dir / "parts";
        fs::create_directories(parts_dir);
        for (std::size_t i = 0; i < parts.size(); ++i) {
            std::string name = parts[i].part_id;
            if (name.empty()) {
                name = parts[i].label;
            }
            if (name.empty()) {
                name = "part_" + std::to_string(i);
            }
            const fs::path part_stl = parts_dir / (name + ".stl");
            run_occt_export_stage("write-part-stl:" + name, [&]() {
                StageExecutionTimer stage_timer(context, "export");
                write_part_mesh_stl_file(part_stl, {&part_meshes[i]});
            }, context);
        }
    }
    context.set_stage("write-topology");
    const std::map<std::string, std::string> source_geometry_digests =
        write_topology_report(
            topology_path,
            plan,
            parts,
            kStlLinearDeflection,
            stl_angular_deflection_for_linear(kStlLinearDeflection),
            cache_root,
            cache_transaction ? &*cache_transaction : nullptr,
            context);
    context.set_stage("write-analysis-boundary");
    write_topology_report(
        analysis_boundary_path,
        plan,
        parts,
        kAnalysisBoundaryLinearDeflection,
        kAnalysisBoundaryAngularDeflection,
        cache_root,
        cache_transaction ? &*cache_transaction : nullptr,
        context,
        source_geometry_digests);
    context.set_stage("write-stage-report");
    write_stage_report(stage_report_path, context);
    if (cache_transaction.has_value()) {
        cache_transaction->commit();
        evict_cache_to_budget(*cache_root);
    }
    context.set_stage("complete");
    return 0;
}

}  // namespace

int main(int argc, char** argv) {
    ExecutionContext context;
    try {
        return run(argc, argv, context);
    } catch (const ParseError& error) {
        write_error_json("parse_error", "parse_failed", error.what(), error.what());
        return 1;
    } catch (const SchemaError& error) {
        write_error_json("schema_error", "schema_mismatch", error.what(), error.what());
        return 2;
    } catch (const EvalError& error) {
        std::string message = error.what();
        std::string code = "validation_failed";
        if (message.find("unsupported direct OCCT op `") != std::string::npos) {
            code = "unsupported_op";
        } else if (message.find("supports exact `target-id:` / `target-ids:` selectors only") !=
                       std::string::npos ||
                   message.find("got unsupported selector payload") != std::string::npos ||
                   message.find("does not recognize `:") != std::string::npos ||
                   message.find("keywords unsupported yet") != std::string::npos) {
            code = "unsupported_selector_form";
        }
        write_error_json("validation_error", code, message, message);
        return 3;
    } catch (const IoError& error) {
        write_error_json("io_error", "io_failed", error.what(), error.what());
        return 4;
    } catch (const OcctRuntimeError& error) {
        write_error_json("runtime_error", "occt_command_failed", error.what(), error.what());
        return 5;
    } catch (const StdFail_NotDone& error) {
        const char* raw_message = error.GetMessageString();
        std::string message = "Direct OCCT stage `" + context.stage() +
            "` failed: " + (raw_message ? raw_message : "unknown OCCT failure");
        write_error_json("runtime_error", "occt_not_done", message, message);
        return 5;
    } catch (const Standard_Failure& error) {
        const char* raw_message = error.GetMessageString();
        std::string message = "Direct OCCT stage `" + context.stage() +
            "` failed: " + (raw_message ? raw_message : "unknown OCCT failure");
        write_error_json("runtime_error", "occt_failure", message, message);
        return 5;
    } catch (const std::exception& error) {
        write_error_json("internal_error", "internal_failure", error.what(), error.what());
        return 10;
    }
}
