use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

pub const REFRESH_COMMAND: &str = "cargo run --manifest-path human/tools/api-gen/Cargo.toml --locked -- human/schema/human-api human/apps/web/src/api/generated";

pub const GENERATED_FILES: &[&str] = &["conformance.ts", "index.ts"];

const PRIMITIVES: &[&str] = &["string", "boolean", "integer", "object"];

const METHODS: &[&str] = &["DELETE", "GET", "PATCH", "POST", "PUT"];

#[derive(Debug, Eq, PartialEq)]
pub struct Violation {
    pub path: PathBuf,
    pub rule: &'static str,
    pub detail: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct GeneratedClient {
    pub files: BTreeMap<String, String>,
    pub operations: usize,
    pub types: usize,
}

type Sections = BTreeMap<String, BTreeMap<String, String>>;

fn parse_kvx(source: &str) -> Result<Sections, String> {
    let mut sections: Sections = BTreeMap::new();
    let mut current = String::new();
    for (index, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            name.clone_into(&mut current);
            sections.entry(current.clone()).or_default();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("line {}: not a key/value declaration", index + 1));
        };
        if current.is_empty() {
            return Err(format!("line {}: declaration outside a section", index + 1));
        }
        let entry = sections.entry(current.clone()).or_default();
        if entry
            .insert(key.trim().to_owned(), value.trim().to_owned())
            .is_some()
        {
            return Err(format!(
                "line {}: duplicate declaration {current}.{}",
                index + 1,
                key.trim()
            ));
        }
    }
    Ok(sections)
}

fn unquote(value: &str) -> Option<&str> {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
}

fn parse_list(value: &str) -> Option<Vec<String>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.is_empty() {
        return Some(Vec::new());
    }
    let mut items = Vec::new();
    let mut rest = inner;
    loop {
        let after_open = rest.trim_start().strip_prefix('"')?;
        let end = after_open.find('"')?;
        items.push(after_open.get(..end)?.to_owned());
        let tail = after_open.get(end + 1..)?.trim_start();
        if tail.is_empty() {
            return Some(items);
        }
        rest = tail.strip_prefix(',')?;
    }
}

fn violation(path: &Path, rule: &'static str, detail: impl Into<String>) -> Violation {
    Violation {
        path: path.to_path_buf(),
        rule,
        detail: detail.into(),
    }
}

struct SchemaFile {
    path: PathBuf,
    sections: Sections,
}

struct Header {
    major: u32,
    minor: u32,
    includes: Vec<String>,
}

struct Scalar {
    typescript: String,
}

struct EnumDef {
    variants: Vec<String>,
}

#[derive(Clone)]
struct Field {
    name: String,
    type_name: String,
    array: bool,
}

struct Record {
    required: Vec<Field>,
    optional: Vec<Field>,
    origin: PathBuf,
}

struct Operation {
    name: String,
    method: String,
    path: String,
    request: String,
    response: String,
    idempotency: bool,
    origin: PathBuf,
}

#[derive(Default)]
struct Model {
    scalars: BTreeMap<String, Scalar>,
    enums: BTreeMap<String, EnumDef>,
    records: BTreeMap<String, Record>,
    operations: Vec<Operation>,
}

fn load_kvx(root: &Path, name: &str, violations: &mut Vec<Violation>) -> Option<SchemaFile> {
    let path = root.join(name);
    let Ok(body) = fs::read_to_string(&path) else {
        violations.push(violation(
            &path,
            "missing-schema-file",
            format!("cannot read {name}"),
        ));
        return None;
    };
    match parse_kvx(&body) {
        Ok(sections) => Some(SchemaFile { path, sections }),
        Err(detail) => {
            violations.push(violation(&path, "invalid-kvx", detail));
            None
        }
    }
}

fn quoted<'entries>(
    entries: &'entries BTreeMap<String, String>,
    key: &str,
) -> Option<&'entries str> {
    entries.get(key).map(String::as_str).and_then(unquote)
}

fn read_header(file: &SchemaFile, violations: &mut Vec<Violation>) -> Header {
    let mut header = Header {
        major: 0,
        minor: 0,
        includes: Vec::new(),
    };
    let Some(schema) = file.sections.get("schema") else {
        violations.push(violation(
            &file.path,
            "missing-schema-section",
            "v1.kvx declares no [schema] section",
        ));
        return header;
    };
    match schema
        .get("major")
        .and_then(|value| value.parse::<u32>().ok())
    {
        Some(major) if major >= 1 => header.major = major,
        _ => violations.push(violation(
            &file.path,
            "missing-schema-version",
            "schema.major must be an explicit integer of at least 1",
        )),
    }
    match schema
        .get("minor")
        .and_then(|value| value.parse::<u32>().ok())
    {
        Some(minor) => header.minor = minor,
        None => violations.push(violation(
            &file.path,
            "missing-schema-version",
            "schema.minor must be an explicit integer",
        )),
    }
    match schema
        .get("includes")
        .map(String::as_str)
        .and_then(parse_list)
    {
        Some(includes) => header.includes = includes,
        None => violations.push(violation(
            &file.path,
            "missing-includes",
            "schema.includes must be a list of module files",
        )),
    }
    header
}

