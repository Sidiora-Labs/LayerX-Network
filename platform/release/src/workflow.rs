#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Node {
    Scalar(String),
    Sequence(Vec<Node>),
    Mapping(Vec<(String, Node)>),
}

impl Node {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Mapping(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, node)| node),
            Node::Scalar(_) | Node::Sequence(_) => None,
        }
    }

    #[must_use]
    pub fn path(&self, keys: &[&str]) -> Option<&Node> {
        keys.iter().try_fold(self, |node, key| node.get(key))
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::Scalar(value) => Some(value.as_str()),
            Node::Sequence(_) | Node::Mapping(_) => None,
        }
    }

    #[must_use]
    pub fn items(&self) -> &[Node] {
        match self {
            Node::Sequence(items) => items.as_slice(),
            Node::Scalar(_) | Node::Mapping(_) => &[],
        }
    }

    #[must_use]
    pub fn entries(&self) -> &[(String, Node)] {
        match self {
            Node::Mapping(entries) => entries.as_slice(),
            Node::Scalar(_) | Node::Sequence(_) => &[],
        }
    }

    #[must_use]
    pub fn strings(&self) -> Vec<&str> {
        match self {
            Node::Scalar(value) => vec![value.as_str()],
            Node::Sequence(items) => items.iter().filter_map(Node::as_str).collect(),
            Node::Mapping(_) => Vec::new(),
        }
    }

    #[must_use]
    pub fn text(&self) -> String {
        let mut text = String::new();
        self.collect_text(&mut text);
        text
    }

    fn collect_text(&self, text: &mut String) {
        match self {
            Node::Scalar(value) => {
                text.push_str(value);
                text.push('\n');
            }
            Node::Sequence(items) => {
                for item in items {
                    item.collect_text(text);
                }
            }
            Node::Mapping(entries) => {
                for (key, node) in entries {
                    if let Node::Scalar(value) = node {
                        text.push_str(key);
                        text.push_str(": ");
                        text.push_str(value);
                        text.push('\n');
                    } else {
                        text.push_str(key);
                        text.push('\n');
                        node.collect_text(text);
                    }
                }
            }
        }
    }
}

struct Line {
    number: usize,
    indent: usize,
    raw: String,
    text: String,
}

struct Parser {
    lines: Vec<Line>,
    position: usize,
}

enum Chomp {
    Clip,
    Strip,
    Keep,
}

/// Parses the block-style YAML subset GitHub workflows are written in.
///
/// # Errors
///
/// Refuses tabs, anchors, aliases, tags, multi-line flow collections,
/// duplicate keys and any indentation that does not nest.
pub fn parse(source: &str) -> Result<Node, String> {
    let mut parser = Parser {
        lines: tokenize(source)?,
        position: 0,
    };
    parser.skip_blank();
    if parser
        .lines
        .get(parser.position)
        .is_some_and(|line| line.indent == 0 && line.text == "---")
    {
        parser.position += 1;
        parser.skip_blank();
    }
    let Some(first) = parser.lines.get(parser.position) else {
        return Err("workflow is empty".to_owned());
    };
    let indent = first.indent;
    let node = parser.block(indent)?;
    parser.skip_blank();
    if let Some(line) = parser.lines.get(parser.position) {
        return Err(format!("line {}: unexpected content", line.number));
    }
    Ok(node)
}

fn tokenize(source: &str) -> Result<Vec<Line>, String> {
    let mut lines = Vec::new();
    for (index, raw) in source.lines().enumerate() {
        let number = index + 1;
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        let content = &raw[indent..];
        if content.starts_with('\t') {
            return Err(format!("line {number}: tab indentation"));
        }
        let text = strip_comment(content).trim_end().to_owned();
        lines.push(Line {
            number,
            indent,
            raw: raw.to_owned(),
            text,
        });
    }
    Ok(lines)
}

fn strip_comment(content: &str) -> &str {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    let mut previous = ' ';
    for (index, character) in content.char_indices() {
        if double && escaped {
            escaped = false;
        } else if double && character == '\\' {
            escaped = true;
        } else if character == '"' && !single {
            double = !double;
        } else if character == '\'' && !double {
            single = !single;
        } else if character == '#' && !single && !double && previous.is_whitespace() {
            return &content[..index];
        }
        previous = character;
    }
    content
}

fn is_sequence_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

fn split_key(text: &str) -> Option<(String, String)> {
    let first = text.chars().next()?;
    if first == '"' || first == '\'' {
        let end = text[1..].find(first)? + 1;
        let key = text[1..end].to_owned();
        let rest = text[end + 1..].trim_start().strip_prefix(':')?;
        if !rest.is_empty() && !rest.starts_with(' ') {
            return None;
        }
        return Some((key, rest.trim().to_owned()));
    }
    if first == '[' || first == '{' {
        return None;
    }
    for (index, character) in text.char_indices() {
        if character != ':' {
            continue;
        }
        let rest = &text[index + 1..];
        if rest.is_empty() || rest.starts_with(' ') {
            let key = text[..index].trim_end();
            if key.is_empty() {
                return None;
            }
            return Some((key.to_owned(), rest.trim().to_owned()));
        }
    }
    None
}

