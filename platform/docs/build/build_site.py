#!/usr/bin/env python3
"""Build the LayerX developer documentation site and gate its executable samples."""

from __future__ import annotations

import argparse
import hashlib
import html
import json
import re
from urllib.parse import urlsplit
from dataclasses import dataclass
from pathlib import Path

ENFORCEMENT_LAYERS = ("protocol", "agent-layer", "service", "hosted-surface")

ENFORCEMENT_MEANING = {
    "protocol": "The LayerX state machine refuses the violating transition. The guarantee survives every"
    " component above it, including a hostile client, a hostile daemon and a hostile gateway.",
    "agent-layer": "layerx-agentd enforces it while it is in the request path. Bypassing the daemon bypasses"
    " the restriction; it is not a protocol guarantee.",
    "service": "A LayerX service process enforces it - layerx-human-service, the settlement service or the"
    " middleware you deploy. It binds callers of that service only.",
    "hosted-surface": "The hosted surface enforces it - the gateway, the faucet or the developer dashboard."
    " It is an operational control on the hosted deployment, not a property of the protocol.",
}

FENCE = re.compile(
    r"^```(?P<language>[A-Za-z0-9+#-]*)"
    r"(?P<attributes>(?:\s+[a-z_]+=[^\s`]+)*)\s*$"
)

ENFORCEMENT_ROW = re.compile(
    r"^\|\s*(?P<title>[^|`]+?)\s*\|\s*`(?P<layer>protocol|agent-layer|service|hosted-surface)`\s*\|",
    re.MULTILINE,
)

COMMENT_SYNTAX = {
    "//": ("// layerx:begin ", "// layerx:end "),
    "#": ("# layerx:begin ", "# layerx:end "),
    "<!--": ("<!-- layerx:begin ", "<!-- layerx:end "),
}


class DocumentationError(Exception):
    pass


@dataclass(frozen=True)
class Page:
    identifier: str
    section: str
    title: str
    summary: str
    source: Path
    order: int
    generated: bool


@dataclass(frozen=True)
class Section:
    identifier: str
    title: str
    order: int


@dataclass(frozen=True)
class Capability:
    identifier: str
    title: str
    enforcement: str
    statement: str
    surface: str


@dataclass(frozen=True)
class Sample:
    identifier: str
    title: str
    language: str
    fence: str
    comment: str
    path: Path
    location: str
    entry: str
    install: str
    build: str
    run: str
    measured_region: str
    maximum_lines: int
    requires: str


@dataclass(frozen=True)
class Operation:
    plane: str
    module: str
    name: str
    method: str
    path: str
    request: str
    response: str
    idempotent: bool
    required: tuple[str, ...]
    response_required: tuple[str, ...]


def parse_document(path: Path) -> dict[str, dict[str, str]]:
    current: str | None = None
    values: dict[str, str] = {}
    sections: dict[str, dict[str, str]] = {}

    def finish() -> None:
        nonlocal current, values
        if current is None:
            return
        if current in sections:
            raise DocumentationError(f"duplicate section {current} in {path}")
        sections[current] = values
        current = None
        values = {}

    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        header = re.fullmatch(r"\[([^]]+)]", line)
        if header:
            finish()
            current = header.group(1)
            continue
        if current is not None and "=" in line and not line.startswith("#"):
            key, value = line.split("=", 1)
            values[key.strip()] = value.strip()
    finish()
    return sections


def text(values: dict[str, str], key: str, origin: str, default: str | None = None) -> str:
    raw = values.get(key)
    if raw is None:
        if default is not None:
            return default
        raise DocumentationError(f"{origin} is missing {key}")
    if not (raw.startswith('"') and raw.endswith('"') and len(raw) >= 2):
        raise DocumentationError(f"{origin} key {key} must be a quoted string")
    return json.loads(raw)


def number(values: dict[str, str], key: str, origin: str) -> int:
    raw = values.get(key)
    if raw is None:
        raise DocumentationError(f"{origin} is missing {key}")
    if not re.fullmatch(r"[0-9]+", raw):
        raise DocumentationError(f"{origin} key {key} must be a non-negative integer")
    return int(raw)


def string_list(values: dict[str, str], key: str, origin: str) -> tuple[str, ...]:
    raw = values.get(key)
    if raw is None:
        return ()
    parsed = json.loads(raw)
    if not isinstance(parsed, list) or not all(isinstance(item, str) for item in parsed):
        raise DocumentationError(f"{origin} key {key} must be a list of strings")
    return tuple(parsed)


def load_sections(root: Path) -> tuple[dict[str, Section], dict[str, Page], dict[str, str]]:
    document = parse_document(root / "site.kvx")
    site = document.get("site")
    if site is None:
        raise DocumentationError("site.kvx is missing the [site] section")
    settings = {
        "name": text(site, "name", "[site]"),
        "tagline": text(site, "tagline", "[site]"),
        "output": text(site, "output", "[site]"),
        "content": text(site, "content", "[site]"),
    }
    sections: dict[str, Section] = {}
    pages: dict[str, Page] = {}
    for header, values in document.items():
        if header.startswith("section."):
            identifier = header.removeprefix("section.")
            sections[identifier] = Section(
                identifier,
                text(values, "title", f"[{header}]"),
                number(values, "order", f"[{header}]"),
            )
        elif header.startswith("page."):
            identifier = header.removeprefix("page.")
            section = text(values, "section", f"[{header}]")
            pages[identifier] = Page(
                identifier,
                section,
                text(values, "title", f"[{header}]"),
                text(values, "summary", f"[{header}]"),
                root / text(values, "file", f"[{header}]"),
                number(values, "order", f"[{header}]"),
                values.get("generated") == "true",
            )
    for page in pages.values():
        if page.section not in sections:
            raise DocumentationError(f"page {page.identifier} names unknown section {page.section}")
    return sections, pages, settings


