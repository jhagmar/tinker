"""Python kit tests: golden round-trip, int-sum, log, encode."""

from __future__ import annotations

import math
import unittest
from pathlib import Path

from tinker_kit import (
    MAX_BYTES,
    MAX_DEPTH,
    KitError,
    decode,
    decode_problem,
    encode,
    log,
    take_logs,
)

GOLDENS = Path(__file__).resolve().parent.parent / "goldens"

ROUND_TRIP = (
    "null",
    "true",
    "false",
    "string",
    "string-escape",
    "ctrl",
    "empty-array",
    "empty-object",
    "float",
    "float-zero",
    "float-neg-zero",
    "float-two",
    "int-safe",
    "int-neg",
    "int-wide",
    "int-sum-instance",
    "int-sum-instance-wide",
    "int-sum-answer",
    "int-sum-answer-wide",
    "nested",
    "set-as-array",
)


def golden(name: str) -> str:
    return (GOLDENS / f"{name}.json").read_text(encoding="utf-8").strip()


class KitTests(unittest.TestCase):
    def test_goldens_round_trip(self) -> None:
        for name in ROUND_TRIP:
            src = golden(name)
            with self.subTest(name=name):
                self.assertEqual(encode(decode(src)), src)

    def test_int_sum_field_access_and_wide_int(self) -> None:
        problem = decode_problem(golden("int-sum-instance"))
        self.assertEqual(problem.v, [1, 2, 3])
        self.assertEqual(encode(sum(problem.v)), golden("int-sum-answer"))
        wide = decode_problem(golden("int-sum-instance-wide"))
        self.assertEqual(wide.v[0], 1)
        self.assertEqual(wide.v[1], 2**53)
        self.assertEqual(encode(sum(wide.v)), golden("int-sum-answer-wide"))
        with self.assertRaises(AttributeError):
            _ = problem.missing

    def test_log_and_set_and_tuple(self) -> None:
        take_logs()
        self.assertEqual(log(6), "6")
        self.assertEqual(take_logs(), ["6"])
        self.assertEqual(encode({3, 1, 2}), golden("set-as-array"))
        self.assertEqual(encode((1, 2, 3)), golden("set-as-array"))

    def test_errors(self) -> None:
        with self.assertRaises(KitError):
            decode("")
        with self.assertRaises(KitError):
            decode("   ")
        with self.assertRaises(KitError):
            decode("null x")
        with self.assertRaises(KitError):
            decode("9007199254740992")
        with self.assertRaises(KitError):
            decode('{"a":1,"a":2}')
        with self.assertRaises(KitError):
            decode('{"$i":1}')
        with self.assertRaises(KitError):
            decode('{"$i":"01"}')
        with self.assertRaises(KitError):
            decode("1e999")
        with self.assertRaises(KitError):
            decode_problem("[]")
        with self.assertRaises(KitError):
            encode({1: 2})
        with self.assertRaises(KitError):
            encode(math.inf)
        with self.assertRaises(KitError):
            encode(object())
        with self.assertRaises(KitError):
            encode("a" * (MAX_BYTES + 1))
        with self.assertRaises(KitError):
            decode("a" * (MAX_BYTES + 1))
        deep = "[" * (MAX_DEPTH + 1) + "]" * (MAX_DEPTH + 1)
        with self.assertRaises(KitError):
            decode(deep)

    def test_tagged_and_bool_before_int(self) -> None:
        self.assertEqual(decode('{"$i":"9007199254740992"}'), 2**53)
        self.assertEqual(encode(True), "true")
        self.assertEqual(encode(False), "false")
        self.assertEqual(encode(None), "null")
        self.assertEqual(encode(-0.0), "-0.0")
        self.assertEqual(encode(2.0), "2.0")
        self.assertEqual(encode("hello"), golden("string"))
        self.assertEqual(encode("\u0001"), golden("ctrl"))
        with self.assertRaises(KitError):
            decode('{"$i":"-"}')
        with self.assertRaises(KitError):
            decode('{"$i":"1","x":2}')


if __name__ == "__main__":
    unittest.main()
