from dataclasses import dataclass
from typing import Any, Dict, Iterable, List, Optional

CAD_SDK_VERSION = "0.1"


def number(
    key: str,
    default: float,
    min: Optional[float] = None,
    max: Optional[float] = None,
    step: Optional[float] = None,
    label: Optional[str] = None,
    section: Optional[str] = None,
    part: Optional[str] = None,
    help: Optional[str] = None,
) -> Dict[str, Any]:
    return _control(
        key,
        "number",
        default,
        label=label,
        min=min,
        max=max,
        step=step,
        section=section,
        part=part,
        help=help,
    )


def select(
    key: str,
    default: Any,
    options: Iterable[Any],
    label: Optional[str] = None,
    section: Optional[str] = None,
    part: Optional[str] = None,
    help: Optional[str] = None,
) -> Dict[str, Any]:
    return _control(
        key,
        "select",
        default,
        label=label,
        options=list(options),
        section=section,
        part=part,
        help=help,
    )


def toggle(
    key: str,
    default: bool,
    label: Optional[str] = None,
    section: Optional[str] = None,
    part: Optional[str] = None,
    help: Optional[str] = None,
) -> Dict[str, Any]:
    return _control(
        key,
        "toggle",
        bool(default),
        label=label,
        section=section,
        part=part,
        help=help,
    )


def image(
    key: str,
    default: str = "",
    label: Optional[str] = None,
    section: Optional[str] = None,
    part: Optional[str] = None,
    help: Optional[str] = None,
) -> Dict[str, Any]:
    return _control(
        key,
        "image",
        default,
        label=label,
        section=section,
        part=part,
        help=help,
    )


def _control(
    key: str,
    kind: str,
    default: Any,
    label: Optional[str] = None,
    min: Optional[float] = None,
    max: Optional[float] = None,
    step: Optional[float] = None,
    options: Optional[List[Any]] = None,
    section: Optional[str] = None,
    part: Optional[str] = None,
    help: Optional[str] = None,
) -> Dict[str, Any]:
    if not key or not isinstance(key, str):
        raise ValueError("Control key must be a non-empty string.")
    return {
        "key": key,
        "type": kind,
        "default": default,
        "label": label or key.replace("_", " ").title(),
        "min": min,
        "max": max,
        "step": step,
        "options": options,
        "section": section,
        "part": part,
        "help": help,
    }


@dataclass(frozen=True)
class ControlRegistry:
    controls: List[Dict[str, Any]]

    def __post_init__(self) -> None:
        keys = [control.get("key") for control in self.controls]
        if len(set(keys)) != len(keys):
            raise ValueError("Control keys must be unique.")

    def defaults(self) -> Dict[str, Any]:
        return {control["key"]: control.get("default") for control in self.controls}

    def bind(self, params: Dict[str, Any]) -> Dict[str, Any]:
        bound: Dict[str, Any] = {}
        for control in self.controls:
            key = control["key"]
            value = params.get(key, control.get("default"))
            kind = control.get("type")
            if kind == "number":
                value = _coerce_number(value, control.get("default"))
            elif kind == "toggle":
                value = bool(value)
            elif kind == "image":
                value = "" if value is None else str(value)
            bound[key] = value
        return bound

    def to_ui_spec(self) -> Dict[str, Any]:
        fields: List[Dict[str, Any]] = []
        for control in self.controls:
            kind = control.get("type")
            field = {
                "key": control["key"],
                "label": control.get("label", control["key"]),
            }
            if kind == "number":
                field["type"] = "number"
                if control.get("min") is not None:
                    field["min"] = float(control["min"])
                if control.get("max") is not None:
                    field["max"] = float(control["max"])
                if control.get("step") is not None:
                    field["step"] = float(control["step"])
            elif kind == "select":
                field["type"] = "select"
                field["options"] = _normalize_options(control.get("options"))
            elif kind == "toggle":
                field["type"] = "checkbox"
            elif kind == "image":
                field["type"] = "image"
            else:
                raise ValueError(f"Unknown control type: {kind}")
            fields.append(field)
        return {"fields": fields}


@dataclass
class BuildContext:
    doc: Any
    params: Dict[str, Any]
    registry: ControlRegistry
    config: Any


def _coerce_number(value: Any, fallback: Any) -> float:
    try:
        return float(value)
    except Exception:
        try:
            return float(fallback)
        except Exception:
            return 0.0


def _normalize_options(options: Optional[List[Any]]) -> List[Dict[str, Any]]:
    if not options:
        return []
    normalized = []
    for option in options:
        if isinstance(option, dict):
            label = option.get("label")
            value = option.get("value")
            if label is None:
                label = str(value)
            normalized.append({"label": str(label), "value": value})
        else:
            normalized.append({"label": str(option), "value": option})
    return normalized