def load_capabilities(root: Path) -> dict[str, Capability]:
    document = parse_document(root / "capabilities.kvx")
    capabilities: dict[str, Capability] = {}
    for header, values in document.items():
        if not header.startswith("capability."):
            continue
        identifier = header.removeprefix("capability.")
        enforcement = text(values, "enforcement", f"[{header}]")
        if enforcement not in ENFORCEMENT_LAYERS:
            raise DocumentationError(
                f"[{header}] enforcement {enforcement} is not one of {', '.join(ENFORCEMENT_LAYERS)}"
            )
        capabilities[identifier] = Capability(
            identifier,
            text(values, "title", f"[{header}]"),
            enforcement,
            text(values, "statement", f"[{header}]"),
            text(values, "surface", f"[{header}]"),
        )
    if not capabilities:
        raise DocumentationError("capabilities.kvx declares no capability")
    return capabilities


def load_samples(root: Path) -> dict[str, Sample]:
    document = parse_document(root / "samples.kvx")
    samples: dict[str, Sample] = {}
    for header, values in document.items():
        if not header.startswith("sample."):
            continue
        identifier = header.removeprefix("sample.")
        comment = text(values, "comment", f"[{header}]")
        if comment not in COMMENT_SYNTAX:
            raise DocumentationError(f"[{header}] comment {comment} is not a known marker syntax")
        samples[identifier] = Sample(
            identifier,
            text(values, "title", f"[{header}]"),
            text(values, "language", f"[{header}]"),
            text(values, "fence", f"[{header}]"),
            comment,
            root / text(values, "path", f"[{header}]"),
            text(values, "path", f"[{header}]"),
            text(values, "entry", f"[{header}]"),
            text(values, "install", f"[{header}]", ""),
            text(values, "build", f"[{header}]", ""),
            text(values, "run", f"[{header}]"),
            text(values, "measured_region", f"[{header}]"),
            number(values, "maximum_integration_lines", f"[{header}]"),
            text(values, "requires", f"[{header}]"),
        )
    if not samples:
        raise DocumentationError("samples.kvx declares no sample")
    return samples


def region_of(sample: Sample, relative: str, region: str) -> list[str]:
    source = sample.path / relative
    if not source.is_file():
        raise DocumentationError(f"sample {sample.identifier} has no file {relative}")
    begin, end = COMMENT_SYNTAX[sample.comment]
    lines = source.read_text(encoding="utf-8").splitlines()
    opened: int | None = None
    closed: int | None = None
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith(begin) and stripped.removeprefix(begin).strip().rstrip("->").strip() == region:
            if opened is not None:
                raise DocumentationError(f"{source} opens region {region} twice")
            opened = index
        if stripped.startswith(end) and stripped.removeprefix(end).strip().rstrip("->").strip() == region:
            if opened is None:
                raise DocumentationError(f"{source} closes region {region} before opening it")
            closed = index
            break
    if opened is None or closed is None:
        raise DocumentationError(f"{source} does not delimit region {region}")
    body = lines[opened + 1 : closed]
    indent = min((len(line) - len(line.lstrip()) for line in body if line.strip()), default=0)
    return [line[indent:] if line.strip() else "" for line in body]


def measure_samples(samples: dict[str, Sample]) -> list[dict[str, object]]:
    measurements: list[dict[str, object]] = []
    for sample in sorted(samples.values(), key=lambda item: item.identifier):
        body = region_of(sample, sample.entry, sample.measured_region)
        counted = [line for line in body if line.strip()]
        if len(counted) > sample.maximum_lines:
            raise DocumentationError(
                f"sample {sample.identifier} adds {len(counted)} lines of integration code in region"
                f" {sample.measured_region}; the published budget is {sample.maximum_lines}"
            )
        measurements.append(
            {
                "sample": sample.identifier,
                "language": sample.language,
                "entry": sample.entry,
                "region": sample.measured_region,
                "integration_lines": len(counted),
                "budget": sample.maximum_lines,
            }
        )
    return measurements


def fence_attributes(raw: str) -> dict[str, str]:
    return {key: value for key, value in (item.split("=", 1) for item in raw.split())}


def fill_samples(document: str, origin: Path, samples: dict[str, Sample]) -> str:
    lines = document.splitlines()
    output: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        match = FENCE.fullmatch(line)
        if match is None or "sample=" not in (match.group("attributes") or ""):
            output.append(line)
            index += 1
            continue
        attributes = fence_attributes(match.group("attributes"))
        identifier = attributes["sample"]
        sample = samples.get(identifier)
        if sample is None:
            raise DocumentationError(f"{origin} references unknown sample {identifier}")
        if (match.group("language") or "") != sample.fence:
            raise DocumentationError(
                f"{origin} opens sample {identifier} with fence language"
                f" {match.group('language') or 'none'}; the sample declares {sample.fence}"
            )
        relative = attributes.get("file", sample.entry)
        region = attributes.get("region", sample.measured_region)
        closing = index + 1
        while closing < len(lines) and lines[closing].rstrip() != "```":
            closing += 1
        if closing >= len(lines):
            raise DocumentationError(f"{origin} leaves a sample fence unclosed at line {index + 1}")
        output.append(line)
        output.extend(region_of(sample, relative, region))
        output.append("```")
        index = closing + 1
    return "\n".join(output) + "\n"


