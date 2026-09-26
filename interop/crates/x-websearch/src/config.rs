use serde_json::{Map, Value};
use std::io::Read as _;
use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub const MAX_CONFIG_BYTES: usize = 1_048_576;
pub const CONNECT_TIMEOUT_LIMIT_MS: u64 = 3_000;
pub const TOTAL_TIMEOUT_LIMIT_MS: u64 = 10_000;
pub const BODY_LIMIT_BYTES: u64 = 2_097_152;
pub const REDIRECT_LIMIT: u8 = 3;
pub const MAX_SEEDS: usize = 10_000;
pub const MAX_PEERS: usize = 256;
pub const MAX_URL_BYTES: usize = 2_048;
pub const MAX_PAGES_PER_CYCLE: u32 = 1_000_000;
pub const MAX_DEPTH: u32 = 32;
pub const MAX_POLITENESS_DELAY_MS: u64 = 3_600_000;
pub const MAX_CONFIRMATIONS: u32 = 1_024;
/// The time between the starts of two crawl cycles when
/// `crawl_interval_seconds` is absent.
pub const DEFAULT_CRAWL_INTERVAL_SECONDS: u64 = 900;
/// The longest `crawl_interval_seconds` accepted: one day.
pub const MAX_CRAWL_INTERVAL_SECONDS: u64 = 86_400;
const MAX_PATH_BYTES: usize = 4_096;
pub const MAX_DID_BYTES: usize = 255;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    Unreadable,
    Oversized,
    Syntax,
    Missing,
    Invalid,
    Placeholder,
    Unknown,
    KeyMaterial,
}

