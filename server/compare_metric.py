import struct
import sys
import json
from dataclasses import dataclass
from pathlib import Path


@dataclass
class MeshMetrics:
    triangle_count: int
    volume: float
    surface_area: float
    min_bound: tuple[float, float, float]
    max_bound: tuple[float, float, float]


def _binary_triangle_count(data: bytes) -> int | None:
    if len(data) < 84:
        return None
    count = struct.unpack("<I", data[80:84])[0]
    expected_size = 84 + count * 50
    if expected_size == len(data):
        return count
    return None


def _parse_binary_stl(data: bytes) -> list[tuple[tuple[float, float, float], ...]]:
    count = _binary_triangle_count(data)
    if count is None:
        raise ValueError("not a binary STL")
    triangles = []
    offset = 84
    for _ in range(count):
        record = data[offset : offset + 50]
        if len(record) != 50:
            raise ValueError("truncated binary STL triangle record")
        coords = struct.unpack("<12fH", record)
        v0 = (coords[3], coords[4], coords[5])
        v1 = (coords[6], coords[7], coords[8])
        v2 = (coords[9], coords[10], coords[11])
        triangles.append((v0, v1, v2))
        offset += 50
    return triangles


def _parse_ascii_stl(data: bytes) -> list[tuple[tuple[float, float, float], ...]]:
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise ValueError("not an ASCII STL") from exc

    vertices = []
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line.startswith("vertex "):
            continue
        parts = line.split()
        if len(parts) != 4:
            raise ValueError(f"malformed vertex line: {raw_line!r}")
        vertices.append((float(parts[1]), float(parts[2]), float(parts[3])))

    if len(vertices) % 3 != 0:
        raise ValueError("ASCII STL vertex count is not divisible by 3")

    return [
        (vertices[index], vertices[index + 1], vertices[index + 2])
        for index in range(0, len(vertices), 3)
    ]


def _load_triangles(path: Path) -> list[tuple[tuple[float, float, float], ...]]:
    data = path.read_bytes()
    binary_count = _binary_triangle_count(data)
    if binary_count is not None:
        return _parse_binary_stl(data)
    return _parse_ascii_stl(data)


def _triangle_volume(
    triangle: tuple[tuple[float, float, float], tuple[float, float, float], tuple[float, float, float]]
) -> float:
    v0, v1, v2 = triangle
    cx = v1[1] * v2[2] - v1[2] * v2[1]
    cy = v1[2] * v2[0] - v1[0] * v2[2]
    cz = v1[0] * v2[1] - v1[1] * v2[0]
    return (v0[0] * cx + v0[1] * cy + v0[2] * cz) / 6.0


def _triangle_area(
    triangle: tuple[tuple[float, float, float], tuple[float, float, float], tuple[float, float, float]]
) -> float:
    v0, v1, v2 = triangle
    ax = v1[0] - v0[0]
    ay = v1[1] - v0[1]
    az = v1[2] - v0[2]
    bx = v2[0] - v0[0]
    by = v2[1] - v0[1]
    bz = v2[2] - v0[2]
    cx = ay * bz - az * by
    cy = az * bx - ax * bz
    cz = ax * by - ay * bx
    return (cx * cx + cy * cy + cz * cz) ** 0.5 / 2.0


def mesh_metrics(path: Path) -> MeshMetrics:
    triangles = _load_triangles(path)
    if not triangles:
        raise ValueError("STL has no triangles")

    xs: list[float] = []
    ys: list[float] = []
    zs: list[float] = []
    signed_volume = 0.0
    surface_area = 0.0

    for triangle in triangles:
        signed_volume += _triangle_volume(triangle)
        surface_area += _triangle_area(triangle)
        for vx, vy, vz in triangle:
            xs.append(vx)
            ys.append(vy)
            zs.append(vz)

    return MeshMetrics(
        triangle_count=len(triangles),
        volume=abs(signed_volume),
        surface_area=surface_area,
        min_bound=(min(xs), min(ys), min(zs)),
        max_bound=(max(xs), max(ys), max(zs)),
    )