def render_operations(root: Path, plane: str, relative: str) -> tuple[dict[str, str], list[Operation], dict[str, dict[str, dict[str, str]]]]:
    base = root / relative
    document = parse_document(base / "v1.kvx")
    schema = document.get("schema")
    if schema is None:
        raise DocumentationError(f"{base}/v1.kvx is missing the [schema] section")
    includes = string_list(schema, "includes", f"{base}/v1.kvx [schema]")
    modules: dict[str, dict[str, dict[str, str]]] = {}
    operations: list[Operation] = []
    for name in ("v1.kvx", *includes):
        path = base / name
        if path.parent != base or not path.is_file():
            raise DocumentationError(f"invalid schema include {path}")
        parsed = parse_document(path)
        module = text(parsed.get("module", {}), "name", f"{path} [module]", name.removesuffix(".kvx"))
        modules[module] = parsed
        mutations = {
            header.removeprefix("mutation.") for header in parsed if header.startswith("mutation.")
        }
        for header, values in parsed.items():
            if not header.startswith("operation."):
                continue
            operation = header.removeprefix("operation.")
            required = string_list(values, "required", f"{path} [{header}]")
            idempotent = values.get("idempotency") == "true" or (
                plane == "agent" and (operation in mutations or "idempotency_key" in required)
            )
            operations.append(
                Operation(
                    plane,
                    module,
                    operation,
                    text(values, "method", f"{path} [{header}]", "POST"),
                    text(values, "path", f"{path} [{header}]", ""),
                    text(values, "request", f"{path} [{header}]", "object"),
                    text(values, "response", f"{path} [{header}]", "object"),
                    idempotent,
                    required,
                    string_list(values, "response_required", f"{path} [{header}]"),
                )
            )
    operations.sort(key=lambda item: (item.module, item.name))
    return schema, operations, modules


def code(value: str) -> str:
    return f"`{value}`" if value else "-"


def field_list(fields: tuple[str, ...]) -> str:
    return ", ".join(f"`{field}`" for field in fields) if fields else "-"


def reference_page(root: Path, plane: str, relative: str, title: str, transport: str) -> str:
    schema, operations, modules = render_operations(root, plane, relative)
    lines = [
        f"<!-- Generated from {relative} by platform/docs/build/build_site.py. Do not hand-edit. -->",
        "",
        f"# {title}",
        "",
        f"Schema `{text(schema, 'name', '[schema]')}`, contract major"
        f" `{schema.get('major', '?')}`, minor `{schema.get('minor', '?')}`,"
        f" generated from `{relative}`.",
        "",
        transport,
        "",
        f"{text(schema, 'compatibility', '[schema]')}",
        "",
        "## Operations",
        "",
    ]
    if plane == "human":
        lines += [
            "| Operation | Method | Path | Request | Response | Idempotency-Key |",
            "|---|---|---|---|---|---|",
        ]
        for item in operations:
            lines.append(
                f"| `{item.name}` | `{item.method}` | {code(item.path)} | {code(item.request)}"
                f" | {code(item.response)} | {'required' if item.idempotent else 'not used'} |"
            )
    else:
        lines += [
            "| Operation | Module | Request | Response | Required request fields |",
            "|---|---|---|---|---|",
        ]
        for item in operations:
            lines.append(
                f"| `{item.name}` | `{item.module}` | {code(item.request)} | {code(item.response)}"
                f" | {field_list(item.required)} |"
            )
    lines += ["", "## Declared types", ""]
    for module in sorted(modules):
        parsed = modules[module]
        declarations = {
            header: values
            for header, values in parsed.items()
            if header.startswith(("type.", "record.", "scalar."))
        }
        if not declarations:
            continue
        lines += [f"### Module `{module}`", ""]
        module_values = parsed.get("module", {})
        for key in ("vocabulary", "verb", "model", "compatibility"):
            if key in module_values:
                lines += [text(module_values, key, f"[module] {key}"), ""]
        lines += ["| Declaration | Kind | Shape |", "|---|---|---|"]
        for header in sorted(declarations):
            kind, _, name = header.partition(".")
            values = declarations[header]
            shape_parts: list[str] = []
            for key in ("variants", "required", "optional", "fields"):
                items = string_list(values, key, f"[{header}]")
                if items:
                    shape_parts.append(f"{key}: {field_list(items)}")
            for key in ("json", "format", "prefix", "wire", "rust", "typescript", "python"):
                if key in values:
                    shape_parts.append(f"{key}: {code(text(values, key, f'[{header}]'))}")
            shape = "<br>".join(shape_parts) if shape_parts else "-"
            lines.append(f"| `{name}` | {kind} | {shape} |")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def errors_page(root: Path) -> str:
    human = parse_document(root / "human/schema/human-api/errors.kvx")
    agent = parse_document(root / "agent/schema/agent-api/errors.kvx")
    lines = [
        "<!-- Generated from human/schema/human-api/errors.kvx and agent/schema/agent-api/errors.kvx"
        " by platform/docs/build/build_site.py. Do not hand-edit. -->",
        "",
        "# Error reference",
        "",
        "Both contracts refuse with a typed shape. No operation on either plane returns an unstructured"
        " error, and no SDK converts a refusal into a success.",
        "",
        "## Human API",
        "",
        text(human.get("module", {}), "model", "human errors [module]"),
        "",
        "| Machine code |",
        "|---|",
    ]
    for variant in string_list(human.get("type.ErrorCode", {}), "variants", "[type.ErrorCode]"):
        lines.append(f"| `{variant}` |")
    lines += ["", "### Retriability", "", "| Class | Meaning |", "|---|---|"]
    retriability = human.get("type.Retriability", {})
    for variant in string_list(retriability, "variants", "[type.Retriability]"):
        meaning = text(retriability, f"{variant}.semantics", "[type.Retriability]", "")
        lines.append(f"| `{variant}` | {meaning} |")
    lines += [
        "",
        "## Agent API",
        "",
        "| Error class |",
        "|---|",
    ]
    for variant in string_list(agent.get("type.ErrorClass", {}), "variants", "[type.ErrorClass]"):
        lines.append(f"| `{variant}` |")
    lines += ["", "### Verification levels", "", "| Level |", "|---|"]
    for variant in string_list(agent.get("type.Level", {}), "variants", "[type.Level]"):
        lines.append(f"| `{variant}` |")
    lines += [
        "",
        "The agent contract orders verification levels by declaration order: a later level implies every"
        " earlier one, and no layer reports a level its evidence does not justify.",
        "",
    ]
    return "\n".join(lines).rstrip() + "\n"


