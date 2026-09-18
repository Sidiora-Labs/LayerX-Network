from __future__ import annotations

import hashlib
import json
import os
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Iterator, Mapping, Sequence, TypeVar


ENDPOINT_ENV = "JEV_ENDPOINT"
DEFAULT_ENDPOINT = "https://openrouter.ai/api/alpha/decisions"
API_KEY_ENV = "OPENROUTER_API_KEY"
MODEL_ENV = "JEV_MODEL"
DEFAULT_MODEL = "typesafe/jev-1.13"
MAX_CHOICE_OPTIONS = 255
MIN_SCORE_LEVELS = 2
RETRY_STATUSES = frozenset((429, 529))
BACKOFF_SECONDS = 0.5
EXCERPT_LIMIT = 512

T = TypeVar("T")


class JevError(RuntimeError):
    pass


class JevConfigError(JevError):
    pass


class JevDryRun(JevError):
    pass


class JevRequestError(JevError):
    def __init__(self, status: int, body_excerpt: str) -> None:
        super().__init__(f"jev request failed (status {status}): {body_excerpt}")
        self.status = status
        self.body_excerpt = body_excerpt


def noul(instructions: str, criteria: Mapping[str, str] | None = None) -> dict[str, object]:
    question: dict[str, object] = {"type": "noul", "instructions": instructions}
    if criteria is not None:
        question["criteria"] = dict(criteria)
    return question


def choice(instructions: str, criteria: Mapping[str, str]) -> dict[str, object]:
    options = dict(criteria)
    if len(options) < 2:
        raise ValueError("a choice question needs at least 2 options")
    if len(options) > MAX_CHOICE_OPTIONS:
        raise ValueError(f"a choice question allows at most {MAX_CHOICE_OPTIONS} options")
    return {"type": "choice", "instructions": instructions, "criteria": options}


def score(instructions: str, levels: Sequence[str]) -> dict[str, object]:
    ordered = list(levels)
    if len(ordered) < MIN_SCORE_LEVELS:
        raise ValueError(f"a score question needs at least {MIN_SCORE_LEVELS} levels")
    return {"type": "score", "instructions": instructions, "criteria": ordered}


@dataclass(frozen=True)
class Answer:
    kind: str
    value: float | str
    probabilities: Mapping[str, float]
    confidence: float
    legend: Mapping[str, str]


@dataclass(frozen=True)
class Evaluation:
    model: str
    requested_model: str
    answers: Mapping[str, Answer]
    usage: Mapping[str, float]
    request_sha256: str
    response_sha256: str
    request_id: str


