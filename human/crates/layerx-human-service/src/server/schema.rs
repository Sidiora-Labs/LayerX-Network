use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

use serde_json::{Map, Value};

const SCHEMA_FILES: &[(&str, &str)] = &[
    (
        "v1.kvx",
        include_str!("../../../../schema/human-api/v1.kvx"),
    ),
    (
        "journeys.kvx",
        include_str!("../../../../schema/human-api/journeys.kvx"),
    ),
    (
        "errors.kvx",
        include_str!("../../../../schema/human-api/errors.kvx"),
    ),
    (
        "stream.kvx",
        include_str!("../../../../schema/human-api/stream.kvx"),
    ),
    (
        "identity.kvx",
        include_str!("../../../../schema/human-api/identity.kvx"),
    ),
    (
        "movement.kvx",
        include_str!("../../../../schema/human-api/movement.kvx"),
    ),
    (
        "agents.kvx",
        include_str!("../../../../schema/human-api/agents.kvx"),
    ),
    (
        "activity.kvx",
        include_str!("../../../../schema/human-api/activity.kvx"),
    ),
    (
        "support.kvx",
        include_str!("../../../../schema/human-api/support.kvx"),
    ),
    (
        "home.kvx",
        include_str!("../../../../schema/human-api/home.kvx"),
    ),
];

const MAX_SCHEMA_DEPTH: usize = 32;
const MAX_PATH_PARAMETER_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Field {
    name: String,
    type_name: String,
    array: bool,
    optional: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TypeDeclaration {
    Scalar {
        prefix: Option<String>,
        format: Option<String>,
    },
    Variants(BTreeSet<String>),
    Record(Vec<Field>),
}

/// One operation compiled directly from the owner-supplied human-api schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorizationClass {
    Read,
    MoneyMovement,
    Approval,
    Withdrawal,
    Exit,
    SecuritySettings,
    SecretReveal,
    WalletRebind,
    AgentArchive,
}

impl AuthorizationClass {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "money-movement" => Some(Self::MoneyMovement),
            "approval" => Some(Self::Approval),
            "withdrawal" => Some(Self::Withdrawal),
            "exit" => Some(Self::Exit),
            "security-settings" => Some(Self::SecuritySettings),
            "secret-reveal" => Some(Self::SecretReveal),
            "wallet-rebind" => Some(Self::WalletRebind),
            "agent-archive" => Some(Self::AgentArchive),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::MoneyMovement => "money-movement",
            Self::Approval => "approval",
            Self::Withdrawal => "withdrawal",
            Self::Exit => "exit",
            Self::SecuritySettings => "security-settings",
            Self::SecretReveal => "secret-reveal",
            Self::WalletRebind => "wallet-rebind",
            Self::AgentArchive => "agent-archive",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Operation {
    pub name: String,
    pub method: String,
    pub path: String,
    pub request: String,
    pub response: String,
    pub idempotency: bool,
    pub authorization_class: AuthorizationClass,
}

impl Operation {
    #[must_use]
    pub fn is_public_bootstrap(&self) -> bool {
        matches!(
            self.name.as_str(),
            "account.create"
                | "passkey.register.begin"
                | "passkey.register.finish"
                | "passkey.assert.begin"
                | "passkey.assert.finish"
                | "session.open"
                | "version"
        )
    }

    #[must_use]
    pub fn uses_refresh_cookie(&self) -> bool {
        self.name == "session.refresh"
    }

    #[must_use]
    pub fn mutates(&self) -> bool {
        self.method != "GET"
    }
}

/// A route match carrying only bounded, decoded path parameters.
#[derive(Debug)]
pub struct RouteMatch<'schema> {
    pub operation: &'schema Operation,
    pub path_parameters: BTreeMap<String, String>,
}

/// The executable schema model used by both request decoding and response encoding.
#[derive(Clone, Debug)]
pub struct ApiSchema {
    major: u32,
    minor: u32,
    types: BTreeMap<String, TypeDeclaration>,
    operations: Vec<Operation>,
}