fn parse_field(spec: &str) -> Option<Field> {
    let (name, type_spec) = spec.split_once(':')?;
    let name = name.trim();
    let type_spec = type_spec.trim();
    if name.is_empty() || type_spec.is_empty() {
        return None;
    }
    let (type_name, array) = type_spec
        .strip_suffix("[]")
        .map_or((type_spec, false), |inner| (inner, true));
    if type_name.is_empty() {
        return None;
    }
    Some(Field {
        name: name.to_owned(),
        type_name: type_name.to_owned(),
        array,
    })
}

fn parse_fields(
    file: &SchemaFile,
    section: &str,
    value: &str,
    violations: &mut Vec<Violation>,
) -> Vec<Field> {
    let Some(specs) = parse_list(value) else {
        violations.push(violation(
            &file.path,
            "invalid-field-declaration",
            format!("{section} must declare its fields as a list"),
        ));
        return Vec::new();
    };
    let mut fields = Vec::new();
    for spec in specs {
        match parse_field(&spec) {
            Some(field) => fields.push(field),
            None => violations.push(violation(
                &file.path,
                "invalid-field-declaration",
                format!("{section} field {spec} must be name:Type"),
            )),
        }
    }
    fields
}

fn name_is_free(model: &Model, name: &str) -> bool {
    !model.scalars.contains_key(name)
        && !model.enums.contains_key(name)
        && !model.records.contains_key(name)
}

fn collect_scalar(
    file: &SchemaFile,
    name: &str,
    entries: &BTreeMap<String, String>,
    model: &mut Model,
    violations: &mut Vec<Violation>,
) {
    if quoted(entries, "json") != Some("string") {
        violations.push(violation(
            &file.path,
            "invalid-scalar-declaration",
            format!("scalar.{name} must declare json = \"string\""),
        ));
        return;
    }
    let Some(typescript @ ("string" | "bigint")) = quoted(entries, "typescript") else {
        violations.push(violation(
            &file.path,
            "invalid-scalar-declaration",
            format!("scalar.{name} must map to a TypeScript string or bigint"),
        ));
        return;
    };
    if name_is_free(model, name) {
        model.scalars.insert(
            name.to_owned(),
            Scalar {
                typescript: typescript.to_owned(),
            },
        );
    } else {
        violations.push(violation(
            &file.path,
            "duplicate-declaration",
            format!("{name} is declared more than once"),
        ));
    }
}

fn collect_enum(
    file: &SchemaFile,
    name: &str,
    variants_value: &str,
    model: &mut Model,
    violations: &mut Vec<Violation>,
) {
    let variants = parse_list(variants_value).unwrap_or_default();
    if variants.is_empty() || variants.iter().any(String::is_empty) {
        violations.push(violation(
            &file.path,
            "invalid-enum-declaration",
            format!("type.{name} variants must be a non-empty list of non-empty strings"),
        ));
        return;
    }
    if name_is_free(model, name) {
        model.enums.insert(name.to_owned(), EnumDef { variants });
    } else {
        violations.push(violation(
            &file.path,
            "duplicate-declaration",
            format!("{name} is declared more than once"),
        ));
    }
}

fn collect_record(
    file: &SchemaFile,
    section: &str,
    name: &str,
    entries: &BTreeMap<String, String>,
    fields_key: &str,
    model: &mut Model,
    violations: &mut Vec<Violation>,
) {
    let Some(fields_value) = entries.get(fields_key) else {
        violations.push(violation(
            &file.path,
            "invalid-record-declaration",
            format!("{section} must declare {fields_key}"),
        ));
        return;
    };
    let required = parse_fields(file, section, fields_value, violations);
    let optional = entries
        .get("optional")
        .map(|value| parse_fields(file, section, value, violations))
        .unwrap_or_default();
    if name_is_free(model, name) {
        model.records.insert(
            name.to_owned(),
            Record {
                required,
                optional,
                origin: file.path.clone(),
            },
        );
    } else {
        violations.push(violation(
            &file.path,
            "duplicate-declaration",
            format!("{name} is declared more than once"),
        ));
    }
}