def canonical_request(
    model: str,
    state: object,
    questions: Mapping[str, Mapping[str, object]],
) -> tuple[bytes, str]:
    body = {
        "model": model,
        "state": state,
        "questions": {name: dict(question) for name, question in questions.items()},
    }
    payload = json.dumps(body, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return payload, hashlib.sha256(payload).hexdigest()


def _number(name: str, value: object) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise JevRequestError(0, f"answer {name} has a non-numeric value {value!r}")
    return float(value)


def _probabilities(name: str, payload: Mapping[str, object]) -> dict[str, float]:
    raw = payload.get("probabilities")
    if not isinstance(raw, dict):
        return {}
    return {str(key): _number(name, value) for key, value in raw.items()}


def parse_answer(name: str, payload: Mapping[str, object]) -> Answer:
    kind = payload.get("type")
    if kind == "noul":
        probability = _number(name, payload.get("noul"))
        return Answer(
            kind="noul",
            value=probability,
            probabilities={"true": probability, "false": 1.0 - probability},
            confidence=max(probability, 1.0 - probability),
            legend={},
        )
    if kind == "choice":
        selected = payload.get("choice")
        if not isinstance(selected, str):
            raise JevRequestError(0, f"answer {name} has no choice value")
        return Answer(
            kind="choice",
            value=selected,
            probabilities=_probabilities(name, payload),
            confidence=_number(name, payload.get("confidence")),
            legend={},
        )
    if kind == "score":
        raw_legend = payload.get("legend")
        legend = (
            {str(key): str(value) for key, value in raw_legend.items()}
            if isinstance(raw_legend, dict)
            else {}
        )
        return Answer(
            kind="score",
            value=_number(name, payload.get("score")),
            probabilities=_probabilities(name, payload),
            confidence=_number(name, payload.get("confidence")),
            legend=legend,
        )
    raise JevRequestError(0, f"answer {name} has unsupported type {kind!r}")


def parse_evaluation(
    body: Mapping[str, object],
    requested_model: str,
    request_sha256: str,
    response_sha256: str,
) -> Evaluation:
    raw_answers = body.get("answers")
    if not isinstance(raw_answers, dict):
        raise JevRequestError(0, "response carries no answers object")
    answers: dict[str, Answer] = {}
    for name, payload in raw_answers.items():
        if not isinstance(payload, dict):
            raise JevRequestError(0, f"answer {name} is not an object")
        answers[str(name)] = parse_answer(str(name), payload)
    usage: dict[str, float] = {}
    raw_usage = body.get("usage")
    if isinstance(raw_usage, dict):
        for key, value in raw_usage.items():
            if not isinstance(value, bool) and isinstance(value, (int, float)):
                usage[str(key)] = float(value)
    model = body.get("model")
    request_id = body.get("id")
    return Evaluation(
        model=model if isinstance(model, str) else requested_model,
        requested_model=requested_model,
        answers=answers,
        usage=usage,
        request_sha256=request_sha256,
        response_sha256=response_sha256,
        request_id=request_id if isinstance(request_id, str) else "",
    )


class JevClient:
    def __init__(
        self,
        *,
        api_key: str | None = None,
        endpoint: str | None = None,
        model: str | None = None,
        timeout_seconds: float = 30.0,
        max_attempts: int = 4,
        log_path: Path | None = None,
    ) -> None:
        self.api_key = api_key
        self.endpoint = endpoint or os.environ.get(ENDPOINT_ENV) or DEFAULT_ENDPOINT
        self.model = model or os.environ.get(MODEL_ENV) or DEFAULT_MODEL
        self.timeout_seconds = timeout_seconds
        self.max_attempts = max(1, max_attempts)
        self.log_path = log_path

    def resolved_api_key(self) -> str:
        key = self.api_key if self.api_key is not None else os.environ.get(API_KEY_ENV)
        if not key:
            raise JevConfigError(f"{API_KEY_ENV} is not set")
        return key

    def evaluate(
        self,
        state: object,
        questions: Mapping[str, Mapping[str, object]],
    ) -> Evaluation:
        payload, request_sha256 = canonical_request(self.model, state, questions)
        key = self.resolved_api_key()
        raw = self._send(payload, key)
        response_sha256 = hashlib.sha256(raw).hexdigest()
        try:
            document = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise JevRequestError(0, f"response is not JSON: {error}") from error
        if not isinstance(document, dict):
            raise JevRequestError(0, "response is not a JSON object")
        evaluation = parse_evaluation(document, self.model, request_sha256, response_sha256)
        self._log(evaluation)
        return evaluation

    def _send(self, payload: bytes, api_key: str) -> bytes:
        request = urllib.request.Request(
            self.endpoint,
            data=payload,
            headers={
                "authorization": f"Bearer {api_key}",
                "content-type": "application/json",
            },
            method="POST",
        )
        status = 0
        excerpt = "no response"
        for attempt in range(1, self.max_attempts + 1):
            try:
                with urllib.request.urlopen(request, timeout=self.timeout_seconds) as response:
                    return response.read()
            except urllib.error.HTTPError as error:
                status = error.code
                excerpt = error.read().decode("utf-8", errors="replace")[:EXCERPT_LIMIT]
                if status not in RETRY_STATUSES:
                    raise JevRequestError(status, excerpt) from error
            except urllib.error.URLError as error:
                status = 0
                excerpt = str(error.reason)[:EXCERPT_LIMIT]
            if attempt < self.max_attempts:
                time.sleep(BACKOFF_SECONDS * float(2 ** (attempt - 1)))
        raise JevRequestError(status, excerpt)

    def _log(self, evaluation: Evaluation) -> None:
        if self.log_path is None:
            return
        entry = {
            "ts": datetime.now(timezone.utc).isoformat(),
            "requested_model": evaluation.requested_model,
            "model": evaluation.model,
            "request_sha256": evaluation.request_sha256,
            "response_sha256": evaluation.response_sha256,
            "usage": dict(evaluation.usage),
            "request_id": evaluation.request_id,
        }
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        with self.log_path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(entry, sort_keys=True) + "\n")


class DryRunClient(JevClient):
    def evaluate(
        self,
        state: object,
        questions: Mapping[str, Mapping[str, object]],
    ) -> Evaluation:
        _, request_sha256 = canonical_request(self.model, state, questions)
        raise JevDryRun(
            f"dry run: {len(questions)} questions for {self.model} "
            f"were not sent ({request_sha256})"
        )


def batched(items: Iterable[T], size: int) -> Iterator[list[T]]:
    if size < 1:
        raise ValueError("batch size must be positive")
    batch: list[T] = []
    for item in items:
        batch.append(item)
        if len(batch) == size:
            yield batch
            batch = []
    if batch:
        yield batch