impl Refusal {
    const fn describe(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::Oversized => "larger than 1 MiB",
            Self::Syntax => "not a JSON object",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
            Self::Placeholder => "a placeholder",
            Self::Unknown => "not a known field",
            Self::KeyMaterial => "key material, which is read only from key files",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError {
    pub field: String,
    pub refusal: Refusal,
}

impl ConfigError {
    fn new(field: impl Into<String>, refusal: Refusal) -> Self {
        Self {
            field: field.into(),
            refusal,
        }
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "configuration refused: {} is {}",
            self.field,
            self.refusal.describe()
        )
    }
}

impl std::error::Error for ConfigError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum AssetSymbol {
    Sid,
    Pax,
    Usdc,
    Usdl,
}

impl AssetSymbol {
    pub const ALL: [Self; 4] = [Self::Sid, Self::Pax, Self::Usdc, Self::Usdl];

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Sid => "SID",
            Self::Pax => "PAX",
            Self::Usdc => "USDC",
            Self::Usdl => "USDL",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssetConfig {
    pub symbol: AssetSymbol,
    pub asset_id: [u8; 32],
    pub price: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrawlConfig {
    pub pages_per_cycle: u32,
    pub pages_per_host: u32,
    pub max_depth: u32,
    pub politeness_delay_ms: u64,
}

impl CrawlConfig {
    #[must_use]
    pub const fn politeness_delay(&self) -> Duration {
        Duration::from_millis(self.politeness_delay_ms)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FetchLimits {
    pub connect_timeout_ms: u64,
    pub total_timeout_ms: u64,
    pub max_body_bytes: u64,
    pub max_redirects: u8,
    pub allow_loopback: bool,
}

impl FetchLimits {
    #[must_use]
    pub const fn connect_timeout(&self) -> Duration {
        Duration::from_millis(self.connect_timeout_ms)
    }

    #[must_use]
    pub const fn total_timeout(&self) -> Duration {
        Duration::from_millis(self.total_timeout_ms)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequencerTrust {
    pub sequencer_id: [u8; 32],
    pub public_key: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayConfig {
    pub endpoint: String,
    pub sequencer: SequencerTrust,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvmConfig {
    pub endpoint: String,
    pub chain_id: u64,
    pub confirmations: u32,
}

// Payment settings: the optional `payment` object. Every field in it may be
// left out, and its default then reproduces the sidecar's behaviour without it.

/// The fee limit the receiver binds into every signed draw when
/// `payment.draw_fee_limit` is absent, in base units of the fee asset.
pub const DEFAULT_DRAW_FEE_LIMIT: u128 = 1_000_000_000_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentConfig {
    /// `payment.payer_did`: the DID whose accounts every metered draw is
    /// taken from. Absent: each request names its payer in the
    /// `LAYERX-PAYER-DID` header.
    pub payer_did: Option<String>,
    /// `payment.draw_fee_limit`: the fee limit, as a canonical decimal
    /// string, the receiver signs into each draw and the most a draw's
    /// receipt may charge. Absent: [`DEFAULT_DRAW_FEE_LIMIT`].
    pub draw_fee_limit: u128,
    /// `payment.conformance_suite`: the absolute path of the directory of
    /// recorded gateway exchanges the x402 adapter's pinned conformance suite
    /// digests. Absent: the pinned suite is registered without reading it.
    pub conformance_suite: Option<PathBuf>,
}

impl Default for PaymentConfig {
    fn default() -> Self {
        Self {
            payer_did: None,
            draw_fee_limit: DEFAULT_DRAW_FEE_LIMIT,
            conformance_suite: None,
        }
    }
}

/// Whether `did` is a DID a payer may be named by: `did:` followed by
/// lowercase ASCII letters, digits, `.`, `_`, `-` and `:`, with no empty
/// segment, no trailing `:`, no `:asset:` segment, and at most
/// [`MAX_DID_BYTES`] bytes.
#[must_use]
pub fn payer_did_valid(did: &str) -> bool {
    did.len() <= MAX_DID_BYTES
        && did.starts_with("did:")
        && !did.ends_with(':')
        && !did.contains("::")
        && !did.contains(":asset:")
        && did
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b':' | b'-'))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub listen: SocketAddr,
    pub data_dir: PathBuf,
    pub seeds: Vec<String>,
    pub crawl: CrawlConfig,
    /// `crawl_interval_seconds`: the time from the start of one crawl cycle
    /// to the start of the next, from 1 to [`MAX_CRAWL_INTERVAL_SECONDS`].
    /// Absent: [`DEFAULT_CRAWL_INTERVAL_SECONDS`].
    pub crawl_interval_seconds: u64,
    pub fetch: FetchLimits,
    pub assets: [AssetConfig; 4],
    pub gateway: GatewayConfig,
    pub payment: PaymentConfig,
    pub evm: EvmConfig,
    pub kernel_network_id: u32,
    pub peers: Vec<String>,
}

impl Config {
    /// # Errors
    /// Names the first field that is missing, unknown, malformed, a placeholder
    /// or key material.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let value: Value =
            serde_json::from_str(text).map_err(|_| ConfigError::new("config", Refusal::Syntax))?;
        let Value::Object(map) = value else {
            return Err(ConfigError::new("config", Refusal::Syntax));
        };
        scan_key_material(&map, "")?;
        let mut root = Object::root(map);
        let listen = listen(&mut root)?;
        let data_dir = data_dir(&mut root)?;
        let fetch = fetch(&mut root)?;
        let seeds = seeds(&mut root, fetch.allow_loopback)?;
        let crawl = crawl(&mut root)?;
        let crawl_interval_seconds = crawl_interval_seconds(&mut root)?;
        let assets = assets(&mut root)?;
        let gateway = gateway(&mut root)?;
        let payment = payment(&mut root)?;
        let evm = evm(&mut root)?;
        let kernel_network_id = positive_u32(&mut root, "kernel_network_id", u32::MAX)?;
        let peers = peers(&mut root)?;
        root.finish()?;
        Ok(Self {
            listen,
            data_dir,
            seeds,
            crawl,
            crawl_interval_seconds,
            fetch,
            assets,
            gateway,
            payment,
            evm,
            kernel_network_id,
            peers,
        })
    }

    /// # Errors
    /// Refuses an unreadable or oversized file and every refusal of `parse`.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        load(path)
    }

    /// The time from the start of one crawl cycle to the start of the next.
    #[must_use]
    pub const fn crawl_interval(&self) -> Duration {
        Duration::from_secs(self.crawl_interval_seconds)
    }

    #[must_use]
    pub fn asset(&self, symbol: AssetSymbol) -> &AssetConfig {
        let index = match symbol {
            AssetSymbol::Sid => 0,
            AssetSymbol::Pax => 1,
            AssetSymbol::Usdc => 2,
            AssetSymbol::Usdl => 3,
        };
        &self.assets[index]
    }
}

/// # Errors
/// Refuses an unreadable or oversized file and every refusal of `Config::parse`.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let file =
        std::fs::File::open(path).map_err(|_| ConfigError::new("config", Refusal::Unreadable))?;
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ConfigError::new("config", Refusal::Unreadable))?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::new("config", Refusal::Oversized));
    }
    let text = String::from_utf8(bytes).map_err(|_| ConfigError::new("config", Refusal::Syntax))?;
    Config::parse(&text)
}

struct Object {
    path: String,
    map: Map<String, Value>,
}

impl Object {
    const fn root(map: Map<String, Value>) -> Self {
        Self {
            path: String::new(),
            map,
        }
    }