def enforcement_page(capabilities: dict[str, Capability]) -> str:
    lines = [
        "<!-- Generated from platform/docs/capabilities.kvx by platform/docs/build/build_site.py."
        " Do not hand-edit. -->",
        "",
        "# Enforcement reference",
        "",
        "Every capability in this documentation carries the layer that actually enforces it. A lower layer"
        " never implies a higher one. This page is generated from the same registry the site build checks,"
        " so a documented capability without a label fails the build.",
        "",
        "## What each label means",
        "",
        "| Label | Meaning |",
        "|---|---|",
    ]
    for layer in ENFORCEMENT_LAYERS:
        lines.append(f"| `{layer}` | {ENFORCEMENT_MEANING[layer]} |")
    for layer in ENFORCEMENT_LAYERS:
        selected = [item for item in capabilities.values() if item.enforcement == layer]
        if not selected:
            continue
        lines += [
            "",
            f"## Enforced by {layer}",
            "",
            "| Capability | Where you meet it | What is guaranteed |",
            "|---|---|---|",
        ]
        for item in sorted(selected, key=lambda entry: entry.title):
            lines.append(f"| {item.title} | {item.surface} | {item.statement} |")
    lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def samples_page(samples: dict[str, Sample], measurements: list[dict[str, object]]) -> str:
    counts = {item["sample"]: item for item in measurements}
    lines = [
        "<!-- Generated from platform/docs/samples.kvx by platform/docs/build/build_site.py."
        " Do not hand-edit. -->",
        "",
        "# Sample index",
        "",
        "Every code block in this documentation is extracted from one of these directories. The site build"
        " re-extracts each block from its source file and fails when a page and its sample disagree, so a"
        " stale sample cannot survive a build.",
        "",
        "| Sample | Language | Directory | Run it | Integration lines |",
        "|---|---|---|---|---|",
    ]
    for sample in sorted(samples.values(), key=lambda item: item.identifier):
        measured = counts[sample.identifier]
        command = " && ".join(part for part in (sample.install, sample.build, sample.run) if part)
        lines.append(
            f"| {sample.title} | `{sample.language}` | `platform/docs/{sample.location}`"
            f" | `{command}` | {measured['integration_lines']} of {measured['budget']} |"
        )
    lines += [
        "",
        "## What each sample needs",
        "",
        "| Sample | Requires |",
        "|---|---|",
    ]
    for sample in sorted(samples.values(), key=lambda item: item.identifier):
        lines.append(f"| {sample.title} | {sample.requires} |")
    lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def inline(value: str) -> str:
    pieces = re.split(r"(`[^`]+`)", value)
    rendered: list[str] = []
    for piece in pieces:
        if piece.startswith("`") and piece.endswith("`") and len(piece) > 1:
            rendered.append(f"<code>{html.escape(piece[1:-1])}</code>")
            continue
        escaped = html.escape(piece)
        escaped = re.sub(r"\[([^\]]+)]\(([^)]+)\)", r'<a href="\2">\1</a>', escaped)
        escaped = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", escaped)
        escaped = escaped.replace("&lt;br&gt;", "<br>")
        rendered.append(escaped)
    return "".join(rendered)


