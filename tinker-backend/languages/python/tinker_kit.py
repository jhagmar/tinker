"""Protocol kit: JSON data model, `problem` field access, `log`, encode return."""

from __future__ import annotations

import json
import math
from typing import Any

MAX_SAFE_INT = 9_007_199_254_740_991
MAX_BYTES = 1_048_576
MAX_DEPTH = 64

_logs: list[str] = []


class KitError(Exception):
    """JSON kit failure."""


class Problem:
    """Decoded problem instance. Field access: ``problem.v``."""

    def __init__(self, obj: dict[str, Any]) -> None:
        if not isinstance(obj, dict):
            raise KitError("expected JSON object")
        object.__setattr__(self, "_obj", obj)

    def __getattr__(self, name: str) -> Any:
        try:
            return self._obj[name]
        except KeyError as exc:
            raise AttributeError(name) from exc


def decode_problem(text: str) -> Problem:
    """Decode a problem instance object."""
    return Problem(decode(text))


def decode(text: str) -> Any:
    """Decode one JSON value (tagged integers become ``int``)."""
    if len(text.encode("utf-8")) > MAX_BYTES:
        raise KitError("JSON exceeds 1 MiB")
    if text.strip() == "":
        raise KitError("empty JSON")

    def pairs(items: list[tuple[str, Any]]) -> dict[str, Any]:
        out: dict[str, Any] = {}
        for key, val in items:
            if key in out:
                raise KitError("duplicate JSON object key")
            out[key] = val
        return out

    def refuse(_const: str) -> None:
        raise KitError("non-finite JSON number")

    try:
        value = json.loads(text, parse_constant=refuse, object_pairs_hook=pairs)
    except json.JSONDecodeError as exc:
        raise KitError("invalid JSON") from exc
    lifted = _lift(value)
    _check_depth(lifted, 0)
    return lifted


def encode(value: Any) -> str:
    """Encode a native value (the ``solve`` return, or ``log`` payload)."""
    text = _dump(value)
    if len(text.encode("utf-8")) > MAX_BYTES:
        raise KitError("JSON exceeds 1 MiB")
    return text


def log(value: Any) -> str:
    """Encode ``value`` with the wire rules and record it for the harness."""
    text = encode(value)
    _logs.append(text)
    return text


def take_logs() -> list[str]:
    """Drain recorded ``log`` payloads."""
    out = list(_logs)
    _logs.clear()
    return out


def _lift(value: Any) -> Any:
    if value is None or isinstance(value, bool):
        return value
    if isinstance(value, int):
        if value < -MAX_SAFE_INT or value > MAX_SAFE_INT:
            raise KitError("integer outside 53 bits must use {$i}")
        return value
    if isinstance(value, float):
        if not math.isfinite(value):
            raise KitError("non-finite JSON number")
        return value
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return [_lift(item) for item in value]
    if isinstance(value, dict):
        if "$i" in value:
            if list(value.keys()) != ["$i"] or not isinstance(value["$i"], str):
                raise KitError("invalid {$i} integer tag")
            return _parse_decimal(value["$i"])
        return {key: _lift(item) for key, item in value.items()}
    raise KitError("invalid JSON")


def _parse_decimal(raw: str) -> int:
    neg = raw.startswith("-")
    digits = raw[1:] if neg else raw
    if digits == "" or not digits.isdigit():
        raise KitError("invalid integer decimal")
    if len(digits) > 1 and digits.startswith("0"):
        raise KitError("invalid integer decimal")
    return int(raw, 10)


def _check_depth(value: Any, depth: int) -> None:
    if isinstance(value, (list, dict)):
        if depth >= MAX_DEPTH:
            raise KitError("JSON nesting is too deep")
        child = depth + 1
        if isinstance(value, list):
            for item in value:
                _check_depth(item, child)
        else:
            for item in value.values():
                _check_depth(item, child)


def _dump(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        if -MAX_SAFE_INT <= value <= MAX_SAFE_INT:
            return str(value)
        return '{"$i":' + _encode_string(str(value)) + "}"
    if isinstance(value, float):
        return _encode_float(value)
    if isinstance(value, str):
        return _encode_string(value)
    if isinstance(value, (list, tuple)):
        return "[" + ",".join(_dump(item) for item in value) + "]"
    if isinstance(value, set):
        parts = sorted(_dump(item) for item in value)
        return "[" + ",".join(parts) + "]"
    if isinstance(value, dict):
        parts = []
        for key, item in value.items():
            if not isinstance(key, str):
                raise KitError("map keys must be strings")
            parts.append(_encode_string(key) + ":" + _dump(item))
        return "{" + ",".join(parts) + "}"
    raise KitError("cannot encode value")


def _encode_float(value: float) -> str:
    if not math.isfinite(value):
        raise KitError("non-finite JSON number")
    if value == 0.0:
        return "-0.0" if math.copysign(1.0, value) < 0 else "0.0"
    text = str(value)
    if "." not in text and "e" not in text and "E" not in text:
        text += ".0"
    return text


def _encode_string(value: str) -> str:
    out = ['"']
    for char in value:
        code = ord(char)
        if char == '"':
            out.append('\\"')
        elif char == "\\":
            out.append("\\\\")
        elif char == "\n":
            out.append("\\n")
        elif char == "\r":
            out.append("\\r")
        elif char == "\t":
            out.append("\\t")
        elif code < 0x20:
            out.append(f"\\u{code:04x}")
        else:
            out.append(char)
    out.append('"')
    return "".join(out)