    fn field_path(&self, name: &str) -> String {
        if self.path.is_empty() {
            name.to_owned()
        } else {
            format!("{}.{name}", self.path)
        }
    }

    fn take(&mut self, name: &str) -> Result<(Value, String), ConfigError> {
        let path = self.field_path(name);
        match self.map.remove(name) {
            None | Some(Value::Null) => Err(ConfigError::new(path, Refusal::Missing)),
            Some(value) => Ok((value, path)),
        }
    }

    /// A field that may be left out: `None` when absent. A present `null` is
    /// malformed.
    fn optional(&mut self, name: &str) -> Result<Option<(Value, String)>, ConfigError> {
        let path = self.field_path(name);
        match self.map.remove(name) {
            None => Ok(None),
            Some(Value::Null) => Err(ConfigError::new(path, Refusal::Invalid)),
            Some(value) => Ok(Some((value, path))),
        }
    }

    fn object(&mut self, name: &str) -> Result<Self, ConfigError> {
        let (value, path) = self.take(name)?;
        match value {
            Value::Object(map) => Ok(Self { path, map }),
            _ => Err(ConfigError::new(path, Refusal::Invalid)),
        }
    }

    fn string(&mut self, name: &str) -> Result<(String, String), ConfigError> {
        let (value, path) = self.take(name)?;
        match value {
            Value::String(text) => Ok((text, path)),
            _ => Err(ConfigError::new(path, Refusal::Invalid)),
        }
    }

    fn integer(&mut self, name: &str) -> Result<(u64, String), ConfigError> {
        let (value, path) = self.take(name)?;
        let number = value
            .as_u64()
            .ok_or_else(|| ConfigError::new(path.clone(), Refusal::Invalid))?;
        Ok((number, path))
    }

    fn finish(self) -> Result<(), ConfigError> {
        match self.map.keys().next() {
            Some(name) => Err(ConfigError::new(self.field_path(name), Refusal::Unknown)),
            None => Ok(()),
        }
    }
}

const HEX_FIELDS: [&str; 3] = ["asset_id", "sequencer_id", "sequencer_public_key"];
const SECRET_WORDS: [&str; 8] = [
    "private",
    "secret",
    "mnemonic",
    "password",
    "passphrase",
    "seed_phrase",
    "keystore",
    "credential",
];

fn secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SECRET_WORDS.iter().any(|word| lower.contains(word))
        || (lower.contains("key") && !lower.ends_with("public_key"))
}

fn secret_text(text: &str) -> bool {
    text.contains("-----BEGIN") || text.to_ascii_uppercase().contains("PRIVATE KEY")
}