def render_markdown(document: str) -> str:
    lines = document.splitlines()
    output: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        if line.startswith("<!--"):
            index += 1
            continue
        fence = FENCE.fullmatch(line)
        if fence is not None:
            language = fence.group("language") or "text"
            index += 1
            body: list[str] = []
            while index < len(lines) and lines[index].rstrip() != "```":
                body.append(lines[index])
                index += 1
            index += 1
            escaped = html.escape("\n".join(body))
            output.append(
                f'<div class="code"><div class="code-language">{html.escape(language)}</div>'
                f"<pre><code>{escaped}</code></pre></div>"
            )
            continue
        heading = re.fullmatch(r"(#{1,4})\s+(.*)", line)
        if heading is not None:
            level = len(heading.group(1))
            slug = re.sub(r"[^a-z0-9]+", "-", heading.group(2).lower()).strip("-")
            output.append(f'<h{level} id="{slug}">{inline(heading.group(2))}</h{level}>')
            index += 1
            continue
        if line.startswith("|") and index + 1 < len(lines) and re.fullmatch(r"\|[-:| ]+\|", lines[index + 1].strip()):
            header = [cell.strip() for cell in line.strip().strip("|").split("|")]
            index += 2
            rows: list[list[str]] = []
            while index < len(lines) and lines[index].startswith("|"):
                rows.append([cell.strip() for cell in lines[index].strip().strip("|").split("|")])
                index += 1
            head = "".join(f"<th>{inline(cell)}</th>" for cell in header)
            body_rows = "".join(
                "<tr>" + "".join(f"<td>{inline(cell)}</td>" for cell in row) + "</tr>" for row in rows
            )
            output.append(
                f'<div class="table"><table><thead><tr>{head}</tr></thead><tbody>{body_rows}</tbody></table></div>'
            )
            continue
        if re.fullmatch(r"[-*]\s+.*", line):
            items: list[str] = []
            while index < len(lines) and re.fullmatch(r"[-*]\s+.*", lines[index]):
                items.append(f"<li>{inline(lines[index][2:].strip())}</li>")
                index += 1
            output.append("<ul>" + "".join(items) + "</ul>")
            continue
        if re.fullmatch(r"[0-9]+\.\s+.*", line):
            items = []
            while index < len(lines) and re.fullmatch(r"[0-9]+\.\s+.*", lines[index]):
                items.append(f"<li>{inline(lines[index].split('.', 1)[1].strip())}</li>")
                index += 1
            output.append("<ol>" + "".join(items) + "</ol>")
            continue
        if line.startswith("> "):
            quote: list[str] = []
            while index < len(lines) and lines[index].startswith("> "):
                quote.append(inline(lines[index][2:]))
                index += 1
            output.append("<blockquote>" + " ".join(quote) + "</blockquote>")
            continue
        if line.strip() == "":
            index += 1
            continue
        paragraph: list[str] = []
        while index < len(lines) and lines[index].strip() != "" and not lines[index].startswith(
            ("#", "|", "> ", "```", "- ", "* ", "<!--")
        ):
            paragraph.append(lines[index].strip())
            index += 1
        output.append(f"<p>{inline(' '.join(paragraph))}</p>")
    return "\n".join(output)


STYLE = """:root{color-scheme:light dark;--ink:#101418;--muted:#5b6672;--rule:#dfe4ea;--surface:#ffffff;
--page:#f6f7f9;--accent:#1f4bd8;--code:#f2f4f7;--protocol:#0f7a4d;--agent-layer:#8a5a00;--service:#1f4bd8;
--hosted-surface:#7a2f9a}
@media (prefers-color-scheme:dark){:root{--ink:#e8ecf1;--muted:#9aa5b1;--rule:#2a313a;--surface:#151a20;
--page:#0e1216;--accent:#8ab0ff;--code:#1b2129;--protocol:#5fd3a0;--agent-layer:#e0b060;--service:#8ab0ff;
--hosted-surface:#c99ae0}}
*{box-sizing:border-box}body{margin:0;background:var(--page);color:var(--ink);
font:16px/1.65 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif}
.shell{display:grid;grid-template-columns:280px minmax(0,1fr);gap:0;min-height:100vh}
nav{border-right:1px solid var(--rule);padding:28px 22px;background:var(--surface);position:sticky;top:0;
height:100vh;overflow-y:auto}
nav .brand{font-weight:700;font-size:18px;letter-spacing:-0.01em;display:block;color:var(--ink);
text-decoration:none}
nav .tagline{color:var(--muted);font-size:13px;margin:6px 0 22px}
nav h2{font-size:11px;text-transform:uppercase;letter-spacing:0.09em;color:var(--muted);margin:22px 0 8px}
nav a{display:block;padding:5px 8px;margin-left:-8px;border-radius:6px;color:var(--ink);text-decoration:none;
font-size:14px}
nav a:hover{background:var(--code)}nav a.current{background:var(--code);font-weight:600;color:var(--accent)}
main{padding:44px 48px 96px;max-width:960px;min-width:0;overflow-wrap:anywhere}
h1{font-size:32px;letter-spacing:-0.02em;margin:0 0 8px}
h2{font-size:22px;letter-spacing:-0.01em;margin:38px 0 10px;padding-top:14px;border-top:1px solid var(--rule)}
h3{font-size:17px;margin:26px 0 8px}h4{font-size:15px;margin:20px 0 6px;color:var(--muted)}
p{margin:12px 0}ul,ol{margin:12px 0;padding-left:22px}li{margin:4px 0}
a{color:var(--accent)}code{background:var(--code);border-radius:4px;padding:1px 5px;font-size:13.5px;
font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
.summary{color:var(--muted);font-size:17px;margin:0 0 4px}
.code{border:1px solid var(--rule);border-radius:10px;overflow:hidden;margin:16px 0;background:var(--surface)}
.code-language{font-size:11px;text-transform:uppercase;letter-spacing:0.08em;color:var(--muted);
padding:7px 14px;border-bottom:1px solid var(--rule);background:var(--code)}
pre{margin:0;padding:14px 16px;overflow-x:auto}
pre code{background:none;padding:0;font-size:13px;line-height:1.6}
.table{overflow-x:auto;border:1px solid var(--rule);border-radius:10px;margin:16px 0;background:var(--surface)}
table{border-collapse:collapse;width:100%;font-size:14px}
th,td{text-align:left;padding:9px 14px;border-bottom:1px solid var(--rule);vertical-align:top}
th{font-size:12px;text-transform:uppercase;letter-spacing:0.06em;color:var(--muted)}
tbody tr:last-child td{border-bottom:none}
blockquote{margin:16px 0;padding:10px 16px;border-left:3px solid var(--accent);background:var(--surface);
border-radius:0 8px 8px 0;color:var(--muted)}
.badge{display:inline-block;font-size:11px;text-transform:uppercase;letter-spacing:0.07em;font-weight:700;
padding:2px 8px;border-radius:999px;border:1px solid currentColor}
.badge.protocol{color:var(--protocol)}.badge.agent-layer{color:var(--agent-layer)}
.badge.service{color:var(--service)}.badge.hosted-surface{color:var(--hosted-surface)}
footer{margin-top:56px;padding-top:18px;border-top:1px solid var(--rule);color:var(--muted);font-size:13px}
.visually-hidden{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;
clip:rect(0 0 0 0);white-space:nowrap;border:0}
.search{position:relative;margin:0 0 8px}
.search input{width:100%;padding:8px 10px;border:1px solid var(--rule);border-radius:8px;
background:var(--page);color:var(--ink);font-size:14px}
.search input:focus{outline:2px solid var(--accent);outline-offset:1px}
.search-results{position:absolute;z-index:10;left:0;right:0;top:calc(100% + 4px);max-height:60vh;
overflow-y:auto;background:var(--surface);border:1px solid var(--rule);border-radius:10px;
box-shadow:0 12px 28px rgba(0,0,0,0.18)}
.search-results a{display:block;padding:8px 12px;color:var(--ink);text-decoration:none;
border-bottom:1px solid var(--rule)}
.search-results a:last-child{border-bottom:none}
.search-results a:hover{background:var(--code)}
.search-title{display:block;font-weight:600;font-size:14px}
.search-section{display:block;font-size:11px;text-transform:uppercase;letter-spacing:0.07em;color:var(--muted)}
.search-snippet{display:block;font-size:12.5px;color:var(--muted)}
.search-empty{padding:10px 12px;margin:0;color:var(--muted);font-size:13px}
@media (max-width:900px){.shell{grid-template-columns:minmax(0,1fr)}nav{position:static;height:auto;
border-right:none;border-bottom:1px solid var(--rule)}main{padding:28px 20px 64px}}
"""


