from __future__ import annotations

import argparse
from typing import Callable, Protocol

from tools.jev.client import JevClient
from tools.jev.report import Report


class Check(Protocol):
    def add_arguments(self, parser: argparse.ArgumentParser) -> None: ...

    def run(self, args: argparse.Namespace, client: JevClient) -> Report: ...


CHECKS: dict[str, Check] = {}


def register(name: str) -> Callable[[Check], Check]:
    def decorator(check: Check) -> Check:
        registered = CHECKS.get(name)
        if registered is not None and registered is not check:
            raise ValueError(f"check {name} is already registered")
        CHECKS[name] = check
        return check

    return decorator
