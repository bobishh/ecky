import os
import traceback
import sys
import json


def run(macro_path, stl_path, params_dict):
    import FreeCAD as App
    import Mesh

    doc = App.newDocument("EckyCAD")

    with open(macro_path, "r", encoding="utf-8") as f:
        code = f.read()

    # Inject params into the execution namespace
    # This allows the macro to check if a variable exists or use these values
    namespace = {"__name__": "__main__", "parameters": params_dict, "params": params_dict}
    
    # Also inject individual keys for convenience
    for k, v in params_dict.items():
        namespace[k] = v

    exec(compile(code, macro_path, "exec"), namespace, namespace)

    if App.ActiveDocument is None:
        App.setActiveDocument(doc.Name)

    active = App.ActiveDocument
    active.recompute()

    exportable = []
    for obj in active.Objects:
        shape = getattr(obj, "Shape", None)
        if shape is None:
            continue
        try:
            if shape.isNull() or shape.Volume <= 0:
                continue
        except Exception:
            continue
        exportable.append(obj)

    if not exportable:
        raise RuntimeError("No solid objects found to export.")

    Mesh.export(exportable, stl_path)


def main():
    print("RUNNER STARTED")
    macro_path = os.environ.get("ECKYCAD_MACRO")
    stl_path = os.environ.get("ECKYCAD_STL")
    params_str = os.environ.get("ECKYCAD_PARAMS", "{}")

    print(f"MACRO: {macro_path}")
    print(f"STL: {stl_path}")
    
    if not macro_path or not stl_path:
        print("Missing ECKYCAD_MACRO or ECKYCAD_STL environment variables.")
        sys.exit(1)

    try:
        params_dict = json.loads(params_str)
        print("Running macro execution...")
        run(macro_path, stl_path, params_dict)
        print("Macro execution finished successfully.")
    except Exception as e:
        print(f"FATAL ERROR: {e}")
        traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__" or __name__ == "freecad_runner":
    main()