fn collect_operation(
    file: &SchemaFile,
    name: &str,
    entries: &BTreeMap<String, String>,
    model: &mut Model,
    violations: &mut Vec<Violation>,
) {
    let section = format!("operation.{name}");
    let method = quoted(entries, "method").unwrap_or_default().to_owned();
    let path = quoted(entries, "path").unwrap_or_default().to_owned();
    let request = quoted(entries, "request").unwrap_or_default().to_owned();
    let response = quoted(entries, "response").unwrap_or_default().to_owned();
    if !METHODS.contains(&method.as_str()) {
        violations.push(violation(
            &file.path,
            "invalid-operation-declaration",
            format!("{section} must declare a method from {METHODS:?}"),
        ));
        return;
    }
    if path.is_empty() || request.is_empty() || response.is_empty() {
        violations.push(violation(
            &file.path,
            "invalid-operation-declaration",
            format!("{section} must declare path, request and response"),
        ));
        return;
    }
    let idempotency = match entries.get("idempotency").map(String::as_str) {
        None | Some("false") => false,
        Some("true") => true,
        Some(other) => {
            violations.push(violation(
                &file.path,
                "invalid-operation-declaration",
                format!("{section} idempotency must be true or false, not {other}"),
            ));
            false
        }
    };
    if model
        .operations
        .iter()
        .any(|existing| existing.name == name)
    {
        violations.push(violation(
            &file.path,
            "duplicate-declaration",
            format!("{section} is declared more than once"),
        ));
        return;
    }
    model.operations.push(Operation {
        name: name.to_owned(),
        method,
        path,
        request,
        response,
        idempotency,
        origin: file.path.clone(),
    });
}

fn collect_declarations(files: &[SchemaFile], violations: &mut Vec<Violation>) -> Model {
    let mut model = Model::default();
    for file in files {
        for (section, entries) in &file.sections {
            if let Some(name) = section.strip_prefix("scalar.") {
                collect_scalar(file, name, entries, &mut model, violations);
            } else if let Some(name) = section.strip_prefix("type.") {
                if let Some(variants) = entries.get("variants") {
                    if entries.contains_key("required") {
                        violations.push(violation(
                            &file.path,
                            "invalid-type-declaration",
                            format!("{section} declares both variants and required"),
                        ));
                    } else {
                        collect_enum(file, name, variants, &mut model, violations);
                    }
                } else if entries.contains_key("required") {
                    collect_record(
                        file, section, name, entries, "required", &mut model, violations,
                    );
                } else {
                    violations.push(violation(
                        &file.path,
                        "invalid-type-declaration",
                        format!("{section} must declare variants or required"),
                    ));
                }
            } else if let Some(name) = section.strip_prefix("record.") {
                collect_record(
                    file, section, name, entries, "fields", &mut model, violations,
                );
            } else if let Some(name) = section.strip_prefix("operation.") {
                collect_operation(file, name, entries, &mut model, violations);
            }
        }
    }
    model
        .operations
        .sort_by(|left, right| left.name.cmp(&right.name));
    model
}

fn type_exists(model: &Model, name: &str) -> bool {
    PRIMITIVES.contains(&name)
        || model.scalars.contains_key(name)
        || model.enums.contains_key(name)
        || model.records.contains_key(name)
}

fn check_type_references(model: &Model, violations: &mut Vec<Violation>) {
    for (name, record) in &model.records {
        for field in record.required.iter().chain(&record.optional) {
            if !type_exists(model, &field.type_name) {
                violations.push(violation(
                    &record.origin,
                    "unresolved-type",
                    format!(
                        "{name}.{} references undeclared type {}",
                        field.name, field.type_name
                    ),
                ));
            }
        }
    }
}

fn parse_path(path: &str) -> Option<Vec<PathSegment>> {
    let rest = path.strip_prefix('/')?;
    let mut segments = Vec::new();
    let mut params = BTreeSet::new();
    for segment in rest.split('/') {
        if segment.is_empty() {
            return None;
        }
        if let Some(inner) = segment.strip_prefix('{') {
            let name = inner.strip_suffix('}')?;
            let named = !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
            if !named || !params.insert(name.to_owned()) {
                return None;
            }
            segments.push(PathSegment::Parameter(name.to_owned()));
        } else if segment.contains(['{', '}']) {
            return None;
        } else {
            segments.push(PathSegment::Literal(segment.to_owned()));
        }
    }
    Some(segments)
}

enum PathSegment {
    Literal(String),
    Parameter(String),
}

struct PlannedOperation<'model> {
    operation: &'model Operation,
    method_name: String,
    path_params: Vec<String>,
    path_expression: String,
    bodyless: bool,
}

fn camel_name(name: &str) -> String {
    let mut out = String::new();
    for (index, segment) in name.split(['.', '-', '_']).enumerate() {
        if index == 0 {
            out.push_str(segment);
        } else {
            let mut chars = segment.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
    }
    out
}

fn lower_first(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().collect::<String>() + chars.as_str()
    })
}

fn path_expression(segments: &[PathSegment]) -> String {
    let mut parts = Vec::new();
    let mut literal = String::new();
    for segment in segments {
        match segment {
            PathSegment::Literal(text) => {
                literal.push('/');
                literal.push_str(text);
            }
            PathSegment::Parameter(name) => {
                literal.push('/');
                parts.push(format!("\"{literal}\""));
                literal.clear();
                parts.push(format!("encodeURIComponent({name})"));
            }
        }
    }
    if !literal.is_empty() || parts.is_empty() {
        parts.push(format!("\"{literal}\""));
    }
    parts.join(" + ")
}