fn quoted(text: &str, number: usize) -> Result<String, String> {
    let mut characters = text.chars();
    let Some(quote) = characters.next() else {
        return Err(format!("line {number}: empty scalar"));
    };
    let mut value = String::new();
    let mut closed = false;
    while let Some(character) = characters.next() {
        if quote == '"' && character == '\\' {
            match characters.next() {
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some(other) => value.push(other),
                None => return Err(format!("line {number}: unterminated escape")),
            }
            continue;
        }
        if character == quote {
            if quote == '\'' && characters.as_str().starts_with('\'') {
                characters.next();
                value.push('\'');
                continue;
            }
            closed = true;
            break;
        }
        value.push(character);
    }
    if !closed {
        return Err(format!("line {number}: unterminated quoted scalar"));
    }
    if !characters.as_str().trim().is_empty() {
        return Err(format!(
            "line {number}: content after the closing quote: {}",
            characters.as_str().trim()
        ));
    }
    Ok(value)
}

fn scalar(text: &str, number: usize) -> Result<Node, String> {
    if text.starts_with('"') || text.starts_with('\'') {
        return quoted(text, number).map(Node::Scalar);
    }
    if text.starts_with('&') || text.starts_with('*') || text.starts_with('!') {
        return Err(format!(
            "line {number}: anchors, aliases and tags are not supported"
        ));
    }
    Ok(Node::Scalar(text.to_owned()))
}

fn split_flow(inner: &str, number: usize) -> Result<Vec<String>, String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for character in inner.chars() {
        match quote {
            Some(open) => {
                current.push(character);
                if character == open {
                    quote = None;
                }
            }
            None => match character {
                '"' | '\'' => {
                    quote = Some(character);
                    current.push(character);
                }
                '[' | '{' | ']' | '}' => {
                    return Err(format!(
                        "line {number}: nested flow collections are not supported"
                    ));
                }
                ',' => {
                    items.push(current.trim().to_owned());
                    current.clear();
                }
                _ => current.push(character),
            },
        }
    }
    if quote.is_some() {
        return Err(format!("line {number}: unterminated quoted scalar"));
    }
    if !current.trim().is_empty() {
        items.push(current.trim().to_owned());
    } else if !items.is_empty() {
        return Err(format!("line {number}: trailing comma in flow collection"));
    }
    if items.iter().any(String::is_empty) {
        return Err(format!("line {number}: empty flow item"));
    }
    Ok(items)
}

fn flow_sequence(text: &str, number: usize) -> Result<Node, String> {
    let inner = text
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| format!("line {number}: flow sequence must close on the same line"))?;
    let items = split_flow(inner, number)?
        .iter()
        .map(|item| scalar(item, number))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Node::Sequence(items))
}

fn flow_mapping(text: &str, number: usize) -> Result<Node, String> {
    let inner = text
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .ok_or_else(|| format!("line {number}: flow mapping must close on the same line"))?;
    let mut entries: Vec<(String, Node)> = Vec::new();
    for item in split_flow(inner, number)? {
        let (key, value) = split_key(&item)
            .ok_or_else(|| format!("line {number}: flow mapping item {item} has no key"))?;
        if entries.iter().any(|(existing, _)| *existing == key) {
            return Err(format!("line {number}: duplicate key {key}"));
        }
        entries.push((key, scalar(&value, number)?));
    }
    Ok(Node::Mapping(entries))
}

fn fold(lines: &[String]) -> String {
    let mut text = String::new();
    let mut previous_blank = true;
    for line in lines {
        if line.is_empty() {
            text.push('\n');
            previous_blank = true;
            continue;
        }
        if !previous_blank {
            text.push(' ');
        }
        text.push_str(line);
        previous_blank = false;
    }
    text
}

impl Parser {
    fn skip_blank(&mut self) {
        while self
            .lines
            .get(self.position)
            .is_some_and(|line| line.text.is_empty())
        {
            self.position += 1;
        }
    }

    fn block(&mut self, indent: usize) -> Result<Node, String> {
        self.skip_blank();
        let Some(line) = self.lines.get(self.position) else {
            return Ok(Node::Scalar(String::new()));
        };
        if line.indent != indent {
            return Err(format!(
                "line {}: expected indentation {indent}, found {}",
                line.number, line.indent
            ));
        }
        if is_sequence_item(&line.text) {
            self.sequence(indent)
        } else {
            self.mapping(indent)
        }
    }