impl ApiSchema {
    /// Parses the schema embedded in this binary and refuses an incomplete model.
    ///
    /// # Errors
    ///
    /// Returns a structural schema error before the service binds a listener.
    pub fn v1() -> Result<Self, SchemaError> {
        let mut all_sections = Vec::new();
        for (file, source) in SCHEMA_FILES {
            all_sections.push((file.to_string(), parse_sections(file, source)?));
        }
        let v1 = all_sections
            .first()
            .and_then(|(_, sections)| sections.get("schema"))
            .ok_or_else(|| SchemaError::new("v1.kvx has no schema section"))?;
        let major = required_number(v1, "major")?;
        let minor = required_number(v1, "minor")?;
        let includes = v1
            .get("includes")
            .ok_or_else(|| SchemaError::new("v1.kvx has no includes declaration"))
            .and_then(|value| parse_list(value))?;
        let embedded = SCHEMA_FILES
            .iter()
            .skip(1)
            .map(|(name, _)| (*name).to_owned())
            .collect::<Vec<_>>();
        if includes != embedded {
            return Err(SchemaError::new(
                "the executable schema files do not match v1.kvx includes",
            ));
        }
        let mut types = BTreeMap::new();
        types.insert(
            "string".to_owned(),
            TypeDeclaration::Scalar {
                prefix: None,
                format: None,
            },
        );
        for (file, sections) in &all_sections {
            for (section, values) in sections {
                if let Some(name) = section.strip_prefix("scalar.") {
                    insert_type(
                        &mut types,
                        name,
                        TypeDeclaration::Scalar {
                            prefix: quoted(values.get("prefix")),
                            format: quoted(values.get("format")),
                        },
                    )?;
                } else if let Some(name) = section.strip_prefix("type.") {
                    if let Some(variants) = values.get("variants") {
                        insert_type(
                            &mut types,
                            name,
                            TypeDeclaration::Variants(parse_list(variants)?.into_iter().collect()),
                        )?;
                    } else {
                        insert_type(
                            &mut types,
                            name,
                            TypeDeclaration::Record(record_fields(
                                file, section, values, "required",
                            )?),
                        )?;
                    }
                } else if let Some(name) = section.strip_prefix("record.") {
                    insert_type(
                        &mut types,
                        name,
                        TypeDeclaration::Record(record_fields(file, section, values, "fields")?),
                    )?;
                }
            }
        }
        for (type_name, declaration) in &types {
            let TypeDeclaration::Record(fields) = declaration else {
                continue;
            };
            for field in fields {
                if !matches!(field.type_name.as_str(), "boolean" | "integer" | "object")
                    && !types.contains_key(&field.type_name)
                {
                    return Err(SchemaError::new(format!(
                        "{type_name}.{} references undeclared type {}",
                        field.name, field.type_name
                    )));
                }
            }
        }
        let mut operations = Vec::new();
        for (file, sections) in &all_sections {
            for (section, values) in sections {
                let Some(name) = section.strip_prefix("operation.") else {
                    continue;
                };
                let authorization_class_value =
                    required_quoted(file, section, values, "authorization_class")?;
                let authorization_class = AuthorizationClass::parse(&authorization_class_value)
                    .ok_or_else(|| {
                        SchemaError::new(format!(
                            "{file}.{section}.authorization_class is not a recognized closed class"
                        ))
                    })?;
                let operation = Operation {
                    name: name.to_owned(),
                    method: required_quoted(file, section, values, "method")?,
                    path: required_quoted(file, section, values, "path")?,
                    request: required_quoted(file, section, values, "request")?,
                    response: required_quoted(file, section, values, "response")?,
                    idempotency: values
                        .get("idempotency")
                        .is_some_and(|value| value == "true"),
                    authorization_class,
                };
                let base_path = format!("/v{major}/");
                if !matches!(
                    operation.method.as_str(),
                    "DELETE" | "GET" | "PATCH" | "POST" | "PUT"
                ) || !operation.path.starts_with(&base_path)
                    || operation.path.contains(['?', '#'])
                    || operation.path.ends_with('/')
                {
                    return Err(SchemaError::new(format!(
                        "{file}.{section} does not declare a canonical v{major} route"
                    )));
                }
                if !types.contains_key(&operation.request)
                    || !types.contains_key(&operation.response)
                {
                    return Err(SchemaError::new(format!(
                        "{}.{} references an undeclared request or response type",
                        file, section
                    )));
                }
                if operations.iter().any(|existing: &Operation| {
                    existing.name == operation.name
                        || (existing.method == operation.method && existing.path == operation.path)
                }) {
                    return Err(SchemaError::new(format!(
                        "{}.{} duplicates an operation name or route",
                        file, section
                    )));
                }
                operations.push(operation);
            }
        }
        operations.sort_by(|left, right| left.name.cmp(&right.name));
        if operations.is_empty() {
            return Err(SchemaError::new("human-api declares no operations"));
        }
        Ok(Self {
            major,
            minor,
            types,
            operations,
        })
    }

