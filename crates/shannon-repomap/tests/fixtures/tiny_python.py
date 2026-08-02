"""Fixture: tiny_python.py

Smallest possible Python module with module-level function, class (with
methods), and a decorated function so we can assert the `decorated_definition`
unwrap path works.
"""

from __future__ import annotations


def add(a: int, b: int) -> int:
    return a + b


class Counter:
    """A tiny counter class with methods and a private helper."""

    def __init__(self, initial: int = 0) -> None:
        self._count = initial

    def increment(self) -> None:
        self._count += 1

    def value(self) -> int:
        return self._count

    def _double(self, n: int) -> int:
        return n * 2


@lru_cache(maxsize=128)
def cached_add(a: int, b: int) -> int:
    return add(a, b)