SEARCH_WIDGET = """(function () {
  var index = window.LAYERX_SEARCH_INDEX || [];
  var input = document.getElementById("search");
  var results = document.getElementById("search-results");
  if (!input || !results) { return; }
  function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, function (character) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[character];
    });
  }
  function render(query) {
    var trimmed = query.trim().toLowerCase();
    if (trimmed.length < 2) { results.hidden = true; results.innerHTML = ""; return; }
    var terms = trimmed.split(/\\s+/);
    var hits = [];
    for (var i = 0; i < index.length; i += 1) {
      var page = index[i];
      var haystack = (page.title + " " + page.section + " " + page.summary + " "
        + page.headings.join(" ") + " " + page.text).toLowerCase();
      var matched = true;
      for (var t = 0; t < terms.length; t += 1) {
        if (haystack.indexOf(terms[t]) < 0) { matched = false; break; }
      }
      if (!matched) { continue; }
      var score = page.title.toLowerCase().indexOf(terms[0]) >= 0 ? 1 : 0;
      hits.push([score, page]);
    }
    hits.sort(function (left, right) { return right[0] - left[0]; });
    hits = hits.slice(0, 10);
    if (!hits.length) {
      results.hidden = false;
      results.innerHTML = '<p class="search-empty">No matches.</p>';
      return;
    }
    var markup = "";
    for (var j = 0; j < hits.length; j += 1) {
      var hit = hits[j][1];
      var snippet = hit.summary || hit.text.slice(0, 120);
      markup += '<a href="' + hit.id + '.html">'
        + '<span class="search-title">' + escapeHtml(hit.title) + '</span>'
        + '<span class="search-section">' + escapeHtml(hit.section) + '</span>'
        + '<span class="search-snippet">' + escapeHtml(snippet) + '</span></a>';
    }
    results.hidden = false;
    results.innerHTML = markup;
  }
  input.addEventListener("input", function () { render(input.value); });
  document.addEventListener("keydown", function (event) {
    var tag = document.activeElement ? document.activeElement.tagName : "";
    if (event.key === "/" && document.activeElement !== input && !/INPUT|TEXTAREA|SELECT/.test(tag)) {
      event.preventDefault();
      input.focus();
    }
  });
}());
"""