fn scan_key_material(map: &Map<String, Value>, prefix: &str) -> Result<(), ConfigError> {
    for (name, value) in map {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        if secret_name(name) {
            return Err(ConfigError::new(path, Refusal::KeyMaterial));
        }
        scan_value(value, &path, HEX_FIELDS.contains(&name.as_str()))?;
    }
    Ok(())
}

fn scan_value(value: &Value, path: &str, hex_field: bool) -> Result<(), ConfigError> {
    match value {
        Value::Object(map) => scan_key_material(map, path),
        Value::Array(items) => items
            .iter()
            .try_for_each(|item| scan_value(item, path, false)),
        Value::String(text) => {
            if secret_text(text) || (!hex_field && parse_hex32(text).is_some()) {
                Err(ConfigError::new(path, Refusal::KeyMaterial))
            } else {
                Ok(())
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
    }
}

const PLACEHOLDER_WORDS: [&str; 9] = [
    "replace_me",
    "replace-me",
    "replace_with",
    "replace-with",
    "changeme",
    "change_me",
    "change-me",
    "placeholder",
    "your_",
];

fn placeholder_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.trim().is_empty()
        || lower.contains('<')
        || lower.contains('>')
        || lower.contains("${")
        || PLACEHOLDER_WORDS.iter().any(|word| lower.contains(word))
}

fn placeholder_host(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    ["example.com", "example.net", "example.org"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
        || [".example", ".invalid", ".test", ".localhost", ".local"]
            .iter()
            .any(|suffix| host.ends_with(suffix))
        || ["example", "invalid", "test"].contains(&host)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[must_use]
pub fn parse_hex32(text: &str) -> Option<[u8; 32]> {
    let digits = text.strip_prefix("0x").unwrap_or(text).as_bytes();
    if digits.len() != 64 {
        return None;
    }
    let mut result = [0; 32];
    for (byte, pair) in result.iter_mut().zip(digits.chunks_exact(2)) {
        *byte = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(result)
}

fn hex32(object: &mut Object, name: &str) -> Result<[u8; 32], ConfigError> {
    let (text, path) = object.string(name)?;
    if placeholder_text(&text) {
        return Err(ConfigError::new(path, Refusal::Placeholder));
    }
    let bytes =
        parse_hex32(&text).ok_or_else(|| ConfigError::new(path.clone(), Refusal::Invalid))?;
    if bytes.iter().all(|byte| *byte == bytes[0]) {
        return Err(ConfigError::new(path, Refusal::Placeholder));
    }
    Ok(bytes)
}

fn bounded(object: &mut Object, name: &str, maximum: u64) -> Result<u64, ConfigError> {
    let (number, path) = object.integer(name)?;
    if number == 0 {
        return Err(ConfigError::new(path, Refusal::Placeholder));
    }
    if number > maximum {
        return Err(ConfigError::new(path, Refusal::Invalid));
    }
    Ok(number)
}

fn positive_u32(object: &mut Object, name: &str, maximum: u32) -> Result<u32, ConfigError> {
    let path = object.field_path(name);
    let number = bounded(object, name, u64::from(maximum))?;
    u32::try_from(number).map_err(|_| ConfigError::new(path, Refusal::Invalid))
}

fn listen(root: &mut Object) -> Result<SocketAddr, ConfigError> {
    let (text, path) = root.string("listen")?;
    if placeholder_text(&text) {
        return Err(ConfigError::new(path, Refusal::Placeholder));
    }
    let address: SocketAddr = text
        .parse()
        .map_err(|_| ConfigError::new(path.clone(), Refusal::Invalid))?;
    if address.port() == 0 {
        return Err(ConfigError::new(path, Refusal::Placeholder));
    }
    Ok(address)
}

fn data_dir(root: &mut Object) -> Result<PathBuf, ConfigError> {
    let (text, path) = root.string("data_dir")?;
    if placeholder_text(&text) || text.starts_with("/path/to") {
        return Err(ConfigError::new(path, Refusal::Placeholder));
    }
    absolute_directory(&text).ok_or_else(|| ConfigError::new(path, Refusal::Invalid))
}

/// An absolute path below the root with only normal components.
fn absolute_directory(text: &str) -> Option<PathBuf> {
    let directory = PathBuf::from(text);
    let valid = text.len() <= MAX_PATH_BYTES
        && !text.chars().any(char::is_control)
        && directory.is_absolute()
        && directory.parent().is_some()
        && directory
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)));
    valid.then_some(directory)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum UrlKind {
    Seed { allow_loopback: bool },
    Endpoint,
}

fn loopback_host(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn forbidden_literal(host: &str) -> bool {
    match host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        Ok(IpAddr::V4(address)) => {
            address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || address.is_multicast()
                || address.is_unspecified()
                || address.is_broadcast()
        }
        Ok(IpAddr::V6(address)) => {
            address.is_loopback()
                || address.is_multicast()
                || address.is_unspecified()
                || (address.segments()[0] & 0xfe00) == 0xfc00
                || (address.segments()[0] & 0xffc0) == 0xfe80
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| forbidden_literal(&mapped.to_string()))
        }
        Err(_) => host == "localhost",
    }
}

fn split_authority(authority: &str) -> Option<(&str, Option<&str>)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &authority[..end + 2];
        let tail = &rest[end + 1..];
        return match tail.strip_prefix(':') {
            Some(port) => Some((host, Some(port))),
            None if tail.is_empty() => Some((host, None)),
            None => None,
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => Some((host, Some(port))),
        Some(_) => None,
        None => Some((authority, None)),
    }
}