fn request_is_bodyless(model: &Model, type_name: &str) -> bool {
    model
        .records
        .get(type_name)
        .is_some_and(|record| record.required.is_empty() && record.optional.is_empty())
}

fn plan_operations<'model>(
    model: &'model Model,
    violations: &mut Vec<Violation>,
) -> Vec<PlannedOperation<'model>> {
    let mut planned = Vec::new();
    let mut method_names: BTreeMap<String, String> = BTreeMap::new();
    for operation in &model.operations {
        for type_name in [&operation.request, &operation.response] {
            if !model.records.contains_key(type_name) {
                violations.push(violation(
                    &operation.origin,
                    "unsupported-operation-shape",
                    format!(
                        "operation.{} must declare record request and response shapes, not {type_name}",
                        operation.name
                    ),
                ));
            }
        }
        let Some(segments) = parse_path(&operation.path) else {
            violations.push(violation(
                &operation.origin,
                "invalid-operation-path",
                format!(
                    "operation.{} path {} has a malformed segment",
                    operation.name, operation.path
                ),
            ));
            continue;
        };
        let method_name = camel_name(&operation.name);
        if let Some(taken) = method_names.insert(method_name.clone(), operation.name.clone()) {
            violations.push(violation(
                &operation.origin,
                "colliding-method-name",
                format!(
                    "operation.{} and operation.{taken} both generate {method_name}",
                    operation.name
                ),
            ));
            continue;
        }
        let path_params = segments
            .iter()
            .filter_map(|segment| match segment {
                PathSegment::Parameter(name) => Some(name.clone()),
                PathSegment::Literal(_) => None,
            })
            .collect();
        planned.push(PlannedOperation {
            operation,
            method_name,
            path_params,
            path_expression: path_expression(&segments),
            bodyless: request_is_bodyless(model, &operation.request),
        });
    }
    planned
}

fn ts_base_type(type_name: &str) -> &str {
    match type_name {
        "integer" => "number",
        "object" => "JsonObject",
        other => other,
    }
}

fn ts_field_type(field: &Field) -> String {
    let base = ts_base_type(&field.type_name);
    if field.array {
        format!("{base}[]")
    } else {
        base.to_owned()
    }
}

fn item_decoder(model: &Model, type_name: &str) -> String {
    match type_name {
        "string" => "expectString".to_owned(),
        "boolean" => "expectBoolean".to_owned(),
        "integer" => "expectInteger".to_owned(),
        "object" => "expectObject".to_owned(),
        other => {
            if let Some(scalar) = model.scalars.get(other) {
                if scalar.typescript == "bigint" {
                    "decodeConsensusInteger".to_owned()
                } else {
                    "expectString".to_owned()
                }
            } else {
                format!("decode{other}")
            }
        }
    }
}

fn decode_expression(model: &Model, field: &Field, value: &str, at: &str) -> String {
    let decoder = item_decoder(model, &field.type_name);
    if field.array {
        format!("decodeArray({value}, {at}, {decoder})")
    } else {
        format!("{decoder}({value}, {at})")
    }
}

fn encode_expression(model: &Model, field: &Field, access: &str) -> String {
    let element = if model.records.contains_key(&field.type_name) {
        Some(format!("encode{}", field.type_name))
    } else if model
        .scalars
        .get(&field.type_name)
        .is_some_and(|scalar| scalar.typescript == "bigint")
    {
        Some("encodeConsensusInteger".to_owned())
    } else {
        None
    };
    match (field.array, element) {
        (true, Some(mapper)) => format!("{access}.map({mapper})"),
        (false, Some(mapper)) => format!("{mapper}({access})"),
        (_, None) => access.to_owned(),
    }
}

const BANNER: &str = "// GENERATED by layerx-human-api-gen from human/schema/human-api. DO NOT EDIT BY HAND.\n// Regenerate with: cargo run --manifest-path human/tools/api-gen/Cargo.toml --locked -- human/schema/human-api human/apps/web/src/api/generated\n";

const RUNTIME: &str = r#"
export type JsonValue = null | boolean | number | string | JsonValue[] | JsonObject;

export interface JsonObject {
  [name: string]: JsonValue;
}

export class HumanApiDecodeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "HumanApiDecodeError";
  }
}

export function requireValue(value: JsonValue | undefined, at: string): JsonValue {
  if (value === undefined) {
    throw new HumanApiDecodeError(at + " is missing");
  }
  return value;
}

export function expectObject(value: JsonValue | undefined, at: string): JsonObject {
  const present = requireValue(value, at);
  if (present === null || typeof present !== "object" || Array.isArray(present)) {
    throw new HumanApiDecodeError(at + " must be a JSON object");
  }
  return present;
}

export function expectArray(value: JsonValue | undefined, at: string): JsonValue[] {
  const present = requireValue(value, at);
  if (!Array.isArray(present)) {
    throw new HumanApiDecodeError(at + " must be a JSON array");
  }
  return present;
}

