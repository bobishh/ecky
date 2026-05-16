from pathlib import Path
import importlib.util


ROOT = Path(__file__).resolve().parents[1]
HELPERS = ROOT / "server" / "_ecky_build123d_helpers.py"


def load_helpers():
    spec = importlib.util.spec_from_file_location("ecky_build123d_helpers", HELPERS)
    module = importlib.util.module_from_spec(spec)
    assert spec is not None and spec.loader is not None
    spec.loader.exec_module(module)
    return module


def expect_value_error(fn, expected):
    try:
        fn()
    except ValueError as err:
        message = str(err)
        assert expected in message, message
        return
    raise AssertionError("expected ValueError")


def main():
    helpers = load_helpers()

    assert helpers._ecky_selector_target_ids(None) is None
    assert helpers._ecky_selector_clauses(None) == []

    assert helpers._ecky_selector_target_ids(
        {"kind": "targetIds", "targetIds": ["body:edge:0:a_b"]}
    ) == ["body:edge:0:a_b"]
    assert helpers._ecky_face_selector_target_ids(
        {"kind": "targetIds", "targetIds": ["body:face:0:a:1"]}
    ) == ["body:face:0:a:1"]
    assert helpers._ecky_selector_clauses(
        {
            "kind": "clauses",
            "clauses": [
                {"kind": "boundary", "axis": "x", "bound": "min"},
                {"kind": "axis", "axis": "z"},
            ],
        }
    ) == [("boundary", "x", "min"), ("axis", "z")]
    assert helpers._ecky_face_selector_clauses(
        {
            "kind": "clauses",
            "clauses": [
                {"kind": "boundary", "axis": "z", "bound": "max"},
            ],
        }
    ) == [("boundary", "z", "max")]
    assert helpers._ecky_face_selector_clauses(
        {
            "kind": "clauses",
            "clauses": [
                {"kind": "planar"},
                {"kind": "normal", "axis": "z"},
                {"kind": "area", "rank": "max"},
            ],
        }
    ) == [("planar",), ("normal", "z"), ("area", "max")]

    expect_value_error(
        lambda: helpers._ecky_selector_target_ids("target-id:body:edge:0:a_b"),
        "requires typed selector payload",
    )
    expect_value_error(
        lambda: helpers._ecky_selector_clauses("left+vertical"),
        "requires typed selector payload",
    )
    expect_value_error(
        lambda: helpers._ecky_face_selector_clauses("top"),
        "requires typed selector payload",
    )

    box = helpers.Box(10, 10, 10)
    assert (
        len(
            helpers._ecky_select_shell_faces(
                box,
                {
                    "kind": "clauses",
                    "clauses": [
                        {"kind": "planar"},
                        {"kind": "normal", "axis": "z"},
                        {"kind": "area", "rank": "max"},
                    ],
                },
            )
        )
        == 2
    )
    assert (
        len(
            helpers._ecky_select_shell_faces(
                box,
                {
                    "kind": "clauses",
                    "clauses": [
                        {"kind": "planar"},
                        {"kind": "normal", "axis": "z"},
                        {"kind": "boundary", "axis": "z", "bound": "max"},
                    ],
                },
            )
        )
        == 1
    )

    print("Selector helpers OK")


if __name__ == "__main__":
    main()