fn check_url(text: &str, path: &str, kind: UrlKind) -> Result<(), ConfigError> {
    let invalid = || ConfigError::new(path, Refusal::Invalid);
    if placeholder_text(text) {
        return Err(ConfigError::new(path, Refusal::Placeholder));
    }
    if text.len() > MAX_URL_BYTES
        || !text.is_ascii()
        || text
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
        || text.contains('@')
        || text.contains('#')
    {
        return Err(invalid());
    }
    let (secure, rest) = if let Some(rest) = text.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err(invalid());
    };
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if kind == UrlKind::Endpoint && tail.contains('?') {
        return Err(invalid());
    }
    let (host, port) = split_authority(authority).ok_or_else(invalid)?;
    if host.is_empty() || host.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Err(invalid());
    }
    if let Some(port) = port {
        if port.parse::<u16>().map_or(true, |number| number == 0) {
            return Err(invalid());
        }
    }
    if placeholder_host(host) {
        return Err(ConfigError::new(path, Refusal::Placeholder));
    }
    match kind {
        UrlKind::Endpoint if !secure && !loopback_host(host) => Err(invalid()),
        UrlKind::Seed {
            allow_loopback: false,
        } if forbidden_literal(host) => Err(invalid()),
        _ => Ok(()),
    }
}

fn url_list(
    root: &mut Object,
    name: &str,
    maximum: usize,
    kind: UrlKind,
) -> Result<Vec<String>, ConfigError> {
    let (value, path) = root.take(name)?;
    let Value::Array(items) = value else {
        return Err(ConfigError::new(path, Refusal::Invalid));
    };
    if items.len() > maximum {
        return Err(ConfigError::new(path, Refusal::Invalid));
    }
    let mut urls: Vec<String> = Vec::with_capacity(items.len());
    for item in items {
        let Value::String(text) = item else {
            return Err(ConfigError::new(path, Refusal::Invalid));
        };
        check_url(&text, &path, kind)?;
        if urls.contains(&text) {
            return Err(ConfigError::new(path, Refusal::Invalid));
        }
        urls.push(text);
    }
    Ok(urls)
}

fn seeds(root: &mut Object, allow_loopback: bool) -> Result<Vec<String>, ConfigError> {
    let seeds = url_list(root, "seeds", MAX_SEEDS, UrlKind::Seed { allow_loopback })?;
    if seeds.is_empty() {
        return Err(ConfigError::new("seeds", Refusal::Missing));
    }
    Ok(seeds)
}