export function expectString(value: JsonValue | undefined, at: string): string {
  const present = requireValue(value, at);
  if (typeof present !== "string") {
    throw new HumanApiDecodeError(at + " must be a JSON string");
  }
  return present;
}

export function expectBoolean(value: JsonValue | undefined, at: string): boolean {
  const present = requireValue(value, at);
  if (typeof present !== "boolean") {
    throw new HumanApiDecodeError(at + " must be a JSON boolean");
  }
  return present;
}

export function expectInteger(value: JsonValue | undefined, at: string): number {
  const present = requireValue(value, at);
  if (typeof present !== "number" || !Number.isSafeInteger(present)) {
    throw new HumanApiDecodeError(at + " must be a JSON integer");
  }
  return present;
}

export function decodeArray<Item>(
  value: JsonValue | undefined,
  at: string,
  item: (value: JsonValue | undefined, at: string) => Item,
): Item[] {
  return expectArray(value, at).map((entry, index) => item(entry, at + "[" + String(index) + "]"));
}

const consensusIntegerShape = /^[0-9]+$/;

export function decodeConsensusInteger(value: JsonValue | undefined, at: string): bigint {
  const text = expectString(value, at);
  if (!consensusIntegerShape.test(text)) {
    throw new HumanApiDecodeError(at + " must be a decimal string of base units");
  }
  return BigInt(text);
}

export function encodeConsensusInteger(value: bigint): string {
  if (value < 0n) {
    throw new HumanApiDecodeError("a consensus integer is never negative");
  }
  return value.toString(10);
}
"#;

const ERROR_CLASS: &str = r#"
export class HumanApiError extends Error {
  readonly status: number;
  readonly trace: TraceId;
  readonly detail: ApiError;

  constructor(status: number, trace: TraceId, detail: ApiError) {
    super(detail.code + ": " + detail.copy_key);
    this.name = "HumanApiError";
    this.status = status;
    this.trace = trace;
    this.detail = detail;
  }
}
"#;

fn emit_scalars(out: &mut String, model: &Model) {
    for (name, scalar) in &model.scalars {
        let _ = writeln!(out, "\nexport type {name} = {};", scalar.typescript);
    }
}