    fn mapping(&mut self, indent: usize) -> Result<Node, String> {
        let mut entries: Vec<(String, Node)> = Vec::new();
        loop {
            self.skip_blank();
            let Some(line) = self.lines.get(self.position) else {
                break;
            };
            if line.indent < indent {
                break;
            }
            let number = line.number;
            if line.indent > indent {
                return Err(format!("line {number}: unexpected indentation"));
            }
            if is_sequence_item(&line.text) {
                return Err(format!("line {number}: sequence item inside a mapping"));
            }
            let (key, rest) =
                split_key(&line.text).ok_or_else(|| format!("line {number}: expected a key"))?;
            if entries.iter().any(|(existing, _)| *existing == key) {
                return Err(format!("line {number}: duplicate key {key}"));
            }
            self.position += 1;
            let value = self.value(indent, &rest, number, true)?;
            entries.push((key, value));
        }
        Ok(Node::Mapping(entries))
    }

    fn sequence(&mut self, indent: usize) -> Result<Node, String> {
        let mut items = Vec::new();
        loop {
            self.skip_blank();
            let Some(line) = self.lines.get(self.position) else {
                break;
            };
            if line.indent < indent || !is_sequence_item(&line.text) {
                break;
            }
            let number = line.number;
            if line.indent > indent {
                return Err(format!("line {number}: unexpected indentation"));
            }
            let after = line.text[1..].to_owned();
            let extra = after.len() - after.trim_start().len();
            let item = after.trim_start().to_owned();
            if item.is_empty() {
                self.position += 1;
                items.push(self.value(indent, "", number, false)?);
            } else if is_sequence_item(&item) || split_key(&item).is_some() {
                let nested = indent + 1 + extra;
                if let Some(line) = self.lines.get_mut(self.position) {
                    line.indent = nested;
                    line.text = item;
                }
                items.push(self.block(nested)?);
            } else {
                self.position += 1;
                items.push(self.value(indent, &item, number, false)?);
            }
        }
        Ok(Node::Sequence(items))
    }

    fn value(
        &mut self,
        indent: usize,
        rest: &str,
        number: usize,
        sibling_sequence: bool,
    ) -> Result<Node, String> {
        let rest = rest.trim();
        if rest.is_empty() {
            self.skip_blank();
            return match self.lines.get(self.position) {
                Some(line) if line.indent > indent => {
                    let deeper = line.indent;
                    self.block(deeper)
                }
                Some(line)
                    if sibling_sequence
                        && line.indent == indent
                        && is_sequence_item(&line.text) =>
                {
                    self.sequence(indent)
                }
                _ => Ok(Node::Scalar(String::new())),
            };
        }
        if let Some(header) = rest.strip_prefix('|') {
            return self.block_scalar(indent, false, header, number);
        }
        if let Some(header) = rest.strip_prefix('>') {
            return self.block_scalar(indent, true, header, number);
        }
        if rest.starts_with('[') {
            return flow_sequence(rest, number);
        }
        if rest.starts_with('{') {
            return flow_mapping(rest, number);
        }
        if rest.starts_with('"') || rest.starts_with('\'') {
            return scalar(rest, number);
        }
        let mut plain = match scalar(rest, number)? {
            Node::Scalar(value) => value,
            Node::Sequence(_) | Node::Mapping(_) => {
                return Err(format!("line {number}: expected a scalar"))
            }
        };
        while let Some(line) = self.lines.get(self.position) {
            if line.text.is_empty() || line.indent <= indent {
                break;
            }
            plain.push(' ');
            plain.push_str(line.text.trim());
            self.position += 1;
        }
        Ok(Node::Scalar(plain))
    }

    fn block_scalar(
        &mut self,
        indent: usize,
        folded: bool,
        header: &str,
        number: usize,
    ) -> Result<Node, String> {
        let chomp = match header {
            "" => Chomp::Clip,
            "-" => Chomp::Strip,
            "+" => Chomp::Keep,
            _ => {
                return Err(format!(
                    "line {number}: unsupported block scalar header {header}"
                ))
            }
        };
        let mut content_indent: Option<usize> = None;
        let mut lines: Vec<String> = Vec::new();
        while let Some(line) = self.lines.get(self.position) {
            if line.raw.trim().is_empty() {
                lines.push(String::new());
                self.position += 1;
                continue;
            }
            if line.indent <= indent {
                break;
            }
            let expected = *content_indent.get_or_insert(line.indent);
            if line.indent < expected {
                return Err(format!(
                    "line {}: block scalar line is less indented than its first line",
                    line.number
                ));
            }
            lines.push(line.raw[expected..].to_owned());
            self.position += 1;
        }
        let trailing = lines
            .iter()
            .rev()
            .take_while(|line| line.is_empty())
            .count();
        lines.truncate(lines.len() - trailing);
        let body = if folded {
            fold(&lines)
        } else {
            lines.join("\n")
        };
        let text = match chomp {
            Chomp::Strip => body,
            Chomp::Clip if body.is_empty() => body,
            Chomp::Clip => format!("{body}\n"),
            Chomp::Keep => format!("{body}\n{}", "\n".repeat(trailing)),
        };
        Ok(Node::Scalar(text))
    }
}