def plain_text(document: str) -> str:
    pieces: list[str] = []
    in_fence = False
    for raw in document.splitlines():
        line = raw.strip()
        if FENCE.fullmatch(line) is not None:
            in_fence = not in_fence
            continue
        if in_fence or not line:
            continue
        if line.startswith(("<!--", "|", "> ")):
            continue
        if re.fullmatch(r"[-*]\s+.*", line) or re.fullmatch(r"[0-9]+\.\s+.*", line):
            line = re.sub(r"^(?:[-*]|[0-9]+\.)\s+", "", line)
        line = re.sub(r"`([^`]*)`", r"\1", line)
        line = re.sub(r"\[([^\]]+)]\(([^)]+)\)", r"\1", line)
        line = line.lstrip("#").strip()
        if line:
            pieces.append(line)
    return " ".join(pieces)


def search_entries(
    documents: dict[str, str], pages: dict[str, Page], sections: dict[str, Section]
) -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    for page in sorted(pages.values(), key=lambda item: (sections[item.section].order, item.order)):
        document = documents[page.identifier]
        headings = [
            re.sub(r"`([^`]*)`", r"\1", match.group(2)).strip()
            for match in re.finditer(r"^(#{1,4})\s+(.*)$", document, re.MULTILINE)
        ]
        entries.append(
            {
                "id": page.identifier.replace("/", "-"),
                "title": page.title,
                "section": sections[page.section].title,
                "summary": page.summary,
                "headings": headings,
                "text": plain_text(document)[:4000],
            }
        )
    return entries


def search_script(entries: list[dict[str, object]]) -> str:
    return (
        "// Generated by platform/docs/build/build_site.py. Do not hand-edit.\n"
        "window.LAYERX_SEARCH_INDEX = "
        + json.dumps(entries, ensure_ascii=False, separators=(",", ":"))
        + ";\n"
        + SEARCH_WIDGET
    )


def navigation(sections: dict[str, Section], pages: dict[str, Page], current: str) -> str:
    parts: list[str] = [
        '<form class="search" role="search" onsubmit="return false">'
        '<label class="visually-hidden" for="search">Search documentation</label>'
        '<input id="search" type="search" placeholder="Search documentation" autocomplete="off"'
        ' spellcheck="false" aria-controls="search-results">'
        '<div id="search-results" class="search-results" hidden></div>'
        "</form>"
    ]
    for section in sorted(sections.values(), key=lambda item: item.order):
        entries = sorted(
            (page for page in pages.values() if page.section == section.identifier),
            key=lambda item: item.order,
        )
        if not entries:
            continue
        parts.append(f"<h2>{html.escape(section.title)}</h2>")
        for page in entries:
            marker = " class=\"current\"" if page.identifier == current else ""
            parts.append(
                f'<a href="{page.identifier.replace("/", "-")}.html"{marker}>{html.escape(page.title)}</a>'
            )
    return "".join(parts)


def render_page(page: Page, body: str, sections: dict[str, Section], pages: dict[str, Page], settings: dict[str, str]) -> str:
    return (
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n"
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n"
        f"<title>{html.escape(page.title)} - {html.escape(settings['name'])}</title>\n"
        f"<meta name=\"description\" content=\"{html.escape(page.summary)}\">\n"
        f"<style>{STYLE}</style>\n</head>\n<body>\n<div class=\"shell\">\n<nav>\n"
        f'<a class="brand" href="index.html">{html.escape(settings["name"])}</a>\n'
        f'<div class="tagline">{html.escape(settings["tagline"])}</div>\n'
        f"{navigation(sections, pages, page.identifier)}\n</nav>\n<main>\n"
        f'<p class="summary">{html.escape(page.summary)}</p>\n{body}\n'
        "<footer>Every capability on this site names the layer that enforces it. Every code block is"
        " extracted from a sample under platform/docs/samples and re-checked on each build.</footer>\n"
        "</main>\n</div>\n"
        '<script src="search.js" defer></script>\n'
        "</body>\n</html>\n"
    )


LINK = re.compile(r'(?:href|src)="([^"]+)"')
EXTERNAL_SCHEME = re.compile(r"^[A-Za-z][A-Za-z0-9+.-]*:")


def check_links(
    pages: dict[str, Page],
    rendered: dict[str, str],
    known: set[str],
) -> tuple[int, list[str]]:
    checked = 0
    problems: list[str] = []
    for page in pages.values():
        for raw in LINK.findall(rendered[page.identifier]):
            raw = html.unescape(raw).strip()
            target = urlsplit(raw).path
            if not target or target.startswith(("mailto:", "tel:")):
                continue
            if EXTERNAL_SCHEME.match(raw) or raw.startswith("//"):
                continue
            checked += 1
            if target.startswith("/"):
                if target.lstrip("/") not in known:
                    problems.append(
                        f"{page.identifier} links to {target}, which no page or asset generates"
                    )
                continue
            if ".." in Path(target).parts:
                problems.append(
                    f"{page.identifier} links outside the standalone site to {target}; "
                    "use an immutable source URL for repository references"
                )
                continue
            if target not in known:
                problems.append(
                    f"{page.identifier} links to {target}, which no page or asset generates"
                )
    return checked, problems