    #[must_use]
    pub const fn version(&self) -> (u32, u32) {
        (self.major, self.minor)
    }

    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    /// Resolves an operation by its frozen schema name.
    #[must_use]
    pub fn operation(&self, name: &str) -> Option<&Operation> {
        self.operations
            .binary_search_by(|operation| operation.name.as_str().cmp(name))
            .ok()
            .map(|index| &self.operations[index])
    }

    /// Matches one method/path pair without permitting encoded separators or traversal.
    ///
    /// # Errors
    ///
    /// Returns a typed path failure when decoding is unsafe.
    pub fn route(&self, method: &str, path: &str) -> Result<Option<RouteMatch<'_>>, SchemaError> {
        if path.contains('?') || path.contains('#') || !path.starts_with('/') {
            return Err(SchemaError::new(
                "the request target is not a canonical path",
            ));
        }
        for operation in &self.operations {
            if operation.method != method {
                continue;
            }
            if let Some(path_parameters) = match_path(&operation.path, path)? {
                return Ok(Some(RouteMatch {
                    operation,
                    path_parameters,
                }));
            }
        }
        Ok(None)
    }

    /// Admits a request body only after recursive validation against its declared schema type.
    ///
    /// # Errors
    ///
    /// Returns the first exact schema path that does not match.
    pub fn decode_request(
        &self,
        operation: &Operation,
        body: Option<Value>,
    ) -> Result<Value, SchemaError> {
        let empty = operation.request == "Empty";
        let value = match (empty, body) {
            (true, None) => Value::Object(Map::new()),
            (true, Some(Value::Object(object))) if object.is_empty() => Value::Object(object),
            (true, Some(_)) => return Err(SchemaError::at("request", "must have no body")),
            (false, Some(value)) => value,
            (false, None) => return Err(SchemaError::at("request", "body is required")),
        };
        self.validate(&operation.request, &value, "request", 0)?;
        Ok(value)
    }

    /// Admits a component result only after recursive validation against the response contract.
    ///
    /// # Errors
    ///
    /// Refuses a component response that could not have been produced by the declared schema.
    pub fn encode_response(
        &self,
        operation: &Operation,
        result: &Value,
    ) -> Result<(), SchemaError> {
        self.validate(&operation.response, result, "response.result", 0)?;
        let balance = match operation.name.as_str() {
            "account.balance" => Some(result),
            "home.summary" => result.get("balance"),
            _ => None,
        };
        if let Some(balance) = balance {
            enforce_verified_balance(balance)?;
        }
        Ok(())
    }

    fn validate(
        &self,
        type_name: &str,
        value: &Value,
        at: &str,
        depth: usize,
    ) -> Result<(), SchemaError> {
        if depth > MAX_SCHEMA_DEPTH {
            return Err(SchemaError::at(at, "exceeds the schema nesting bound"));
        }
        match type_name {
            "boolean" if value.is_boolean() => return Ok(()),
            "integer" if value.as_i64().is_some() || value.as_u64().is_some() => return Ok(()),
            "object" if value.is_object() => return Ok(()),
            "boolean" | "integer" | "object" => {
                return Err(SchemaError::at(at, format!("must be {type_name}")));
            }
            _ => {}
        }
        let declaration = self
            .types
            .get(type_name)
            .ok_or_else(|| SchemaError::at(at, format!("references unknown type {type_name}")))?;
        match declaration {
            TypeDeclaration::Scalar { prefix, format } => {
                let text = value
                    .as_str()
                    .ok_or_else(|| SchemaError::at(at, format!("must be a {type_name} string")))?;
                if prefix.as_ref().is_some_and(|required| {
                    !text.starts_with(required) || text.len() == required.len()
                }) {
                    return Err(SchemaError::at(
                        at,
                        format!("must carry the {type_name} prefix"),
                    ));
                }
                match format.as_deref() {
                    Some("decimal")
                        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) =>
                    {
                        return Err(SchemaError::at(at, "must be a decimal base-unit string"));
                    }
                    Some("currency")
                        if text.is_empty()
                            || !text
                                .bytes()
                                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()) =>
                    {
                        return Err(SchemaError::at(at, "must be an uppercase currency code"));
                    }
                    Some("rfc3339-utc")
                        if text.len() < 20
                            || !text.ends_with('Z')
                            || text.as_bytes().get(4) != Some(&b'-')
                            || text.as_bytes().get(7) != Some(&b'-')
                            || text.as_bytes().get(10) != Some(&b'T') =>
                    {
                        return Err(SchemaError::at(at, "must be an RFC 3339 UTC timestamp"));
                    }
                    Some("copy-key")
                        if text.is_empty()
                            || !text.bytes().all(|byte| {
                                byte.is_ascii_lowercase()
                                    || byte.is_ascii_digit()
                                    || matches!(byte, b'.' | b'_' | b'-')
                            }) =>
                    {
                        return Err(SchemaError::at(at, "must be a copy-catalog key"));
                    }
                    _ => {}
                }
                Ok(())
            }
            TypeDeclaration::Variants(variants) => {
                let variant = value
                    .as_str()
                    .ok_or_else(|| SchemaError::at(at, format!("must be a {type_name} string")))?;
                if variants.contains(variant) {
                    Ok(())
                } else {
                    Err(SchemaError::at(
                        at,
                        format!("is not a declared {type_name} variant"),
                    ))
                }
            }
            TypeDeclaration::Record(fields) => {
                let object = value
                    .as_object()
                    .ok_or_else(|| SchemaError::at(at, format!("must be a {type_name} object")))?;
                for field in fields {
                    let child_at = format!("{at}.{}", field.name);
                    let Some(child) = object.get(&field.name) else {
                        if field.optional {
                            continue;
                        }
                        return Err(SchemaError::at(child_at, "is required"));
                    };
                    if field.array {
                        let entries = child
                            .as_array()
                            .ok_or_else(|| SchemaError::at(&child_at, "must be an array"))?;
                        for (index, entry) in entries.iter().enumerate() {
                            self.validate(
                                &field.type_name,
                                entry,
                                &format!("{child_at}[{index}]"),
                                depth + 1,
                            )?;
                        }
                    } else {
                        self.validate(&field.type_name, child, &child_at, depth + 1)?;
                    }
                }
                let declared = fields
                    .iter()
                    .map(|field| field.name.as_str())
                    .collect::<BTreeSet<_>>();
                if let Some(unknown) = object.keys().find(|key| !declared.contains(key.as_str())) {
                    return Err(SchemaError::at(
                        format!("{at}.{unknown}"),
                        "is not declared in this schema version",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Returns the schema-owned receipt-verified balance operation exported by v1.
#[must_use]
pub fn human_api_balance_operation(schema: &ApiSchema) -> Option<&Operation> {
    schema
        .operations()
        .iter()
        .find(|operation| operation.name == "account.balance")
}

fn enforce_verified_balance(balance: &Value) -> Result<(), SchemaError> {
    let object = balance
        .as_object()
        .ok_or_else(|| SchemaError::at("response.result.balance", "must be an object"))?;
    let verification = object
        .get("verification")
        .and_then(Value::as_str)
        .ok_or_else(|| SchemaError::at("response.result.balance.verification", "is required"))?;
    if !matches!(verification, "receipt-verified" | "checkpoint-finalised") {
        return Err(SchemaError::at(
            "response.result.balance.verification",
            "must be backed by a LayerX receipt or checkpoint",
        ));
    }
    let evidence = object
        .get("evidence")
        .and_then(Value::as_array)
        .ok_or_else(|| SchemaError::at("response.result.balance.evidence", "is required"))?;
    let backed = evidence.iter().any(|entry| {
        entry.as_object().is_some_and(|entry| {
            let class = entry.get("class").and_then(Value::as_str);
            let level = entry.get("verification").and_then(Value::as_str);
            match verification {
                "receipt-verified" => {
                    matches!(class, Some("layerx-receipt") | Some("checkpoint-proof"))
                        && matches!(
                            level,
                            Some("receipt-verified") | Some("checkpoint-finalised")
                        )
                }
                "checkpoint-finalised" => {
                    class == Some("checkpoint-proof") && level == Some("checkpoint-finalised")
                }
                _ => false,
            }
        })
    });
    if !backed {
        return Err(SchemaError::at(
            "response.result.balance.evidence",
            "does not back the reported verification level",
        ));
    }
    Ok(())
}

fn parse_sections(
    file: &str,
    source: &str,
) -> Result<BTreeMap<String, BTreeMap<String, String>>, SchemaError> {
    let mut sections = BTreeMap::<String, BTreeMap<String, String>>::new();
    let mut current = String::new();
    for (index, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            current = section.to_owned();
            sections.entry(current.clone()).or_default();
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            SchemaError::new(format!(
                "{file}:{} is not a key/value declaration",
                index + 1
            ))
        })?;
        if current.is_empty() {
            return Err(SchemaError::new(format!(
                "{file}:{} declares a value outside a section",
                index + 1
            )));
        }
        if sections
            .entry(current.clone())
            .or_default()
            .insert(key.trim().to_owned(), value.trim().to_owned())
            .is_some()
        {
            return Err(SchemaError::new(format!(
                "{file}:{} repeats {}.{}",
                index + 1,
                current,
                key.trim()
            )));
        }
    }
    Ok(sections)
}

fn record_fields(
    file: &str,
    section: &str,
    values: &BTreeMap<String, String>,
    required_key: &str,
) -> Result<Vec<Field>, SchemaError> {
    let required = values.get(required_key).ok_or_else(|| {
        SchemaError::new(format!("{file}.{section} has no {required_key} fields"))
    })?;
    let mut fields = parse_fields(required, false)?;
    if let Some(optional) = values.get("optional") {
        fields.extend(parse_fields(optional, true)?);
    }
    let mut names = BTreeSet::new();
    if fields.iter().any(|field| !names.insert(field.name.clone())) {
        return Err(SchemaError::new(format!(
            "{file}.{section} repeats a field"
        )));
    }
    Ok(fields)
}

fn parse_fields(value: &str, optional: bool) -> Result<Vec<Field>, SchemaError> {
    parse_list(value)?
        .into_iter()
        .map(|declaration| {
            let (name, declared_type) = declaration
                .split_once(':')
                .ok_or_else(|| SchemaError::new(format!("invalid field {declaration}")))?;
            let (type_name, array) = declared_type
                .strip_suffix("[]")
                .map_or((declared_type, false), |inner| (inner, true));
            if name.is_empty() || type_name.is_empty() {
                return Err(SchemaError::new(format!("invalid field {declaration}")));
            }
            Ok(Field {
                name: name.to_owned(),
                type_name: type_name.to_owned(),
                array,
                optional,
            })
        })
        .collect()
}

fn parse_list(value: &str) -> Result<Vec<String>, SchemaError> {
    let inner = value
        .strip_prefix('[')
        .and_then(|text| text.strip_suffix(']'))
        .ok_or_else(|| SchemaError::new("invalid kvx list"))?
        .trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let mut rest = inner;
    loop {
        let quoted = rest
            .trim_start()
            .strip_prefix('"')
            .ok_or_else(|| SchemaError::new("kvx list item is not quoted"))?;
        let end = quoted
            .find('"')
            .ok_or_else(|| SchemaError::new("kvx list item is unterminated"))?;
        items.push(quoted[..end].to_owned());
        let tail = quoted[end + 1..].trim_start();
        if tail.is_empty() {
            return Ok(items);
        }
        rest = tail
            .strip_prefix(',')
            .ok_or_else(|| SchemaError::new("kvx list items are not comma separated"))?;
    }
}

fn insert_type(
    types: &mut BTreeMap<String, TypeDeclaration>,
    name: &str,
    declaration: TypeDeclaration,
) -> Result<(), SchemaError> {
    if types.insert(name.to_owned(), declaration).is_some() {
        Err(SchemaError::new(format!("duplicate schema type {name}")))
    } else {
        Ok(())
    }
}

fn quoted(value: Option<&String>) -> Option<String> {
    value
        .and_then(|text| text.strip_prefix('"'))
        .and_then(|text| text.strip_suffix('"'))
        .map(str::to_owned)
}

fn required_quoted(
    file: &str,
    section: &str,
    values: &BTreeMap<String, String>,
    key: &str,
) -> Result<String, SchemaError> {
    quoted(values.get(key))
        .ok_or_else(|| SchemaError::new(format!("{file}.{section}.{key} is not a quoted value")))
}

fn required_number(values: &BTreeMap<String, String>, key: &str) -> Result<u32, SchemaError> {
    values
        .get(key)
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| SchemaError::new(format!("schema.{key} is not an integer")))
}

fn match_path(template: &str, path: &str) -> Result<Option<BTreeMap<String, String>>, SchemaError> {
    let expected = template.split('/').skip(1).collect::<Vec<_>>();
    let actual = path.split('/').skip(1).collect::<Vec<_>>();
    if expected.len() != actual.len() {
        return Ok(None);
    }
    let mut parameters = BTreeMap::new();
    for (declared, supplied) in expected.into_iter().zip(actual) {
        if let Some(name) = declared
            .strip_prefix('{')
            .and_then(|value| value.strip_suffix('}'))
        {
            let decoded = percent_decode(supplied)?;
            if decoded.is_empty()
                || decoded.len() > MAX_PATH_PARAMETER_BYTES
                || decoded.contains(['/', '\\', '\0'])
                || matches!(decoded.as_str(), "." | "..")
            {
                return Err(SchemaError::new("unsafe path parameter"));
            }
            parameters.insert(name.to_owned(), decoded);
        } else if declared != supplied {
            return Ok(None);
        }
    }
    Ok(Some(parameters))
}

fn percent_decode(value: &str) -> Result<String, SchemaError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes
                .get(index + 1)
                .and_then(|byte| hex(*byte))
                .ok_or_else(|| SchemaError::new("malformed path escape"))?;
            let low = bytes
                .get(index + 2)
                .and_then(|byte| hex(*byte))
                .ok_or_else(|| SchemaError::new("malformed path escape"))?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| SchemaError::new("path parameter is not UTF-8"))
}

const fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// A startup or message-shape failure tied to the schema, never emitted unstructured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaError {
    detail: String,
}

impl SchemaError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    fn at(at: impl AsRef<str>, detail: impl AsRef<str>) -> Self {
        Self::new(format!("{} {}", at.as_ref(), detail.as_ref()))
    }

    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl Display for SchemaError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for SchemaError {}