fn emit_enums(out: &mut String, model: &Model) {
    for (name, definition) in &model.enums {
        let union = definition
            .variants
            .iter()
            .map(|variant| format!("\"{variant}\""))
            .collect::<Vec<_>>()
            .join(" | ");
        let list = definition
            .variants
            .iter()
            .map(|variant| format!("\"{variant}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let variants_const = format!("{}Variants", lower_first(name));
        let _ = writeln!(out, "\nexport type {name} = {union};");
        let _ = writeln!(
            out,
            "\nexport const {variants_const}: readonly {name}[] = [{list}];"
        );
        let _ = writeln!(
            out,
            "\nexport function decode{name}(value: JsonValue | undefined, at: string): {name} {{\n  const text = expectString(value, at);\n  for (const variant of {variants_const}) {{\n    if (variant === text) {{\n      return variant;\n    }}\n  }}\n  throw new HumanApiDecodeError(at + \" must be a declared {name} variant\");\n}}"
        );
    }
}

fn emit_record_interface(out: &mut String, name: &str, record: &Record) {
    if record.required.is_empty() && record.optional.is_empty() {
        let _ = writeln!(out, "\nexport type {name} = Record<string, never>;");
        return;
    }
    let _ = writeln!(out, "\nexport interface {name} {{");
    for field in &record.required {
        let _ = writeln!(out, "  {}: {};", field.name, ts_field_type(field));
    }
    for field in &record.optional {
        let _ = writeln!(out, "  {}?: {};", field.name, ts_field_type(field));
    }
    out.push_str("}\n");
}

fn emit_record_decode(out: &mut String, model: &Model, name: &str, record: &Record) {
    let _ = writeln!(
        out,
        "\nexport function decode{name}(value: JsonValue | undefined, at: string): {name} {{"
    );
    if record.required.is_empty() && record.optional.is_empty() {
        out.push_str("  expectObject(value, at);\n  return {};\n}\n");
        return;
    }
    out.push_str("  const object = expectObject(value, at);\n");
    if record.required.is_empty() {
        let _ = writeln!(out, "  const result: {name} = {{}};");
    } else {
        let _ = writeln!(out, "  const result: {name} = {{");
        for field in &record.required {
            let value = format!("object[\"{}\"]", field.name);
            let at = format!("at + \".{}\"", field.name);
            let _ = writeln!(
                out,
                "    {}: {},",
                field.name,
                decode_expression(model, field, &value, &at)
            );
        }
        out.push_str("  };\n");
    }
    for field in &record.optional {
        let value = format!("object[\"{}\"]", field.name);
        let at = format!("at + \".{}\"", field.name);
        let _ = writeln!(
            out,
            "  if ({value} !== undefined) {{\n    result.{} = {};\n  }}",
            field.name,
            decode_expression(model, field, &value, &at)
        );
    }
    out.push_str("  return result;\n}\n");
}

fn emit_record_encode(out: &mut String, model: &Model, name: &str, record: &Record) {
    let _ = writeln!(
        out,
        "\nexport function encode{name}(value: {name}): JsonValue {{"
    );
    if record.required.is_empty() && record.optional.is_empty() {
        out.push_str("  return { ...value };\n}\n");
        return;
    }
    if record.required.is_empty() {
        out.push_str("  const result: JsonObject = {};\n");
    } else {
        out.push_str("  const result: JsonObject = {\n");
        for field in &record.required {
            let access = format!("value.{}", field.name);
            let _ = writeln!(
                out,
                "    {}: {},",
                field.name,
                encode_expression(model, field, &access)
            );
        }
        out.push_str("  };\n");
    }
    for field in &record.optional {
        let access = format!("value.{}", field.name);
        let _ = writeln!(
            out,
            "  if ({access} !== undefined) {{\n    result[\"{}\"] = {};\n  }}",
            field.name,
            encode_expression(model, field, &access)
        );
    }
    out.push_str("  return result;\n}\n");
}

fn emit_manifest(out: &mut String, planned: &[PlannedOperation]) {
    out.push_str(
        "\nexport type HttpMethod = \"DELETE\" | \"GET\" | \"PATCH\" | \"POST\" | \"PUT\";\n",
    );
    out.push_str("\nexport interface OperationShape {\n  readonly method: HttpMethod;\n  readonly path: string;\n  readonly pathParams: readonly string[];\n  readonly request: string;\n  readonly response: string;\n  readonly idempotency: boolean;\n  readonly bodyless: boolean;\n}\n");
    out.push_str("\nexport const operationNames = [\n");
    for plan in planned {
        let _ = writeln!(out, "  \"{}\",", plan.operation.name);
    }
    out.push_str("] as const;\n");
    out.push_str("\nexport type OperationName = (typeof operationNames)[number];\n");
    out.push_str(
        "\nexport const operations: { readonly [name in OperationName]: OperationShape } = {\n",
    );
    for plan in planned {
        let params = plan
            .path_params
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            out,
            "  \"{}\": {{ method: \"{}\", path: \"{}\", pathParams: [{params}], request: \"{}\", response: \"{}\", idempotency: {}, bodyless: {} }},",
            plan.operation.name,
            plan.operation.method,
            plan.operation.path,
            plan.operation.request,
            plan.operation.response,
            plan.operation.idempotency,
            plan.bodyless
        );
    }
    out.push_str("};\n");
}

fn method_signature(plan: &PlannedOperation) -> String {
    let mut parameters = Vec::new();
    for name in &plan.path_params {
        parameters.push(format!("{name}: string"));
    }
    if !plan.bodyless {
        parameters.push(format!("request: {}", plan.operation.request));
    }
    if plan.operation.idempotency {
        parameters.push("idempotencyKey: string".to_owned());
    }
    format!(
        "{}({}): Promise<{}>",
        plan.method_name,
        parameters.join(", "),
        plan.operation.response
    )
}

fn emit_client(out: &mut String, planned: &[PlannedOperation]) {
    out.push_str(
        "\nexport type FetchLike = (input: string, init: RequestInit) => Promise<Response>;\n",
    );
    out.push_str("\nexport interface HumanApiClientOptions {\n  readonly baseUrl?: string;\n  readonly fetch?: FetchLike;\n  readonly headers?: { readonly [name: string]: string };\n  readonly credentials?: RequestCredentials;\n  readonly csrfToken?: () => string | undefined;\n  readonly trace?: () => string | undefined;\n}\n");
    out.push_str("\nexport interface HumanApiClient {\n");
    for plan in planned {
        let _ = writeln!(out, "  {};", method_signature(plan));
    }
    out.push_str("}\n");
    out.push_str("\nexport function createHumanApiClient(options: HumanApiClientOptions = {}): HumanApiClient {\n");
    out.push_str("  const baseUrl = options.baseUrl ?? \"\";\n");
    out.push_str("  const transport: FetchLike = options.fetch ?? ((input, init) => globalThis.fetch(input, init));\n");
    out.push_str("  const baseHeaders = options.headers ?? {};\n");
    out.push_str("  const credentials = options.credentials ?? \"include\";\n");
    out.push_str("  async function execute(\n    method: HttpMethod,\n    path: string,\n    body: JsonValue | undefined,\n    idempotencyKey: string | undefined,\n  ): Promise<JsonValue> {\n");
    out.push_str("    const headers: { [name: string]: string } = { ...baseHeaders, Accept: \"application/json\" };\n");
    out.push_str("    if (body !== undefined) {\n      headers[\"Content-Type\"] = \"application/json\";\n    }\n");
    out.push_str("    if (idempotencyKey !== undefined) {\n      headers[\"Idempotency-Key\"] = idempotencyKey;\n    }\n");
    out.push_str("    const csrfToken = options.csrfToken?.();\n");
    out.push_str("    if (method !== \"GET\" && csrfToken !== undefined) {\n      headers[\"X-LayerX-CSRF\"] = csrfToken;\n    }\n");
    out.push_str("    const outboundTrace = options.trace?.();\n");
    out.push_str("    if (outboundTrace !== undefined) {\n      headers[\"X-LayerX-Trace\"] = outboundTrace;\n    }\n");
    out.push_str("    const init: RequestInit = { method, headers, credentials };\n");
    out.push_str("    if (body !== undefined) {\n      init.body = JSON.stringify(body);\n    }\n");
    out.push_str("    const response = await transport(baseUrl + path, init);\n");
    out.push_str("    let parsed: JsonValue;\n    try {\n      parsed = (await response.json()) as JsonValue;\n    } catch {\n      throw new HumanApiDecodeError(\"the response body is not JSON\");\n    }\n");
    out.push_str("    const envelope = expectObject(parsed, \"response\");\n");
    out.push_str("    const ok = expectBoolean(envelope[\"ok\"], \"response.ok\");\n");
    out.push_str("    const trace = expectString(envelope[\"trace\"], \"response.trace\");\n");
    out.push_str("    if (!ok) {\n      throw new HumanApiError(response.status, trace, decodeApiError(envelope[\"error\"], \"response.error\"));\n    }\n");
    out.push_str("    return requireValue(envelope[\"result\"], \"response.result\");\n  }\n");
    out.push_str("  const client: HumanApiClient = {\n");
    for plan in planned {
        let mut arguments = plan.path_params.clone();
        if !plan.bodyless {
            arguments.push("request".to_owned());
        }
        if plan.operation.idempotency {
            arguments.push("idempotencyKey".to_owned());
        }
        let body = if plan.bodyless {
            "undefined".to_owned()
        } else {
            format!("encode{}(request)", plan.operation.request)
        };
        let key = if plan.operation.idempotency {
            "idempotencyKey"
        } else {
            "undefined"
        };
        let _ = writeln!(
            out,
            "    {}: async ({}) =>\n      decode{}(await execute(\"{}\", {}, {body}, {key}), \"{} result\"),",
            plan.method_name,
            arguments.join(", "),
            plan.operation.response,
            plan.operation.method,
            plan.path_expression,
            plan.operation.name
        );
    }
    out.push_str("  };\n  return client;\n}\n");
}

fn emit_index(header: &Header, model: &Model, planned: &[PlannedOperation]) -> String {
    let mut out = String::new();
    out.push_str(BANNER);
    let _ = writeln!(
        out,
        "\nexport const schemaVersion = {{ major: {}, minor: {} }} as const;",
        header.major, header.minor
    );
    out.push_str(RUNTIME);
    emit_scalars(&mut out, model);
    emit_enums(&mut out, model);
    for (name, record) in &model.records {
        emit_record_interface(&mut out, name, record);
        emit_record_decode(&mut out, model, name, record);
        emit_record_encode(&mut out, model, name, record);
    }
    out.push_str(ERROR_CLASS);
    emit_manifest(&mut out, planned);
    emit_client(&mut out, planned);
    out
}

const CONFORMANCE_RUNTIME: &str = r#"
export interface ConformanceRun {
  readonly client: HumanApiClient;
  readonly params: { readonly [name: string]: string };
  readonly body?: JsonValue;
  readonly idempotencyKey?: string;
}

export function runParam(run: ConformanceRun, name: string): string {
  const value = run.params[name];
  if (value === undefined) {
    throw new Error("the golden request path is missing the " + name + " parameter");
  }
  return value;
}

export function runBody(run: ConformanceRun): JsonValue {
  if (run.body === undefined) {
    throw new Error("the golden request carries no body");
  }
  return run.body;
}

export function runKey(run: ConformanceRun): string {
  if (run.idempotencyKey === undefined) {
    throw new Error("the golden request carries no idempotency key");
  }
  return run.idempotencyKey;
}
"#;

fn emit_conformance(planned: &[PlannedOperation]) -> String {
    let mut imports = BTreeSet::new();
    for plan in planned {
        imports.insert(format!("encode{}", plan.operation.response));
        if !plan.bodyless {
            imports.insert(format!("decode{}", plan.operation.request));
        }
    }
    let mut out = String::new();
    out.push_str(BANNER);
    out.push_str("\nimport {\n");
    for import in &imports {
        let _ = writeln!(out, "  {import},");
    }
    out.push_str("  type HumanApiClient,\n  type JsonValue,\n  type OperationName,\n} from \"./index.ts\";\n");
    out.push_str(CONFORMANCE_RUNTIME);
    out.push_str("\nexport const conformance: { readonly [name in OperationName]: (run: ConformanceRun) => Promise<JsonValue> } = {\n");
    for plan in planned {
        let mut arguments = plan
            .path_params
            .iter()
            .map(|name| format!("runParam(run, \"{name}\")"))
            .collect::<Vec<_>>();
        if !plan.bodyless {
            arguments.push(format!(
                "decode{}(runBody(run), \"golden request body\")",
                plan.operation.request
            ));
        }
        if plan.operation.idempotency {
            arguments.push("runKey(run)".to_owned());
        }
        let _ = writeln!(
            out,
            "  \"{}\": async (run) =>\n    encode{}(await run.client.{}({})),",
            plan.operation.name,
            plan.operation.response,
            plan.method_name,
            arguments.join(", ")
        );
    }
    out.push_str("};\n");
    out
}

fn load_all(root: &Path, violations: &mut Vec<Violation>) -> Option<(Header, Vec<SchemaFile>)> {
    let v1 = load_kvx(root, "v1.kvx", violations)?;
    let header = read_header(&v1, violations);
    let mut files = vec![v1];
    for include in &header.includes {
        let named_kvx = Path::new(include)
            .extension()
            .is_some_and(|extension| extension == "kvx");
        if named_kvx && root.join(include).is_file() {
            if let Some(file) = load_kvx(root, include, violations) {
                files.push(file);
            }
        } else {
            violations.push(violation(
                &root.join(include),
                "missing-include",
                format!("include {include} does not exist beside v1.kvx"),
            ));
        }
    }
    Some((header, files))
}

/// Generates the deterministic TypeScript client and its conformance surface
/// from the human-api schema.
///
/// # Errors
///
/// Returns every violation preventing a sound generation.
pub fn generate_client(root: &Path) -> Result<GeneratedClient, Vec<Violation>> {
    let mut violations = Vec::new();
    let Some((header, files)) = load_all(root, &mut violations) else {
        return Err(violations);
    };
    let model = collect_declarations(&files, &mut violations);
    check_type_references(&model, &mut violations);
    let planned = plan_operations(&model, &mut violations);
    if !violations.is_empty() {
        return Err(violations);
    }
    let mut generated = BTreeMap::new();
    generated.insert("index.ts".to_owned(), emit_index(&header, &model, &planned));
    generated.insert("conformance.ts".to_owned(), emit_conformance(&planned));
    Ok(GeneratedClient {
        files: generated,
        operations: planned.len(),
        types: model.scalars.len() + model.enums.len() + model.records.len(),
    })
}

pub fn human_api_ts_client(root: &Path) -> Result<GeneratedClient, Vec<Violation>> {
    generate_client(root)
}

/// Writes the generated client into `out_dir`, replacing what is there.
///
/// # Errors
///
/// Returns generation violations, or the write failure as a violation.
pub fn write_client(root: &Path, out_dir: &Path) -> Result<GeneratedClient, Vec<Violation>> {
    let generated = human_api_ts_client(root)?;
    if let Err(error) = fs::create_dir_all(out_dir) {
        return Err(vec![violation(
            out_dir,
            "unwritable-output",
            format!("cannot create the output directory: {error}"),
        )]);
    }
    for (name, body) in &generated.files {
        let path = out_dir.join(name);
        if let Err(error) = fs::write(&path, body) {
            return Err(vec![violation(
                &path,
                "unwritable-output",
                format!("cannot write the generated file: {error}"),
            )]);
        }
    }
    Ok(generated)
}

/// The drift gate: regenerates from the schema and fails when the committed
/// output is stale, hand-edited, missing or accompanied by unexpected files.
///
/// # Errors
///
/// Returns generation violations or every drift found under `out_dir`.
pub fn check_client(root: &Path, out_dir: &Path) -> Result<GeneratedClient, Vec<Violation>> {
    let generated = generate_client(root)?;
    let mut violations = Vec::new();
    for (name, body) in &generated.files {
        let path = out_dir.join(name);
        match fs::read_to_string(&path) {
            Ok(committed) if &committed == body => {}
            Ok(_) => violations.push(violation(
                &path,
                "stale-or-hand-edited-output",
                format!("{name} does not match regeneration from the schema; refresh it with: {REFRESH_COMMAND}"),
            )),
            Err(_) => violations.push(violation(
                &path,
                "missing-generated-output",
                format!("{name} is not present; generate it with: {REFRESH_COMMAND}"),
            )),
        }
    }
    match fs::read_dir(out_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !generated.files.contains_key(&name) {
                    violations.push(violation(
                        &entry.path(),
                        "unexpected-generated-output",
                        format!("{name} is not produced by the generator"),
                    ));
                }
            }
        }
        Err(error) => violations.push(violation(
            out_dir,
            "missing-generated-output",
            format!("cannot read the output directory: {error}"),
        )),
    }
    if violations.is_empty() {
        Ok(generated)
    } else {
        Err(violations)
    }
}

pub fn human_api_drift_gate(
    root: &Path,
    out_dir: &Path,
) -> Result<GeneratedClient, Vec<Violation>> {
    check_client(root, out_dir)
}
