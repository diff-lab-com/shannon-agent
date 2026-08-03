"""Python fixture: small ETL helpers, used by the multi-language walk test."""

from dataclasses import dataclass
from typing import Iterable


@dataclass
class Record:
    id: int
    value: float


class Pipeline:
    def __init__(self, name: str):
        self.name = name
        self._records: list[Record] = []

    def ingest(self, rows: Iterable[Record]) -> None:
        self._records.extend(rows)

    def total(self) -> float:
        return sum(r.value for r in self._records)


def transform(records: list[Record]) -> list[Record]:
    return [Record(r.id, r.value * 2.0) for r in records]


def main() -> None:
    pipe = Pipeline("daily")
    pipe.ingest([Record(1, 1.0), Record(2, 2.5)])
    print(pipe.total())