def _bounding_box_error(reference: MeshMetrics, generated: MeshMetrics) -> float:
    dx = abs(reference.min_bound[0] - generated.min_bound[0]) + abs(reference.max_bound[0] - generated.max_bound[0])
    dy = abs(reference.min_bound[1] - generated.min_bound[1]) + abs(reference.max_bound[1] - generated.max_bound[1])
    dz = abs(reference.min_bound[2] - generated.min_bound[2]) + abs(reference.max_bound[2] - generated.max_bound[2])
    return dx + dy + dz


def _bounding_box_axis_deltas(reference: MeshMetrics, generated: MeshMetrics) -> dict[str, float]:
    return {
        "x": abs(reference.min_bound[0] - generated.min_bound[0])
        + abs(reference.max_bound[0] - generated.max_bound[0]),
        "y": abs(reference.min_bound[1] - generated.min_bound[1])
        + abs(reference.max_bound[1] - generated.max_bound[1]),
        "z": abs(reference.min_bound[2] - generated.min_bound[2])
        + abs(reference.max_bound[2] - generated.max_bound[2]),
    }


def main() -> None:
    json_mode = False
    args = sys.argv[1:]
    if args and args[0] == "--json":
        json_mode = True
        args = args[1:]

    if len(args) != 2:
        print("Usage: compare_metric.py [--json] <ref.stl> <gen.stl>")
        sys.exit(1)

    ref_path = Path(args[0])
    gen_path = Path(args[1])

    try:
        ref_mesh = mesh_metrics(ref_path)
        gen_mesh = mesh_metrics(gen_path)
    except Exception as exc:
        print(f"Error loading STL mesh data: {exc}")
        sys.exit(1)

    vol_diff = abs(ref_mesh.volume - gen_mesh.volume) / ref_mesh.volume * 100 if ref_mesh.volume else float("inf")
    bb_diff = _bounding_box_error(ref_mesh, gen_mesh)
    bb_axis = _bounding_box_axis_deltas(ref_mesh, gen_mesh)

    if vol_diff < 5.0 and bb_diff < 10.0:
        status = "EXCELLENT MATCH"
    elif vol_diff < 20.0 and bb_diff < 30.0:
        status = "GOOD MATCH"
    else:
        status = "POOR MATCH"

    if json_mode:
        print(
            json.dumps(
                {
                    "reference_volume": ref_mesh.volume,
                    "generated_volume": gen_mesh.volume,
                    "reference_surface_area": ref_mesh.surface_area,
                    "generated_surface_area": gen_mesh.surface_area,
                    "reference_triangle_count": ref_mesh.triangle_count,
                    "generated_triangle_count": gen_mesh.triangle_count,
                    "volume_difference_percent": vol_diff,
                    "bounding_box_match_error": bb_diff,
                    "bounding_box_axis_deltas": bb_axis,
                    "status": status,
                }
            )
        )
        return

    print("--- Metric Comparison ---")
    print(f"Reference Volume: {ref_mesh.volume:.2f} mm^3")
    print(f"Generated Volume: {gen_mesh.volume:.2f} mm^3")
    print(f"Reference Surface Area: {ref_mesh.surface_area:.2f} mm^2")
    print(f"Generated Surface Area: {gen_mesh.surface_area:.2f} mm^2")
    print(f"Reference Triangles: {ref_mesh.triangle_count}")
    print(f"Generated Triangles: {gen_mesh.triangle_count}")
    print(f"Volume Difference: {vol_diff:.2f}%")
    print(f"Bounding Box Match Error: {bb_diff:.2f} mm")
    print(
        "Bounding Box Axis Deltas: "
        f"x={bb_axis['x']:.2f} mm "
        f"y={bb_axis['y']:.2f} mm "
        f"z={bb_axis['z']:.2f} mm"
    )
    print(f"Status: {status}")


if __name__ == "__main__":
    main()