fn peers(root: &mut Object) -> Result<Vec<String>, ConfigError> {
    url_list(root, "peers", MAX_PEERS, UrlKind::Endpoint)
}

fn endpoint(object: &mut Object) -> Result<String, ConfigError> {
    let (text, path) = object.string("endpoint")?;
    check_url(&text, &path, UrlKind::Endpoint)?;
    Ok(text)
}

fn crawl(root: &mut Object) -> Result<CrawlConfig, ConfigError> {
    let mut object = root.object("crawl")?;
    let pages_per_cycle = positive_u32(&mut object, "pages_per_cycle", MAX_PAGES_PER_CYCLE)?;
    let pages_per_host = positive_u32(&mut object, "pages_per_host", pages_per_cycle)?;
    let (depth, path) = object.integer("max_depth")?;
    let max_depth = u32::try_from(depth)
        .ok()
        .filter(|depth| *depth <= MAX_DEPTH)
        .ok_or_else(|| ConfigError::new(path, Refusal::Invalid))?;
    let politeness_delay_ms = bounded(&mut object, "politeness_delay_ms", MAX_POLITENESS_DELAY_MS)?;
    object.finish()?;
    Ok(CrawlConfig {
        pages_per_cycle,
        pages_per_host,
        max_depth,
        politeness_delay_ms,
    })
}

fn crawl_interval_seconds(root: &mut Object) -> Result<u64, ConfigError> {
    let Some((value, path)) = root.optional("crawl_interval_seconds")? else {
        return Ok(DEFAULT_CRAWL_INTERVAL_SECONDS);
    };
    value
        .as_u64()
        .filter(|seconds| (1..=MAX_CRAWL_INTERVAL_SECONDS).contains(seconds))
        .ok_or_else(|| ConfigError::new(path, Refusal::Invalid))
}

fn fetch(root: &mut Object) -> Result<FetchLimits, ConfigError> {
    let mut object = root.object("fetch")?;
    let connect_timeout_ms = bounded(&mut object, "connect_timeout_ms", CONNECT_TIMEOUT_LIMIT_MS)?;
    let total_path = object.field_path("total_timeout_ms");
    let total_timeout_ms = bounded(&mut object, "total_timeout_ms", TOTAL_TIMEOUT_LIMIT_MS)?;
    if total_timeout_ms < connect_timeout_ms {
        return Err(ConfigError::new(total_path, Refusal::Invalid));
    }
    let max_body_bytes = bounded(&mut object, "max_body_bytes", BODY_LIMIT_BYTES)?;
    let (redirects, path) = object.integer("max_redirects")?;
    let max_redirects = u8::try_from(redirects)
        .ok()
        .filter(|redirects| *redirects <= REDIRECT_LIMIT)
        .ok_or_else(|| ConfigError::new(path, Refusal::Invalid))?;
    let (flag, path) = object.take("allow_loopback")?;
    let allow_loopback = flag
        .as_bool()
        .ok_or_else(|| ConfigError::new(path, Refusal::Invalid))?;
    object.finish()?;
    Ok(FetchLimits {
        connect_timeout_ms,
        total_timeout_ms,
        max_body_bytes,
        max_redirects,
        allow_loopback,
    })
}

fn price(object: &mut Object) -> Result<u128, ConfigError> {
    let (text, path) = object.string("price")?;
    if placeholder_text(&text) {
        return Err(ConfigError::new(path, Refusal::Placeholder));
    }
    if text.is_empty()
        || text.len() > 39
        || !text.bytes().all(|byte| byte.is_ascii_digit())
        || (text.len() > 1 && text.starts_with('0'))
    {
        return Err(ConfigError::new(path, Refusal::Invalid));
    }
    let amount: u128 = text
        .parse()
        .map_err(|_| ConfigError::new(path.clone(), Refusal::Invalid))?;
    if amount == 0 {
        return Err(ConfigError::new(path, Refusal::Placeholder));
    }
    Ok(amount)
}

