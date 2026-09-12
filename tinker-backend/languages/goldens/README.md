# Goldens

Compact JSON fixtures for the problem / answer / `log` data model. Rust (`tinker-catalog`) and the Python kit round-trip the same files.

Integer values whose magnitude is greater than `2^53 - 1` use `{"$i":"<decimal>"}`. Sets encode as arrays.
