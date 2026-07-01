#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepClass_FaceClassifier.hxx>
#include <BRepAdaptor_Curve.hxx>
#include <BRepAdaptor_Surface.hxx>
#include <BRepBuilderAPI_GTransform.hxx>
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
#include <BRep_Builder.hxx>
#include <BRepBuilderAPI_MakeSolid.hxx>
#include <BRepBuilderAPI_Sewing.hxx>
#include <BRepBuilderAPI_TransitionMode.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepLib.hxx>
#include <ShapeFix_Shape.hxx>
#include <BOPAlgo_Builder.hxx>
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
#include <GeomAPI_PointsToBSpline.hxx>
#include <GProp_GProps.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Poly_Triangulation.hxx>
#include <StlAPI_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <Standard_Failure.hxx>
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
#include <TopoDS_Shape.hxx>
#include <TopoDS_Wire.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_ListOfShape.hxx>
#include <algorithm>
#include <array>
#include <cctype>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <map>
#include <memory>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <variant>
#include <vector>
#include <gp_Ax1.hxx>
#include <gp_Ax2.hxx>
#include <gp_Ax3.hxx>
#include <gp_Circ.hxx>
#include <gp_Elips.hxx>
#include <gp_Dir.hxx>
#include <gp_GTrsf.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Pnt.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>
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

struct Part {
    std::string part_id;
    std::string label;
    std::uint64_t root = 0;
    std::vector<Command> commands;
};

struct Plan {
    std::uint32_t schema_version = 0;
    std::string plan_id;
    std::vector<Part> parts;
};

struct ShapeRecord {
    std::string part_id;
    std::string label;
    TopoDS_Shape shape;
};

struct SlotValue {
    enum class Kind { Shape, Frame };

    Kind kind = Kind::Shape;
    TopoDS_Shape shape;
    gp_Trsf frame;

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

struct IoError : std::runtime_error {
    using std::runtime_error::runtime_error;
};

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
    yyjson_val* commands = json_array(json_require(value, "commands"), "commands");
    size_t command_index;
    size_t command_max;
    yyjson_val* command;
    yyjson_arr_foreach(commands, command_index, command_max, command) {
        part.commands.push_back(parse_command(command));
    }
    return part;
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

void write_part_topology(
    std::ostream& out,
    const std::string& part_id,
    const std::string& label,
    const TopoDS_Shape& shape,
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
    out << ",\"edges\":[";

    bool first_edge = true;
    TopTools_IndexedMapOfShape edge_map;
    TopExp::MapShapes(shape, TopAbs_EDGE, edge_map);
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
            out << "}";
        } catch (...) {
        }
    }

    out << "],\"faces\":[";
    bool first_face = true;
    int face_index = 0;
    for (TopExp_Explorer explorer(shape, TopAbs_FACE); explorer.More(); explorer.Next(), ++face_index) {
        try {
            TopoDS_Face face = TopoDS::Face(explorer.Current());
            GProp_GProps props;
            BRepGProp::SurfaceProperties(face, props);
            gp_Pnt center = props.CentreOfMass();
            double area = props.Mass();

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
            out << quote_json_string(face_target_id(part_id, face_index, face));
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
            out << "}";
        } catch (...) {
        }
    }

    out << "]}";
}