fn assets(root: &mut Object) -> Result<[AssetConfig; 4], ConfigError> {
    let mut object = root.object("assets")?;
    let mut result = [AssetConfig {
        symbol: AssetSymbol::Sid,
        asset_id: [0; 32],
        price: 0,
    }; 4];
    for (slot, symbol) in result.iter_mut().zip(AssetSymbol::ALL) {
        let mut asset = object.object(symbol.code())?;
        let asset_id = hex32(&mut asset, "asset_id")?;
        let price = price(&mut asset)?;
        asset.finish()?;
        *slot = AssetConfig {
            symbol,
            asset_id,
            price,
        };
    }
    object.finish()?;
    for (index, asset) in result.iter().enumerate() {
        if result[..index]
            .iter()
            .any(|earlier| earlier.asset_id == asset.asset_id)
        {
            return Err(ConfigError::new(
                format!("assets.{}.asset_id", asset.symbol.code()),
                Refusal::Invalid,
            ));
        }
    }
    Ok(result)
}

fn gateway(root: &mut Object) -> Result<GatewayConfig, ConfigError> {
    let mut object = root.object("gateway")?;
    let endpoint = endpoint(&mut object)?;
    let sequencer_id = hex32(&mut object, "sequencer_id")?;
    let key_path = object.field_path("sequencer_public_key");
    let public_key = hex32(&mut object, "sequencer_public_key")?;
    let valid_key =
        ed25519_dalek::VerifyingKey::from_bytes(&public_key).is_ok_and(|key| !key.is_weak());
    if !valid_key {
        return Err(ConfigError::new(key_path, Refusal::Invalid));
    }
    object.finish()?;
    Ok(GatewayConfig {
        endpoint,
        sequencer: SequencerTrust {
            sequencer_id,
            public_key,
        },
    })
}

fn payment(root: &mut Object) -> Result<PaymentConfig, ConfigError> {
    let mut config = PaymentConfig::default();
    let Some((value, path)) = root.optional("payment")? else {
        return Ok(config);
    };
    let Value::Object(map) = value else {
        return Err(ConfigError::new(path, Refusal::Invalid));
    };
    let mut object = Object { path, map };
    if let Some((value, path)) = object.optional("payer_did")? {
        let text = optional_text(value, &path)?;
        if !payer_did_valid(&text) {
            return Err(ConfigError::new(path, Refusal::Invalid));
        }
        config.payer_did = Some(text);
    }
    if let Some((value, path)) = object.optional("draw_fee_limit")? {
        let text = optional_text(value, &path)?;
        config.draw_fee_limit = text
            .parse::<u128>()
            .ok()
            .filter(|limit| *limit != 0 && limit.to_string() == text)
            .ok_or_else(|| ConfigError::new(path, Refusal::Invalid))?;
    }
    if let Some((value, path)) = object.optional("conformance_suite")? {
        let text = optional_text(value, &path)?;
        if text.starts_with("/path/to") {
            return Err(ConfigError::new(path, Refusal::Placeholder));
        }
        config.conformance_suite = Some(
            absolute_directory(&text).ok_or_else(|| ConfigError::new(path, Refusal::Invalid))?,
        );
    }
    object.finish()?;
    Ok(config)
}

/// The text of an optional string field, refusing another JSON type as
/// malformed and a placeholder as a placeholder.
fn optional_text(value: Value, path: &str) -> Result<String, ConfigError> {
    let Value::String(text) = value else {
        return Err(ConfigError::new(path, Refusal::Invalid));
    };
    if placeholder_text(&text) {
        return Err(ConfigError::new(path, Refusal::Placeholder));
    }
    Ok(text)
}

fn evm(root: &mut Object) -> Result<EvmConfig, ConfigError> {
    let mut object = root.object("evm")?;
    let endpoint = endpoint(&mut object)?;
    let chain_id = bounded(&mut object, "chain_id", u64::MAX)?;
    let confirmations = positive_u32(&mut object, "confirmations", MAX_CONFIRMATIONS)?;
    object.finish()?;
    Ok(EvmConfig {
        endpoint,
        chain_id,
        confirmations,
    })
}
