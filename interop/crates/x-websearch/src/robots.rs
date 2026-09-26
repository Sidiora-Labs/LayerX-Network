use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The product token every robots.txt group is matched against.
pub const USER_AGENT_TOKEN: &str = "x-websearch";

/// How long a parsed robots.txt is reused for its origin.
pub const CACHE_LIFETIME: Duration = Duration::from_secs(86_400);

const MAX_CACHED_ORIGINS: usize = 4_096;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Rule {
    allow: bool,
    pattern: String,
}

/// The robots.txt rules that apply to the user-agent token `x-websearch`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Robots {
    rules: Vec<Rule>,
}

#[derive(Default)]
struct Group {
    agents: Vec<String>,
    rules: Vec<Rule>,
    has_rule_lines: bool,
}

impl Robots {
    /// No rules: every path is allowed.
    #[must_use]
    pub const fn allow_all() -> Self {
        Self { rules: Vec::new() }
    }

    /// One rule disallowing every path.
    #[must_use]
    pub fn disallow_all() -> Self {
        Self {
            rules: vec![Rule {
                allow: false,
                pattern: "/".to_owned(),
            }],
        }
    }

    /// Parses robots.txt and keeps the groups for `x-websearch`, or the `*`
    /// groups when no group names the token.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut groups: Vec<Group> = Vec::new();
        let mut current = Group::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or_default().trim();
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            match key.as_str() {
                "user-agent" => {
                    if current.has_rule_lines {
                        groups.push(std::mem::take(&mut current));
                    }
                    let agent = value
                        .split(|character: char| character == '/' || character.is_whitespace())
                        .next()
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    current.agents.push(agent);
                }
                "allow" | "disallow" if !current.agents.is_empty() => {
                    current.has_rule_lines = true;
                    if !value.is_empty() {
                        current.rules.push(Rule {
                            allow: key == "allow",
                            pattern: value.to_owned(),
                        });
                    }
                }
                _ => {}
            }
        }
        if !current.agents.is_empty() {
            groups.push(current);
        }
        let named = |token: &str| -> Vec<Rule> {
            groups
                .iter()
                .filter(|group| group.agents.iter().any(|agent| agent == token))
                .flat_map(|group| group.rules.iter().cloned())
                .collect()
        };
        let own = groups
            .iter()
            .any(|group| group.agents.iter().any(|agent| agent == USER_AGENT_TOKEN));
        Self {
            rules: if own {
                named(USER_AGENT_TOKEN)
            } else {
                named("*")
            },
        }
    }

    /// Whether a path with its query may be fetched: the longest matching
    /// rule decides, an allow wins a tie, and `/robots.txt` is always allowed.
    #[must_use]
    pub fn allows(&self, path: &str) -> bool {
        if path == "/robots.txt" {
            return true;
        }
        let mut best: Option<(usize, bool)> = None;
        for rule in &self.rules {
            if !pattern_matches(&rule.pattern, path) {
                continue;
            }
            let length = rule.pattern.len();
            best = match best {
                Some((best_length, best_allow))
                    if best_length > length || (best_length == length && best_allow) =>
                {
                    Some((best_length, best_allow))
                }
                _ => Some((length, rule.allow)),
            };
        }
        best.is_none_or(|(_, allow)| allow)
    }
}

fn pattern_matches(pattern: &str, path: &str) -> bool {
    let (pattern, anchored) = match pattern.strip_suffix('$') {
        Some(stripped) => (stripped, true),
        None => (pattern, false),
    };
    let pieces: Vec<&str> = pattern.split('*').collect();
    let Some((first, others)) = pieces.split_first() else {
        return false;
    };
    let Some(mut rest) = path.strip_prefix(first) else {
        return false;
    };
    if others.is_empty() {
        return !anchored || rest.is_empty();
    }
    let Some((last, middle)) = others.split_last() else {
        return false;
    };
    for piece in middle {
        match rest.find(piece) {
            Some(index) => rest = &rest[index + piece.len()..],
            None => return false,
        }
    }
    if anchored {
        rest.ends_with(last)
    } else {
        rest.contains(last)
    }
}

/// Parsed robots.txt per origin, each kept for [`CACHE_LIFETIME`].
#[derive(Default)]
pub struct RobotsCache {
    entries: Mutex<HashMap<String, (Instant, Robots)>>,
}

impl RobotsCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The cached rules for an origin, when still fresh.
    #[must_use]
    pub fn get(&self, origin: &str) -> Option<Robots> {
        let entries = self.entries.lock().ok()?;
        entries
            .get(origin)
            .filter(|(stored, _)| stored.elapsed() < CACHE_LIFETIME)
            .map(|(_, robots)| robots.clone())
    }

    pub fn insert(&self, origin: &str, robots: Robots) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        if entries.len() >= MAX_CACHED_ORIGINS {
            entries.retain(|_, (stored, _)| stored.elapsed() < CACHE_LIFETIME);
        }
        if entries.len() < MAX_CACHED_ORIGINS {
            entries.insert(origin.to_owned(), (Instant::now(), robots));
        }
    }
}