void write_topology_report(const fs::path& topology_path, const std::vector<ShapeRecord>& parts) {
    std::ofstream out(topology_path);
    if (!out) {
        throw IoError("failed to open topology file");
    }
    out << "{\"parts\":[";
    bool first_part = true;
    for (const auto& part : parts) {
        write_part_topology(out, part.part_id, part.label, part.shape, first_part);
    }
    out << "]}";
    if (!out.good()) {
        throw IoError("failed to write topology file");
    }
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

std::array<double, 3> require_point3_arg(const Arg& arg, const std::string& label);

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
            args.origin = require_point3_arg(keyword.value, "plane :origin");
            continue;
        }
        if (keyword.name == "x") {
            args.x_axis = require_point3_arg(keyword.value, "plane :x");
            continue;
        }
        if (keyword.name == "normal") {
            args.normal = require_point3_arg(keyword.value, "plane :normal");
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
                        // 3. Every returned face is material (holes are inner
                        //    wires already). Adjacent regions share seam edges
                        //    from the arrangement; unify them into one face so
                        //    the per-region prisms don't fight over coincident
                        //    walls in later booleans. Unify (not fuse!) keeps
                        //    hole rings intact.
                        ShapeUpgrade_UnifySameDomain unify(
                            faces_shape, Standard_True, Standard_True, Standard_False);
                        unify.Build();
                        TopoDS_Shape unified = unify.Shape();
                        if (unified.IsNull()) {
                            unified = faces_shape;
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
            FacedWire entry;
            entry.wire = wire;
            entry.face = face;
            entry.area = face_area(face);
            faced.push_back(entry);
        } catch (...) {
            continue;
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
    wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt(x0 + r, y0, 0), gp_Pnt(x1 - r, y0, 0)).Edge());
    wire_builder.Add(BRepBuilderAPI_MakeEdge(
                         GC_MakeArcOfCircle(gp_Pnt(x1 - r, y0, 0),
                                            gp_Pnt(x1 - r + arc_mid, y0 + r - arc_mid, 0),
                                            gp_Pnt(x1, y0 + r, 0))
                             .Value())
                         .Edge());
    wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt(x1, y0 + r, 0), gp_Pnt(x1, y1 - r, 0)).Edge());
    wire_builder.Add(BRepBuilderAPI_MakeEdge(
                         GC_MakeArcOfCircle(gp_Pnt(x1, y1 - r, 0),
                                            gp_Pnt(x1 - r + arc_mid, y1 - r + arc_mid, 0),
                                            gp_Pnt(x1 - r, y1, 0))
                             .Value())
                         .Edge());
    wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt(x1 - r, y1, 0), gp_Pnt(x0 + r, y1, 0)).Edge());
    wire_builder.Add(BRepBuilderAPI_MakeEdge(
                         GC_MakeArcOfCircle(gp_Pnt(x0 + r, y1, 0),
                                            gp_Pnt(x0 + r - arc_mid, y1 - r + arc_mid, 0),
                                            gp_Pnt(x0, y1 - r, 0))
                             .Value())
                         .Edge());
    wire_builder.Add(BRepBuilderAPI_MakeEdge(gp_Pnt(x0, y1 - r, 0), gp_Pnt(x0, y0 + r, 0)).Edge());
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