def generated_documents(root: Path, repository: Path, capabilities: dict[str, Capability], samples: dict[str, Sample], measurements: list[dict[str, object]]) -> dict[Path, str]:
    reference = root / "content" / "reference"
    return {
        reference
        / "human-api.md": reference_page(
            repository,
            "human",
            "human/schema/human-api",
            "Human API reference",
            "Transport is HTTPS with JSON bodies under the `/v1` base path. Every amount is a decimal string"
            " of base units and always travels with its currency code. Every mutation that can move money"
            " requires the `Idempotency-Key` header, and repeating the request returns the original journey"
            " rather than a second effect.",
        ),
        reference
        / "agent-api.md": reference_page(
            repository,
            "agent",
            "agent/schema/agent-api",
            "Agent API reference",
            "The agent contract is spoken by `layerx-agentd` and by direct-node SDK deployments. Requests and"
            " responses are canonical maps carrying the contract major and minor version; consensus integers"
            " are fixed-width in Rust and decimal strings in the dynamic-language SDKs.",
        ),
        reference / "errors.md": errors_page(repository),
        reference / "enforcement.md": enforcement_page(capabilities),
        reference / "samples.md": samples_page(samples, measurements),
    }


def build(repository: Path, write: bool) -> list[str]:
    root = repository / "platform" / "docs"
    sections, pages, settings = load_sections(root)
    capabilities = load_capabilities(root)
    samples = load_samples(root)
    measurements = measure_samples(samples)
    stale: list[str] = []

    for path, content in generated_documents(root, repository, capabilities, samples, measurements).items():
        if write:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
        elif not path.is_file() or path.read_text(encoding="utf-8") != content:
            stale.append(str(path.relative_to(repository)))

    content = root / settings["content"]
    documents: dict[str, str] = {}
    for page in pages.values():
        if not page.source.is_file():
            raise DocumentationError(f"page {page.identifier} has no source file {page.source}")
        source = page.source.read_text(encoding="utf-8")
        if not page.source.is_relative_to(content):
            documents[page.identifier] = source
            continue
        filled = fill_samples(source, page.source, samples)
        if filled != source:
            if write:
                page.source.write_text(filled, encoding="utf-8")
            else:
                stale.append(str(page.source.relative_to(repository)))
        documents[page.identifier] = filled

    declared = {value.title: value.enforcement for value in capabilities.values()}
    if not declared:
        raise DocumentationError("no capability carries an enforcement label")
    surfaced: set[str] = set()
    for page in pages.values():
        if page.generated or not page.source.is_relative_to(content):
            continue
        if "Enforced by" not in documents[page.identifier]:
            raise DocumentationError(
                f"page {page.identifier} documents capabilities without an enforcement table"
            )
        for match in ENFORCEMENT_ROW.finditer(documents[page.identifier]):
            title, layer = match.group("title"), match.group("layer")
            if title not in declared:
                raise DocumentationError(
                    f"page {page.identifier} labels {title!r}, which no capability in"
                    " platform/docs/capabilities.kvx declares"
                )
            if declared[title] != layer:
                raise DocumentationError(
                    f"page {page.identifier} labels {title!r} as {layer};"
                    f" capabilities.kvx declares it {declared[title]}"
                )
            surfaced.add(title)
    unsurfaced = sorted(set(declared) - surfaced)
    if unsurfaced:
        raise DocumentationError(
            "capabilities declared but documented on no page: " + ", ".join(unsurfaced)
        )

    output = root / settings["output"]
    rendered = {
        page.identifier: render_page(
            page, render_markdown(documents[page.identifier]), sections, pages, settings
        )
        for page in pages.values()
    }
    filenames = {
        page.identifier: f"{page.identifier.replace('/', '-')}.html" for page in pages.values()
    }
    known = set(filenames.values()) | {"search.js", "manifest.json", "measurements.json"}
    checked, problems = check_links(pages, rendered, known)
    if problems:
        raise DocumentationError("broken documentation links: " + "; ".join(sorted(problems)))
    if write:
        output.mkdir(parents=True, exist_ok=True)
        manifest_pages: list[dict[str, object]] = []
        for page in pages.values():
            body = rendered[page.identifier].encode("utf-8")
            target = output / filenames[page.identifier]
            target.write_bytes(body)
            manifest_pages.append(
                {
                    "id": page.identifier,
                    "title": page.title,
                    "section": sections[page.section].title,
                    "path": filenames[page.identifier],
                    "bytes": len(body),
                    "sha256": hashlib.sha256(body).hexdigest(),
                }
            )
        search = search_script(search_entries(documents, pages, sections))
        (output / "search.js").write_text(search, encoding="utf-8")
        (output / "measurements.json").write_text(
            json.dumps({"schema": 1, "samples": measurements}, indent=2) + "\n", encoding="utf-8"
        )
        (output / "link-check.json").write_text(
            json.dumps(
                {
                    "schema": 1,
                    "generated_pages": len(pages),
                    "internal_links_checked": checked,
                    "problems": problems,
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        (output / "manifest.json").write_text(
            json.dumps(
                {
                    "schema": 1,
                    "site": settings["name"],
                    "pages": manifest_pages,
                    "search_index": "search.js",
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
    return stale


def platform_docs_site() -> str:
    return "layerx-docs-site-v1"


def platform_docs_sample_gate() -> str:
    return "layerx-docs-sample-gate-v1"


def main() -> int:
    parser = argparse.ArgumentParser(description=platform_docs_site())
    parser.add_argument("--check", action="store_true", help="fail when any generated page or sample block is stale")
    parser.add_argument("repository", nargs="?", default=".")
    arguments = parser.parse_args()
    repository = Path(arguments.repository).resolve()
    try:
        stale = build(repository, not arguments.check)
    except DocumentationError as error:
        raise SystemExit(f"documentation build failed: {error}") from error
    if stale:
        raise SystemExit("stale documentation output: " + ", ".join(sorted(set(stale))))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
