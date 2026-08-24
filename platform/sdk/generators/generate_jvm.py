#!/usr/bin/env python3
"""Generate the Java-first LayerX schema model and typed operation catalogue."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


JAVA_KEYWORDS = {
    "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char", "class",
    "const", "continue", "default", "do", "double", "else", "enum", "extends", "final",
    "finally", "float", "for", "goto", "if", "implements", "import", "instanceof", "int",
    "interface", "long", "native", "new", "package", "private", "protected", "public",
    "return", "short", "static", "strictfp", "super", "switch", "synchronized", "this",
    "throw", "throws", "transient", "try", "void", "volatile", "while", "record", "sealed",
    "permits", "yield", "var",
}


def sections(root: Path) -> dict[str, dict[str, object]]:
    result: dict[str, dict[str, object]] = {}
    for path in sorted(root.glob("*.kvx")):
        if path.name == "baseline.kvx":
            continue
        current: str | None = None
        for raw in path.read_text(encoding="utf-8").splitlines():
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            if line.startswith("[") and line.endswith("]"):
                current = line[1:-1]
                result.setdefault(current, {})
                continue
            if current is None or "=" not in line:
                continue
            key, encoded = (part.strip() for part in line.split("=", 1))
            try:
                result[current][key] = json.loads(encoded)
            except json.JSONDecodeError:
                result[current][key] = encoded
    return result


def java_name(value: str) -> str:
    words = re.split(r"[^A-Za-z0-9]+", value)
    rendered = "".join(word[:1].upper() + word[1:] for word in words if word)
    if not rendered or rendered[0].isdigit():
        rendered = "Value" + rendered
    return rendered


def constant_name(value: str) -> str:
    rendered = re.sub(r"[^A-Za-z0-9]+", "_", value).strip("_").upper()
    return ("VALUE_" if rendered[:1].isdigit() else "") + rendered


def field_name(value: str) -> str:
    rendered = re.sub(r"[^A-Za-z0-9_]", "_", value)
    if rendered in JAVA_KEYWORDS or rendered[:1].isdigit():
        rendered += "_"
    return rendered


def string_list(value: object) -> list[str]:
    return [item for item in value if isinstance(item, str)] if isinstance(value, list) else []


class Plane:
    def __init__(self, name: str, values: dict[str, dict[str, object]]) -> None:
        self.name = name
        self.values = values
        self.models = {
            key.split(".", 1)[1]: entries
            for key, entries in values.items()
            if key.startswith(("type.", "record."))
        }
        self.scalars = {
            key.split(".", 1)[1]: entries
            for key, entries in values.items()
            if key.startswith("scalar.")
        }

    def fields(self, model: str | None, fallback: list[str]) -> list[tuple[str, str | None, bool]]:
        resolved_model = (model or "").split("<", 1)[0]
        entries = self.models.get(resolved_model, {})
        raw_required = string_list(entries.get("required")) or string_list(entries.get("fields"))
        raw_optional = string_list(entries.get("optional"))
        if not raw_required and not raw_optional:
            raw_required = fallback
        fields: list[tuple[str, str | None, bool]] = []
        for encoded, optional in [(item, False) for item in raw_required] + [
            (item, True) for item in raw_optional
        ]:
            name, separator, declared = encoded.partition(":")
            field_type = declared if separator else self.declared_field_type(entries, name)
            fields.append((name, field_type, optional))
        return fields

    def declared_field_type(self, entries: dict[str, object], name: str) -> str | None:
        value = entries.get(name)
        if not isinstance(value, str):
            return None
        base = value.removesuffix("[]")
        if base in self.models or base in self.scalars or base in {
            "string", "boolean", "integer", "object", "u8", "u16", "u32", "u64", "i32",
            "Amount", "Sequence", "BudgetLimit", "TimestampSeconds",
        }:
            return value
        return None

    def java_type(self, declared: str | None, optional: bool = False) -> str:
        if declared is None or "<" in declared:
            return "JsonNode"
        if declared.endswith("[]"):
            return f"List<{self.java_type(declared[:-2], True)}>"
        if declared in {"Amount", "BudgetLimit"}:
            return "ProtocolAmount"
        if declared in {"Sequence", "TimestampSeconds", "u64"}:
            return "BigInteger"
        if declared in {"integer", "u8", "u16", "u32", "i32"}:
            return "Long" if optional else "long"
        if declared == "boolean":
            return "Boolean" if optional else "boolean"
        if declared == "object":
            return "JsonNode"
        if declared in self.models:
            return f"{self.name}Models.{java_name(declared)}"
        return "String"


def operation_specs(plane: Plane) -> list[dict[str, object]]:
    mutations = {key.split(".", 1)[1] for key in plane.values if key.startswith("mutation.")}
    result = []
    for section, entries in sorted(plane.values.items()):
        if not section.startswith("operation."):
            continue
        wire = section.split(".", 1)[1]
        request_model = entries.get("request") if isinstance(entries.get("request"), str) else None
        response_model = entries.get("response") if isinstance(entries.get("response"), str) else None
        request_fallback = string_list(entries.get("request_required")) or string_list(entries.get("required"))
        response_fallback = string_list(entries.get("response_required"))
        result.append({
            "wire": wire,
            "name": java_name(wire),
            "request_fields": plane.fields(request_model, request_fallback),
            "response_fields": plane.fields(response_model, response_fallback),
            "response_model": response_model,
            "idempotency": bool(entries.get("idempotency")) or wire in mutations
                or "idempotency_key" in request_fallback,
        })
    return result


def record_source(name: str, fields: list[tuple[str, str | None, bool]], plane: Plane,
                  marker: str, indent: str, delegating: bool = False) -> list[str]:
    parameters = []
    for wire, declared, optional in fields:
        annotation = f'@JsonProperty("{wire}") ' if field_name(wire) != wire else ""
        parameters.append(f"{annotation}{plane.java_type(declared, optional)} {field_name(wire)}")
    joined = ", ".join(parameters)
    lines = [f"{indent}public record {name}({joined}) implements SchemaTypes.{marker} {{"]
    validations = []
    for wire, declared, optional in fields:
        field = field_name(wire)
        java_type = plane.java_type(declared, optional)
        if optional:
            if java_type.startswith("List<"):
                validations.append(f"if ({field} != null) {field} = List.copyOf({field});")
            continue
        if java_type.startswith("List<"):
            validations.append(f"{field} = List.copyOf(Objects.requireNonNull({field}, \"{wire}\"));")
        elif java_type not in {"long", "boolean"}:
            validations.append(f"Objects.requireNonNull({field}, \"{wire}\");")
        if declared in {"Sequence", "TimestampSeconds", "u64"}:
            validations.append(f"SchemaTypes.protocolU64({field});")
    if validations:
        if delegating:
            lines.append(f"{indent}    @JsonCreator(mode = JsonCreator.Mode.DELEGATING)")
        lines.append(f"{indent}    public {name} {{")
        lines.extend(f"{indent}        {validation}" for validation in validations)
        lines.append(f"{indent}    }}")
    if delegating:
        wire, declared, optional = fields[0]
        lines.append(f"{indent}    @JsonValue public {plane.java_type(declared, optional)} wireValue() {{ return {field_name(wire)}; }}")
    lines.append(f"{indent}}}")
    return lines


def render_models(plane: Plane) -> list[str]:
    lines = [f"    public static final class {plane.name}Models {{", f"        private {plane.name}Models() {{}}"]
    for model, entries in sorted(plane.models.items()):
        name = java_name(model)
        variants = string_list(entries.get("variants"))
        payload_variant = any(f"{variant}.required" in entries for variant in variants)
        if variants and not payload_variant:
            lines.append(f"        public enum {name} {{")
            for index, variant in enumerate(variants):
                suffix = ";" if index + 1 == len(variants) else ","
                lines.append(f'            {constant_name(variant)}("{variant}"){suffix}')
            lines.extend([
                "            private final String wire;",
                f"            {name}(String wire) {{ this.wire = wire; }}",
                "            @JsonValue public String wire() { return wire; }",
                f"            @JsonCreator public static {name} fromWire(String wire) {{",
                f"                for ({name} value : values()) if (value.wire.equals(wire)) return value;",
                "                throw new IllegalArgumentException(\"unknown schema variant\");",
                "            }",
                "        }",
            ])
            continue
        fields = plane.fields(model, [])
        if variants:
            allowed = ", ".join(f'"{variant}"' for variant in variants)
            marker = "GeneratedEvent" if "Event" in name else "GeneratedResponse"
            lines.append(f"        public record {name}(String kind, JsonNode value) implements SchemaTypes.{marker} {{")
            lines.append(f"            private static final Set<String> KINDS = Set.of({allowed});")
            lines.append(f"            public {name} {{")
            lines.append("                Objects.requireNonNull(kind, \"kind\");")
            lines.append("                Objects.requireNonNull(value, \"value\");")
            lines.append("                if (!KINDS.contains(kind)) throw PlatformSdkException.invalidArgument();")
            lines.append("            }")
            lines.append("        }")
        elif fields:
            marker = "GeneratedEvent" if "Event" in name else "GeneratedResponse"
            lines.extend(record_source(name, fields, plane, marker, "        "))
    lines.append("    }")
    return lines


def render_operations(plane: Plane, specs: list[dict[str, object]]) -> list[str]:
    lines = [f"    public static final class {plane.name}Operations {{", f"        private {plane.name}Operations() {{}}"]
    for spec in specs:
        request = f"{spec['name']}Request"
        response = f"{spec['name']}Response"
        lines.extend(record_source(request, spec["request_fields"], plane, "GeneratedRequest", "        "))
        response_fields = spec["response_fields"]
        delegating = not response_fields
        if delegating:
            response_model = spec["response_model"]
            declared = response_model if isinstance(response_model, str) and response_model in plane.models else "object"
            response_fields = [("value", declared, False)]
        lines.extend(record_source(response, response_fields, plane, "GeneratedResponse", "        ", delegating))
        lines.append(
            f"        public static final SchemaTypes.TypedOperation<{request}, {response}> {constant_name(spec['wire'])} = "
            f"new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.{plane.name.upper()}, \"{spec['wire']}\", "
            f"{str(spec['idempotency']).lower()}, {request}.class, {response}.class);"
        )
    lines.append("    }")
    return lines


def generate(repo: Path) -> str:
    agent = Plane("Agent", sections(repo / "agent/schema/agent-api"))
    human = Plane("Human", sections(repo / "human/schema/human-api"))
    output = [
        "// Code generated from the LayerX Agent API and Human API schemas. DO NOT EDIT.",
        "", "package com.sidiora.layerx.sdk;", "",
        "import com.fasterxml.jackson.annotation.JsonCreator;",
        "import com.fasterxml.jackson.annotation.JsonProperty;",
        "import com.fasterxml.jackson.annotation.JsonValue;",
        "import com.fasterxml.jackson.databind.JsonNode;",
        "import java.math.BigInteger;", "import java.util.List;", "import java.util.Map;",
        "import java.util.Objects;", "import java.util.Set;", "", "public final class GeneratedSchema {",
        "    private GeneratedSchema() {}",
    ]
    output.extend(render_models(agent))
    output.extend(render_models(human))
    agent_specs, human_specs = operation_specs(agent), operation_specs(human)
    output.extend(render_operations(agent, agent_specs))
    output.extend(render_operations(human, human_specs))
    output.append("    public static final Map<String, SchemaTypes.TypedOperation<?, ?>> AGENT = Map.ofEntries(")
    for index, spec in enumerate(agent_specs):
        suffix = ");" if index + 1 == len(agent_specs) else ","
        output.append(f'        Map.entry("{spec["wire"]}", AgentOperations.{constant_name(spec["wire"])} ){suffix}'.replace(" }", "}"))
    output.append("    public static final Map<String, SchemaTypes.TypedOperation<?, ?>> HUMAN = Map.ofEntries(")
    for index, spec in enumerate(human_specs):
        suffix = ");" if index + 1 == len(human_specs) else ","
        output.append(f'        Map.entry("{spec["wire"]}", HumanOperations.{constant_name(spec["wire"])} ){suffix}'.replace(" }", "}"))
    output.extend(["}", ""])
    return "\n".join(output)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("repo", nargs="?", default=".")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--stdout", action="store_true")
    arguments = parser.parse_args()
    repo = Path(arguments.repo).resolve()
    destination = repo / "platform/sdk/jvm/src/main/java/com/sidiora/layerx/sdk/GeneratedSchema.java"
    expected = generate(repo)
    if arguments.stdout:
        print(expected, end="")
        return 0
    if arguments.check:
        if not destination.is_file() or destination.read_text(encoding="utf-8") != expected:
            raise SystemExit("generated JVM schema is stale; run make platform-sdk-generate")
        return 0
    destination.write_text(expected, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