TopoDS_Shape extrude_shape(const TopoDS_Shape& shape, double height) {
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

TopoDS_Shape sweep_shape(const TopoDS_Shape& profile, const TopoDS_Shape& path, bool frenet) {
    BRepOffsetAPI_MakePipeShell pipe(first_wire(path, "sweep"));
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
    pipe.SetMode(frenet ? Standard_True : Standard_False);
    pipe.SetTransitionMode(frenet ? BRepBuilderAPI_RightCorner : BRepBuilderAPI_Transformed);
    pipe.Add(first_wire(profile, "sweep"), Standard_False, Standard_False);
    pipe.Build();
    if (!pipe.IsDone()) {
        throw EvalError("sweep failed to build");
    }
    // MakeSolid caps the swept tube into a solid. Its boolean return is the
    // difference between "renders as a tube" and "can be clipped/cut": a helix
    // whose ends will not auto-cap leaves an open shell here, which later
    // BRepAlgoAPI_Common (clip-box) silently reduces to nothing.
    pipe.MakeSolid();
    TopoDS_Shape swept = pipe.Shape();
    // Defensive: if the pipe-shell did not cap into a solid, sew + close it so
    // downstream booleans have a solid to operate on.
    if (!shape_has_solid(swept)) {
        swept = solidify_swept_shell(swept);
    }
    if (!shape_has_solid(swept)) {
        throw EvalError("sweep did not produce a closed solid");
    }
    // Heal an invalid (self-intersecting / out-of-tolerance) swept solid so it
    // can be intersected and subtracted like any other solid.
    if (!BRepCheck_Analyzer(swept).IsValid()) {
        ShapeFix_Shape fixer(swept);
        fixer.Perform();
        TopoDS_Shape fixed = fixer.Shape();
        if (shape_has_solid(fixed)) {
            swept = fixed;
        }
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

TopoDS_Shape fuse_shapes(const TopoDS_Shape& lhs, const TopoDS_Shape& rhs) {
    return BRepAlgoAPI_Fuse(lhs, rhs).Shape();
}

TopoDS_Shape cut_shapes(const TopoDS_Shape& lhs, const TopoDS_Shape& rhs) {
    return BRepAlgoAPI_Cut(lhs, rhs).Shape();
}

TopoDS_Shape common_shapes(const TopoDS_Shape& lhs, const TopoDS_Shape& rhs) {
    return BRepAlgoAPI_Common(lhs, rhs).Shape();
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
    BRepFilletAPI_MakeFillet fillet(shape);
    auto add_edge = [&](const TopoDS_Edge& edge) {
        if (radius2.has_value()) {
            fillet.Add(radius, *radius2, edge);
        } else {
            fillet.Add(radius, edge);
        }
    };
    if (!selector.has_value()) {
        TopTools_IndexedMapOfShape edge_map;
        TopExp::MapShapes(shape, TopAbs_EDGE, edge_map);
        int edge_count = 0;
        for (int edge_ordinal = 1; edge_ordinal <= edge_map.Extent(); ++edge_ordinal) {
            add_edge(TopoDS::Edge(edge_map.FindKey(edge_ordinal)));
            ++edge_count;
        }
        if (edge_count == 0) {
            throw EvalError("fillet found no edges");
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
            add_edge(TopoDS::Edge(edge_map.FindKey(edge_ordinal)));
        }
    }
    return fillet.Shape();
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

TopoDS_Shape make_helix_path_wire(double radius, double pitch, double height, bool lefthand) {
    if (!std::isfinite(radius) || radius <= 0.0) {
        throw EvalError("helix-path radius must be positive");
    }
    if (!std::isfinite(pitch) || pitch <= 0.0) {
        throw EvalError("helix-path pitch must be positive");
    }
    if (!std::isfinite(height) || height <= 0.0) {
        throw EvalError("helix-path height must be positive");
    }
    const double turns = height / pitch;
    const double end_angle = (lefthand ? -1.0 : 1.0) * 6.28318530717958647692 * turns;
    Handle(Geom_CylindricalSurface) surface =
        new Geom_CylindricalSurface(gp_Ax3(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), radius);
    Handle(Geom2d_TrimmedCurve) curve2d =
        GCE2d_MakeSegment(gp_Pnt2d(0, 0), gp_Pnt2d(end_angle, height)).Value();
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
    // `dynamic_cast` and C++ catch-by-type dispatch across the whole runner
    // (gp_VectorWithNullMagnitude thrown by OCCT booleans then escapes every
    // `catch (const Standard_Failure&)`). Linear edges carry their typeinfo in
    // the OCCT dylib and are already what every other op here produces, so the
    // wire-soup arrangement and downstream booleans stay stable. The	n    // exact-curve parity feature must wait for the runner to be built inside
    // OCCT's visibility scope (see openspec/changes/svg-native-exact-curves).
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

SlotValue evaluate_command(
    const Command& command,
    const std::map<std::uint64_t, SlotValue>& slots,
    const std::string& part_id
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

    if (!command.keywords.empty() && op != "box" && op != "sphere" && op != "cylinder" &&
        op != "cone" && op != "torus" && op != "wedge" && op != "profile" && op != "plane" &&
        op != "clip-box" && op != "fillet" && op != "chamfer" && op != "shell" && op != "bspline" &&
        op != "sweep" && op != "draft") {
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
        if (!reader.Read(shape, command.args[0].text_value.c_str())) {
            throw EvalError(op + " could not read STL file");
        }
        return shape;
    }
    if (op == "extrude") {
        return extrude_shape(get_ref_shape(0), require_number_arg(command.args, 1, op));
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
                                    require_bool_arg(command.args, 3, op));
    }
    if (op == "bezier-path") {
        return make_bezier_path_wire(require_point3_sequence(command.args, op));
    }
    if (op == "plane") {
        return make_plane_frame(plane_args(command));
    }
    if (op == "location") {
        if (command.args.empty()) {
            return make_location_frame(nullptr);
        }
        if (command.args.size() == 1) {
            const gp_Trsf& frame = get_ref_frame(0);
            return make_location_frame(&frame);
        }
        throw EvalError(op + " expects zero or one frame reference");
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
        TopoDS_Shape result = lookup_shape(slots, refs.front().ref_value, op);
        for (std::size_t index = 1; index < refs.size(); ++index) {
            const TopoDS_Shape& next = lookup_shape(slots, refs[index].ref_value, op);
            if (op == "union") {
                result = fuse_shapes(result, next);
            } else if (op == "difference") {
                result = cut_shapes(result, next);
            } else {
                result = common_shapes(result, next);
            }
        }
        return result;
    }
    if (op == "translate") {
        return translate_shape(get_ref_shape(3), require_number_arg(command.args, 0, op),
                               require_number_arg(command.args, 1, op),
                               require_number_arg(command.args, 2, op));
    }
    if (op == "rotate") {
        return rotate_shape(get_ref_shape(3), require_number_arg(command.args, 0, op),
                            require_number_arg(command.args, 1, op),
                            require_number_arg(command.args, 2, op));
    }
    if (op == "scale") {
        if (command.args.size() == 2) {
            double factor = require_number_arg(command.args, 0, op);
            return scale_shape(get_ref_shape(1), factor, factor, factor);
        }
        if (command.args.size() == 3) {
            return scale_shape(get_ref_shape(2), require_number_arg(command.args, 0, op),
                               require_number_arg(command.args, 1, op), 1.0);
        }
        if (command.args.size() == 4) {
            return scale_shape(get_ref_shape(3), require_number_arg(command.args, 0, op),
                               require_number_arg(command.args, 1, op),
                               require_number_arg(command.args, 2, op));
        }
        throw EvalError(op + " expects one to three factors and a shape");
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

std::vector<ShapeRecord> evaluate_plan(const Plan& plan) {
    if (plan.parts.empty()) {
        throw EvalError("plan needs at least one part");
    }

    std::vector<ShapeRecord> parts;
    for (const Part& part : plan.parts) {
        std::map<std::uint64_t, SlotValue> slots;
        for (const Command& command : part.commands) {
            SlotValue value = evaluate_command(command, slots, part.part_id);
            slots[command.output] = value;
        }
        auto root = slots.find(part.root);
        if (root == slots.end()) {
            throw EvalError("missing root shape for part `" + part.part_id + "`");
        }
        if (root->second.kind != SlotValue::Kind::Shape) {
            throw EvalError("root slot for part `" + part.part_id + "` is not a shape");
        }
        parts.push_back(ShapeRecord{part.part_id, part.label, root->second.shape});
    }
    return parts;
}

void write_step_file(const fs::path& path, const TopoDS_Shape& shape) {
    STEPControl_Writer writer;
    writer.Transfer(shape, STEPControl_AsIs);
    if (writer.Write(path.string().c_str()) != IFSelect_RetDone) {
        throw IoError("failed to write STEP");
    }
}

// Preview/export STL tessellation quality. The previous 0.2 mm linear-only
// deflection left visibly faceted cylinders (coarse on print). Use a finer
// linear deflection plus an angular deflection so curved surfaces (cylinders,
// fillets, lofts) stay smooth regardless of radius.
static constexpr double kStlLinearDeflection = 0.04;   // mm chord error
static constexpr double kStlAngularDeflection = 0.25;  // rad (~14 deg) per facet

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

void write_stl_file(const fs::path& path, const TopoDS_Shape& shape) {
    BRepMesh_IncrementalMesh mesh(
        shape, kStlLinearDeflection, Standard_False, kStlAngularDeflection, Standard_True);
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
        const Poly_Array1OfTriangle& tri_arr = triangulation->Triangles();
        for (Standard_Integer triangle_index = tri_arr.Lower(); triangle_index <= tri_arr.Upper();
             ++triangle_index) {
            Standard_Integer n1 = 0;
            Standard_Integer n2 = 0;
            Standard_Integer n3 = 0;
            tri_arr(triangle_index).Get(n1, n2, n3);
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

int run(int argc, char** argv) {
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

    std::string plan_text = read_text_file(plan_path);
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
    const std::vector<ShapeRecord> parts = evaluate_plan(plan);

    fs::create_directories(out_dir);
    const fs::path step_path = out_dir / "model.step";
    const fs::path stl_path = out_dir / "preview.stl";
    const fs::path topology_path = out_dir / "topology.json";

    TopoDS_Shape export_shape = parts.size() == 1 ? parts.front().shape : compound_shapes([&]() {
        std::vector<TopoDS_Shape> shapes;
        shapes.reserve(parts.size());
        for (const auto& part : parts) {
            shapes.push_back(part.shape);
        }
        return shapes;
    }());

    write_step_file(step_path, export_shape);
    write_stl_file(stl_path, export_shape);
    // Write per-part binary STL files so multipart export (3MF / zip) has
    // distinct geometry per part instead of duplicating the merged mesh.
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
            write_stl_file(part_stl, parts[i].shape);
        }
    }
    write_topology_report(topology_path, parts);
    return 0;
}

}  // namespace

int main(int argc, char** argv) {
    try {
        return run(argc, argv);
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
    } catch (const StdFail_NotDone& error) {
        std::string message = error.GetMessageString();
        write_error_json("runtime_error", "occt_not_done", message, message);
        return 5;
    } catch (const Standard_Failure& error) {
        std::string message = error.GetMessageString();
        write_error_json("runtime_error", "occt_failure", message, message);
        return 5;
    } catch (const std::exception& error) {
        write_error_json("internal_error", "internal_failure", error.what(), error.what());
        return 10;
    }
}
