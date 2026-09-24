use anyhow::Context;
use clap::Parser;
use config::{Config, File};
use itertools::{Either, Itertools};
use json_dotpath::DotPaths;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    marker::PhantomData,
    path::{Path, PathBuf},
};

const ANCHOR_START: &str = "anchors.envs.start";
const ANCHOR_END: &str = "anchors.envs.end";

mod types;
pub use types::*;

#[deprecated(since = "0.2.0", note = "use run_env_collector_cli instead")]
pub fn run_env_collector_cli_old<S: Serialize + DeserializeOwned>(
    service_name: &str,
    markdown_path: &str,
    config_path: &str,
    vars_filter: PrefixFilter,
    anchor_postfix: Option<&str>,
) {
    let settings = EnvCollectorSettingsBuilder::default()
        .service_name(service_name.to_string())
        .markdown_path(markdown_path)
        .config_path(config_path)
        .vars_filter(vars_filter)
        .anchor_postfix(anchor_postfix.map(|s| s.to_string()))
        .build()
        .expect("wrong settings");
    run_env_collector_cli::<S>(settings);
}

pub fn run_env_collector_cli<S: Serialize + DeserializeOwned>(settings: EnvCollectorSettings) {
    let collector = EnvCollector::<S>::new(
        settings.service_name,
        settings.markdown_path.clone(),
        settings.config_path,
        settings.vars_filter,
        settings.anchor_postfix,
        settings.format_markdown,
    );
    let options = EnvCollectorOptions::parse();
    let incorrect = collector
        .verify_markdown(&options)
        .expect("Failed to find incorrect variables");
    if incorrect.is_empty() {
        println!("All variables are documented correctly");
    } else {
        println!("Found incorrect variables:");
        for env in incorrect {
            println!("({})\t{}", env.tag(), env.inner().key);
        }

        if options.validate_only {
            std::process::exit(1);
        } else {
            println!(
                "Ready to update markdown file: {:?}",
                settings.markdown_path
            );
            println!("Press any key to continue...");
            std::io::stdin().read_line(&mut String::new()).unwrap();
            collector
                .update_markdown(&options)
                .expect("Failed to update markdown");
        }
    }
}

#[derive(Parser, Debug, Default)]
#[command(version, about, long_about = None)]
pub struct EnvCollectorOptions {
    /// Only validate the markdown file, do not update it
    #[arg(long)]
    validate_only: bool,
    /// Skip default values when validating and updating markdown
    #[arg(long)]
    ignore_defaults: bool,
    /// Do not remove variables from the markdown file absent in the config
    #[arg(long)]
    ignore_unused: bool,
}

#[derive(Debug, Clone)]
pub struct EnvCollector<S> {
    service_name: String,
    markdown_path: PathBuf,
    config_path: PathBuf,
    vars_filter: PrefixFilter,
    anchor_postfix: Option<String>,
    format_markdown: bool,

    settings: PhantomData<S>,
}

impl<S> EnvCollector<S>
where
    S: Serialize + DeserializeOwned,
{
    pub fn new(
        service_name: String,
        markdown_path: PathBuf,
        config_path: PathBuf,
        vars_filter: PrefixFilter,
        anchor_postfix: Option<String>,
        format_markdown: bool,
    ) -> Self {
        Self {
            service_name,
            markdown_path,
            config_path,
            vars_filter,
            anchor_postfix,
            format_markdown,
            settings: Default::default(),
        }
    }

    pub fn verify_markdown(
        &self,
        options: &EnvCollectorOptions,
    ) -> Result<Vec<ReportedVariable>, anyhow::Error> {
        find_mistakes_in_markdown::<S>(
            &self.service_name,
            self.markdown_path.as_path(),
            self.config_path.as_path(),
            self.vars_filter.clone(),
            self.anchor_postfix.clone(),
            options,
        )
    }

    pub fn update_markdown(&self, options: &EnvCollectorOptions) -> Result<(), anyhow::Error> {
        update_markdown_file::<S>(
            &self.service_name,
            self.markdown_path.as_path(),
            self.config_path.as_path(),
            self.vars_filter.clone(),
            self.anchor_postfix.clone(),
            self.format_markdown,
            options,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportedVariable {
    /// The variable is missing or some of its fields are incorrect
    Incorrect(EnvVariable),
    /// The variable is present in the markdown, but not in the config
    Unused(EnvVariable),
}

impl ReportedVariable {
    pub fn inner(&self) -> &EnvVariable {
        match self {
            ReportedVariable::Incorrect(var) => var,
            ReportedVariable::Unused(var) => var,
        }
    }

    pub fn into_inner(self) -> EnvVariable {
        match self {
            ReportedVariable::Incorrect(var) => var,
            ReportedVariable::Unused(var) => var,
        }
    }

    pub fn tag(&self) -> &'static str {
        match self {
            ReportedVariable::Incorrect(_) => "incorrect",
            ReportedVariable::Unused(_) => "unused",
        }
    }
}

#[derive(Debug, Clone, Ord, PartialOrd, PartialEq, Eq)]
pub struct EnvVariable {
    pub key: String,
    pub description: String,
    pub required: bool,
    pub default_value: Option<String>,
    pub table_index: Option<usize>,
}

fn filter_non_ascii(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii()).collect()
}

impl EnvVariable {
    fn strings_equal_in_ascii(lhs: &str, rhs: &str) -> bool {
        filter_non_ascii(lhs) == filter_non_ascii(rhs)
    }

    pub fn eq_with_ignores(&self, other: &Self, ignore_defaults: bool) -> bool {
        let are_defaults_equal = if ignore_defaults {
            true
        } else {
            self.default_value == other.default_value
        };
        Self::strings_equal_in_ascii(&self.key, &other.key)
            && self.required == other.required
            && are_defaults_equal
    }
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq)]
struct Envs {
    pub vars: BTreeMap<String, EnvVariable>,
}

impl From<BTreeMap<String, EnvVariable>> for Envs {
    fn from(vars: BTreeMap<String, EnvVariable>) -> Self {
        Self { vars }
    }
}

impl Envs {
    pub fn from_example<S>(
        service_prefix: &str,
        example_config_path: &str,
        vars_filter: PrefixFilter,
    ) -> Result<Self, anyhow::Error>
    where
        S: Serialize + DeserializeOwned,
    {
        let settings: S = Config::builder()
            .add_source(File::with_name(example_config_path))
            .build()
            .context("failed to build config")?
            .try_deserialize()
            .context("failed to deserialize config")?;
        let json = serde_json::to_value(&settings).context("failed to convert config to json")?;
        let from_config: Envs = flatten_json(&json, service_prefix)
            .into_iter()
            .filter(|(key, _)| vars_filter.filter(key))
            .map(|(key, value)| {
                let default_value =
                    default_of_var(&settings, &from_key_to_json_path(&key, service_prefix));
                let required = default_value.is_none();
                let description = try_get_description(&key, &value, &default_value);
                let default_value =
                    default_value.map(|v| format!("`{}`", json_value_to_env_value(&v)));
                let var = EnvVariable {
                    key: key.clone(),
                    required,
                    default_value,
                    description,
                    // No order in json
                    table_index: None,
                };

                (key, var)
            })
            .collect::<BTreeMap<_, _>>()
            .into();

        Ok(from_config)
    }

    pub fn from_markdown(
        markdown_content: &str,
        anchor_postfix: Option<String>,
    ) -> Result<Self, anyhow::Error> {
        let start_anchor = push_postfix_to_anchor(ANCHOR_START, anchor_postfix.clone());
        let line_start = markdown_content
            .find(&start_anchor)
            .context("anchors.envs.start not found")?
            + start_anchor.len();
        let end_anchor = push_postfix_to_anchor(ANCHOR_END, anchor_postfix);
        let line_end = markdown_content
            .find(&end_anchor)
            .context("anchors.envs.end not found")?
            - 1;
        let table_content = &markdown_content[line_start..=line_end];

        let re = regex::Regex::new(regex_md_table_row()).context("regex creation")?;
        let result = re
            .captures_iter(table_content)
            .map(|c| c.extract())
            .enumerate()
            .map(
                |(index, (_, [key, required, description, default_value]))| {
                    let id = filter_non_ascii(key);
                    let required = required.trim().eq("true");
                    let default_value = if default_value.trim().is_empty() {
                        None
                    } else {
                        Some(default_value.trim().to_string())
                    };
                    let var = EnvVariable {
                        key: key.to_string(),
                        default_value,
                        required,
                        description: description.trim().to_string(),
                        table_index: Some(index),
                    };

                    (id, var)
                },
            )
            .collect::<BTreeMap<_, _>>()
            .into();

        Ok(result)
    }

    pub fn update_no_override(&mut self, other: Envs, ignore_defaults: bool) {
        for (id, value) in other.vars {
            let entry = self.vars.entry(id).or_insert(value.clone());
            if !ignore_defaults {
                entry.default_value = value.default_value;
            }
            entry.required = value.required;
        }
    }

    pub fn remove_unused_envs(&mut self, used: &Envs) {
        self.vars.retain(|id, _| used.vars.contains_key(id));
    }

    /// Preserve order of variables with `table_index`, sort others alphabetically
    /// according to their id (~key) (required go first).
    pub fn sorted_with_required(&self) -> Vec<EnvVariable> {
        let mut result = Vec::with_capacity(self.vars.len());
        let (mut vars_with_index, mut vars_no_index): (BTreeMap<_, _>, BTreeMap<_, _>) =
            self.vars.iter().partition_map(|(id, var)| {
                if let Some(i) = var.table_index {
                    Either::Left((i, var))
                } else {
                    Either::Right(((!var.required, id), var))
                }
            });
        loop {
            let i = result.len();
            if let Some(var) = vars_with_index.remove(&i) {
                result.push(var.clone());
            } else if let Some((_, var)) = vars_no_index.pop_first() {
                result.push(var.clone());
            } else if let Some((_, var)) = vars_with_index.pop_first() {
                result.push(var.clone());
            } else {
                break;
            }
        }
        result
    }
}

fn find_mistakes_in_markdown<S>(
    service_name: &str,
    markdown_path: &Path,
    config_path: &Path,
    vars_filter: PrefixFilter,
    anchor_postfix: Option<String>,
    options: &EnvCollectorOptions,
) -> Result<Vec<ReportedVariable>, anyhow::Error>
where
    S: Serialize + DeserializeOwned,
{
    let example = Envs::from_example::<S>(
        service_name,
        config_path
            .to_str()
            .expect("config path is not valid utf-8"),
        vars_filter,
    )?;
    let markdown: Envs = Envs::from_markdown(
        std::fs::read_to_string(markdown_path)
            .context("failed to read markdown file")?
            .as_str(),
        anchor_postfix,
    )?;

    let mut incorrect: Vec<_> = example
        .vars
        .iter()
        .filter(|(id, value)| {
            let maybe_markdown_var = markdown.vars.get(*id);
            maybe_markdown_var
                .map(|var| !var.eq_with_ignores(value, options.ignore_defaults))
                .unwrap_or(true)
        })
        .map(|(_, value)| ReportedVariable::Incorrect(value.clone()))
        .collect();

    if !options.ignore_unused {
        let unused = markdown
            .vars
            .iter()
            .filter(|(id, _)| !example.vars.contains_key(id.as_str()));
        incorrect.extend(unused.map(|(_, value)| ReportedVariable::Unused(value.clone())));
    }

    Ok(incorrect)
}

fn update_markdown_file<S>(
    service_name: &str,
    markdown_path: &Path,
    config_path: &Path,
    vars_filter: PrefixFilter,
    anchor_postfix: Option<String>,
    format_markdown: bool,
    options: &EnvCollectorOptions,
) -> Result<(), anyhow::Error>
where
    S: Serialize + DeserializeOwned,
{
    let from_config = Envs::from_example::<S>(
        service_name,
        config_path
            .to_str()
            .expect("config path is not valid utf-8"),
        vars_filter,
    )?;
    let mut markdown_config = Envs::from_markdown(
        std::fs::read_to_string(markdown_path)
            .context("failed to read markdown file")?
            .as_str(),
        anchor_postfix.clone(),
    )?;
    if !options.ignore_unused {
        markdown_config.remove_unused_envs(&from_config);
    }
    markdown_config.update_no_override(from_config, options.ignore_defaults);
    let table = serialize_env_vars_to_md_table(markdown_config, format_markdown);

    let content = std::fs::read_to_string(markdown_path).context("failed to read markdown file")?;
    let lines = content.lines().collect::<Vec<&str>>();
    let line_start = lines
        .iter()
        .position(|line| {
            line.contains(&push_postfix_to_anchor(
                ANCHOR_START,
                anchor_postfix.clone(),
            ))
        })
        .context("anchors.envs.start not found in markdown")?;
    let line_end = lines
        .iter()
        .position(|line| line.contains(&push_postfix_to_anchor(ANCHOR_END, anchor_postfix.clone())))
        .context("anchors.envs.end not found in markdown")?;

    let new_content = [&lines[..=line_start], &[&table], &lines[line_end..]].concat();
    std::fs::write(markdown_path, new_content.join("\n")).context("failed to write file")?;
    Ok(())
}

fn default_of_var<S>(settings: &S, path: &str) -> Option<serde_json::Value>
where
    S: Serialize + DeserializeOwned,
{
    let mut json = serde_json::to_value(settings).expect("structure should be serializable");
    json.dot_remove(path).expect("value path not found");

    let settings_with_default_value = serde_json::from_value::<S>(json).ok()?;
    let json: serde_json::Value = serde_json::to_value(&settings_with_default_value)
        .expect("structure should be serializable");
    let default_value: serde_json::Value = json
        .dot_get(path)
        .expect("value path not found")
        .unwrap_or_default();
    Some(default_value)
}

fn try_get_description(_key: &str, value: &str, default: &Option<serde_json::Value>) -> String {
    if value.is_empty() {
        return Default::default();
    }
    let default_str = default
        .as_ref()
        .map(json_value_to_env_value)
        .unwrap_or_default();

    // If the value is the same as the default value, we don't need to show it in the description
    if default_str == value {
        return Default::default();
    }

    format!("e.g. `{value}`")
}

fn from_key_to_json_path(key: &str, service_prefix: &str) -> String {
    key.trim_start_matches(&format!("{service_prefix}__"))
        .to_lowercase()
        .replace("__", ".")
        .to_string()
}

fn serialize_env_vars_to_md_table(vars: Envs, format_markdown: bool) -> String {
    // zero-width spaces in "Required" so that
    // the word can be broken down and
    // its colum doesn't take unnecessary space
    let mut result = r#"
| Variable | Req&#x200B;uir&#x200B;ed | Description | Default value |
| --- | --- | --- | --- |
"#
    .to_string();

    for env in vars.sorted_with_required() {
        let required = if env.required { " true " } else { " " };
        let description = if env.description.is_empty() {
            " ".to_string()
        } else {
            format!(" {} ", env.description)
        };
        let default_value = env
            .default_value
            .as_ref()
            .map(|v| format!(" {v} "))
            .unwrap_or(" ".to_string());
        result.push_str(&format!(
            "| `{}` |{}|{}|{}|\n",
            env.key, required, description, default_value,
        ));
    }
    if format_markdown {
        markdown_table_formatter::format_tables(&result)
    } else {
        result
    }
}

fn push_postfix_to_anchor(anchor: &str, postfix: Option<String>) -> String {
    if let Some(postfix) = postfix {
        format!("{anchor}.{postfix}")
    } else {
        anchor.to_string()
    }
}

fn regex_md_table_row() -> &'static str {
    concat!(
        r"\|\s*`([^|`]*)`\s*",
        r"\|\s*([^|]*)\s*",
        r"\|\s*([^|]*)\s*",
        r"\|\s*([^|]*)\s*",
        r#"\|"#
    )
}

fn flatten_json(json: &Value, initial_prefix: &str) -> BTreeMap<String, String> {
    let mut env_vars = BTreeMap::new();
    _flat_json(json, initial_prefix, &mut env_vars);
    env_vars
}

fn json_value_to_env_value(json: &Value) -> String {
    match json {
        Value::String(s) => s.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(v) => v
            .iter()
            .map(json_value_to_env_value)
            .collect::<Vec<String>>()
            .join(","),
        _ => panic!("unsupported value type: {json:?}"),
    }
}

fn _flat_json(json: &Value, prefix: &str, env_vars: &mut BTreeMap<String, String>) {
    match json {
        Value::Object(map) => {
            for (key, value) in map {
                let new_prefix = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}__{key}")
                };
                _flat_json(value, &new_prefix, env_vars);
            }
        }
        _ => {
            let env_var_name = prefix.to_uppercase();
            let env_var_value = json_value_to_env_value(json);
            env_vars.insert(env_var_name, env_var_value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockscout_service_launcher::database::DatabaseSettings;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use std::{collections::HashSet, io::Write};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestSettings {
        pub test: String,
        #[serde(default = "default_test2")]
        pub test2: i32,
        pub test3_set: Option<bool>,
        pub test4_not_set: Option<bool>,
        #[serde(default)]
        pub test5_with_unicode: bool,
        #[serde(default = "very_cool_string")]
        pub string_with_default: String,
        pub database: DatabaseSettings,
    }

    fn default_test2() -> i32 {
        1000
    }

    fn very_cool_string() -> String {
        "kekek".into()
    }

    fn var(
        key: &str,
        val: Option<&str>,
        required: bool,
        description: &str,
    ) -> (String, EnvVariable) {
        (
            key.into(),
            EnvVariable {
                key: key.to_string(),
                default_value: val.map(str::to_string),
                required,
                description: description.into(),
                table_index: None,
            },
        )
    }

    fn tempfile_with_content(content: &str, format: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new().suffix(format).tempfile().unwrap();
        writeln!(file, "{content}").unwrap();
        file
    }

    fn default_config_example_file_toml() -> tempfile::NamedTempFile {
        let content = r#"test = "value"
        test2 = 123
        test3_set = false
        [database.connect]
        url = "test-url"
        "#;
        tempfile_with_content(content, ".toml")
    }

    fn default_config_example_file_json() -> tempfile::NamedTempFile {
        let content = r#"{
            "test": "value",
            "test2": 123,
            "test3_set": false,
            "database": {
                "connect": {
                    "url": "test-url"
                }
            }
        }"#;
        tempfile_with_content(content, ".json")
    }

    fn default_envs() -> Envs {
        Envs::from(BTreeMap::from_iter(vec![
            var("TEST_SERVICE__TEST", None, true, "e.g. `value`"),
            var(
                "TEST_SERVICE__DATABASE__CREATE_DATABASE",
                Some("`false`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__RUN_MIGRATIONS",
                Some("`false`"),
                false,
                "",
            ),
            var("TEST_SERVICE__TEST2", Some("`1000`"), false, "e.g. `123`"),
            var(
                "TEST_SERVICE__TEST3_SET",
                Some("`null`"),
                false,
                "e.g. `false`",
            ),
            var("TEST_SERVICE__TEST4_NOT_SET", Some("`null`"), false, ""),
            var(
                "TEST_SERVICE__TEST5_WITH_UNICODE",
                Some("`false`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__STRING_WITH_DEFAULT",
                Some("`kekek`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT__URL",
                None,
                true,
                "e.g. `test-url`",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__MAX_CONNECTIONS",
                Some("`null`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__MIN_CONNECTIONS",
                Some("`null`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__CONNECT_TIMEOUT",
                Some("`null`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__CONNECT_LAZY",
                Some("`false`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__IDLE_TIMEOUT",
                Some("`null`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__ACQUIRE_TIMEOUT",
                Some("`null`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__MAX_LIFETIME",
                Some("`null`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__POSTGRES_APPLICATION_NAME",
                Some("`null`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__POSTGRES_STATEMENT_TIMEOUT",
                Some("`null`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_LOGGING",
                Some("`true`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_LOGGING_LEVEL",
                Some("`debug`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_SLOW_STATEMENTS_LOGGING_LEVEL",
                Some("`off`"),
                false,
                "",
            ),
            var(
                "TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_SLOW_STATEMENTS_LOGGING_THRESHOLD",
                Some("`1`"),
                false,
                "",
            ),
        ]))
    }

    fn default_markdown_content() -> &'static str {
        r#"

[anchor]: <> (anchors.envs.start.irrelevant_postfix)
[anchor]: <> (anchors.envs.end.irrelevant_postfix)

[anchor]: <> (anchors.envs.start.cool_postfix)

| Variable                                                      | Required    | Description      | Default Value |
|---------------------------------------------------------------|-------------|------------------|---------------|
| `TEST_SERVICE__TEST`                                          | true        | e.g. `value`     |               |
| `TEST_SERVICE__DATABASE__CREATE_DATABASE`                     | false       |                  | `false`       |
| `TEST_SERVICE__DATABASE__RUN_MIGRATIONS`                      | false       |                  | `false`       |
| `TEST_SERVICE__TEST2`                                         | false       | e.g. `123`       | `1000`        |
| `TEST_SERVICE__TEST3_SET`                                     | false       | e.g. `false`     | `null`        |
| `TEST_SERVICE__TEST4_NOT_SET`                                 | false       |                  | `null`        |
| `TEST_SERVICE__TEST5_WITH_UNICODE`                            | false       |                  | `false`       |
| `TEST_SERVICE__STRING_WITH_DEFAULT`                           | false       |                  | `kekek`       |
| `TEST_SERVICE__DATABASE__CONNECT__URL`                        | true        | e.g. `test-url`  |               |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__CONNECT_LAZY`       | false       |                  | `false`       |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__ACQUIRE_TIMEOUT` | | | `null` |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__CONNECT_TIMEOUT` | | | `null` |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__IDLE_TIMEOUT` | | | `null` |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__MAX_CONNECTIONS` | | | `null` |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__MAX_LIFETIME` | | | `null` |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__MIN_CONNECTIONS` | | | `null` |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__POSTGRES_APPLICATION_NAME` | | | `null` |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__POSTGRES_STATEMENT_TIMEOUT` | | | `null` |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_LOGGING` | | | `true` |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_LOGGING_LEVEL` | | | `debug` |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_SLOW_STATEMENTS_LOGGING_LEVEL` | | | `off` |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_SLOW_STATEMENTS_LOGGING_THRESHOLD` | | | `1` |
[anchor]: <> (anchors.envs.end.cool_postfix)
"#
    }

    #[test]
    fn from_toml_example_works() {
        let example_file = default_config_example_file_toml();
        let vars = Envs::from_example::<TestSettings>(
            "TEST_SERVICE",
            example_file.path().to_str().unwrap(),
            PrefixFilter::Empty,
        )
        .unwrap();
        let expected = default_envs();
        assert_eq!(vars, expected);
    }

    #[test]
    fn from_json_example_works() {
        let example_file = default_config_example_file_json();
        let vars = Envs::from_example::<TestSettings>(
            "TEST_SERVICE",
            example_file.path().to_str().unwrap(),
            PrefixFilter::Empty,
        )
        .unwrap();
        let expected = default_envs();
        assert_eq!(vars, expected);
    }

    #[test]
    fn from_markdown_works() {
        let markdown = default_markdown_content();
        let mut vars = Envs::from_markdown(markdown, Some("cool_postfix".to_string())).unwrap();
        // purge indices for correct comparison
        for (_, var) in vars.vars.iter_mut() {
            var.table_index = None;
        }
        let expected = default_envs();
        assert_eq!(vars, expected);
    }

    #[test]
    fn update_and_validate_works() {
        let mut markdown = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            markdown,
            r#"
[anchor]: <> (anchors.envs.start)
| `TEST_SERVICE__TEST5_WITH_UNICODE♡♡♡` | | the variable should be matched with `TEST_SERVICE__TEST5_WITH_UNICODE` and the unicode must be saved | `false` |
|`SOME_EXTRA_VARS`| | comment should be saved. `kek` |`example_value` |
|`SOME_EXTRA_VARS2`| true |        |`example_value2` |

[anchor]: <> (anchors.envs.end)
"#
        )
        .unwrap();

        let config = default_config_example_file_toml();

        let collector = EnvCollector::<TestSettings>::new(
            "TEST_SERVICE".to_string(),
            markdown.path().to_path_buf(),
            config.path().to_path_buf(),
            PrefixFilter::Empty,
            None,
            true,
        );
        let options = EnvCollectorOptions {
            ignore_unused: true,
            ..Default::default()
        };

        let incorrect = collector
            .verify_markdown(&options)
            .unwrap()
            .into_iter()
            .map(|v| v.into_inner())
            .collect::<Vec<_>>();

        assert_eq!(
            incorrect,
            default_envs()
                .vars
                .values()
                .filter(|var| var.key != "TEST_SERVICE__TEST5_WITH_UNICODE")
                .map(Clone::clone)
                .collect::<Vec<EnvVariable>>()
        );

        collector.update_markdown(&options).unwrap();
        let incorrect = collector.verify_markdown(&options).unwrap();
        assert_eq!(incorrect, vec![]);

        let markdown_content = std::fs::read_to_string(markdown.path()).unwrap();
        assert_eq!(
            markdown_content,
            r#"
[anchor]: <> (anchors.envs.start)

| Variable                                                                          | Req&#x200B;uir&#x200B;ed | Description                                                                                          | Default value    |
| --------------------------------------------------------------------------------- | ------------------------ | ---------------------------------------------------------------------------------------------------- | ---------------- |
| `TEST_SERVICE__TEST5_WITH_UNICODE♡♡♡`                                             |                          | the variable should be matched with `TEST_SERVICE__TEST5_WITH_UNICODE` and the unicode must be saved | `false`          |
| `SOME_EXTRA_VARS`                                                                 |                          | comment should be saved. `kek`                                                                       | `example_value`  |
| `SOME_EXTRA_VARS2`                                                                | true                     |                                                                                                      | `example_value2` |
| `TEST_SERVICE__DATABASE__CONNECT__URL`                                            | true                     | e.g. `test-url`                                                                                      |                  |
| `TEST_SERVICE__TEST`                                                              | true                     | e.g. `value`                                                                                         |                  |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__ACQUIRE_TIMEOUT`                        |                          |                                                                                                      | `null`           |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__CONNECT_LAZY`                           |                          |                                                                                                      | `false`          |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__CONNECT_TIMEOUT`                        |                          |                                                                                                      | `null`           |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__IDLE_TIMEOUT`                           |                          |                                                                                                      | `null`           |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__MAX_CONNECTIONS`                        |                          |                                                                                                      | `null`           |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__MAX_LIFETIME`                           |                          |                                                                                                      | `null`           |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__MIN_CONNECTIONS`                        |                          |                                                                                                      | `null`           |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__POSTGRES_APPLICATION_NAME`              |                          |                                                                                                      | `null`           |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__POSTGRES_STATEMENT_TIMEOUT`             |                          |                                                                                                      | `null`           |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_LOGGING`                           |                          |                                                                                                      | `true`           |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_LOGGING_LEVEL`                     |                          |                                                                                                      | `debug`          |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_SLOW_STATEMENTS_LOGGING_LEVEL`     |                          |                                                                                                      | `off`            |
| `TEST_SERVICE__DATABASE__CONNECT_OPTIONS__SQLX_SLOW_STATEMENTS_LOGGING_THRESHOLD` |                          |                                                                                                      | `1`              |
| `TEST_SERVICE__DATABASE__CREATE_DATABASE`                                         |                          |                                                                                                      | `false`          |
| `TEST_SERVICE__DATABASE__RUN_MIGRATIONS`                                          |                          |                                                                                                      | `false`          |
| `TEST_SERVICE__STRING_WITH_DEFAULT`                                               |                          |                                                                                                      | `kekek`          |
| `TEST_SERVICE__TEST2`                                                             |                          | e.g. `123`                                                                                           | `1000`           |
| `TEST_SERVICE__TEST3_SET`                                                         |                          | e.g. `false`                                                                                         | `null`           |
| `TEST_SERVICE__TEST4_NOT_SET`                                                     |                          |                                                                                                      | `null`           |

[anchor]: <> (anchors.envs.end)
"#
        );
    }

    #[test]
    fn override_defaults_works() {
        let mut markdown = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            markdown,
            r#"
[anchor]: <> (anchors.envs.start)
| `TEST_SERVICE__TEST5_WITH_UNICODE`  |                          | aboba         | `false`          |
| `TEST_SERVICE__TEST`                | true                     | e.g. `value`  |                  |
| `TEST_SERVICE__STRING_WITH_DEFAULT` |                          |               | `old_default`    |
[anchor]: <> (anchors.envs.end)
"#
        )
        .unwrap();

        let config = default_config_example_file_toml();

        let collector = EnvCollector::<TestSettings>::new(
            "TEST_SERVICE".to_string(),
            markdown.path().to_path_buf(),
            config.path().to_path_buf(),
            PrefixFilter::blacklist(&["TEST_SERVICE__DATABASE"]),
            None,
            true,
        );
        let options = EnvCollectorOptions::default();

        let incorrect = collector
            .verify_markdown(&options)
            .unwrap()
            .into_iter()
            .map(|v| v.into_inner())
            .collect::<Vec<_>>();
        assert_eq!(
            incorrect,
            default_envs()
                .vars
                .values()
                .filter(|var| var.key != "TEST_SERVICE__TEST"
                    && var.key != "TEST_SERVICE__TEST5_WITH_UNICODE"
                    // `STRING_WITH_DEFAULT` has wrong default, so it's reported as incorrect
                    && !var.key.starts_with("TEST_SERVICE__DATABASE"))
                .map(Clone::clone)
                .collect::<Vec<EnvVariable>>()
        );

        collector.update_markdown(&options).unwrap();
        let incorrect = collector.verify_markdown(&options).unwrap();
        assert_eq!(incorrect, vec![]);

        let markdown_content = std::fs::read_to_string(markdown.path()).unwrap();
        // check that default for `TEST_SERVICE__STRING_WITH_DEFAULT` is updated, as requested
        assert_eq!(
            markdown_content,
            r#"
[anchor]: <> (anchors.envs.start)

| Variable                            | Req&#x200B;uir&#x200B;ed | Description  | Default value |
| ----------------------------------- | ------------------------ | ------------ | ------------- |
| `TEST_SERVICE__TEST5_WITH_UNICODE`  |                          | aboba        | `false`       |
| `TEST_SERVICE__TEST`                | true                     | e.g. `value` |               |
| `TEST_SERVICE__STRING_WITH_DEFAULT` |                          |              | `kekek`       |
| `TEST_SERVICE__TEST2`               |                          | e.g. `123`   | `1000`        |
| `TEST_SERVICE__TEST3_SET`           |                          | e.g. `false` | `null`        |
| `TEST_SERVICE__TEST4_NOT_SET`       |                          |              | `null`        |

[anchor]: <> (anchors.envs.end)
"#
        );
    }

    #[test]
    fn remove_unused_works() {
        let mut markdown = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            markdown,
            r#"
[anchor]: <> (anchors.envs.start)
| `SOME_EXTRA_VARS`                   |      | comment should be saved. `kek` | `example_value`  |
| `SOME_EXTRA_VARS2`                  | true |                                | `example_value2` |
| `TEST_SERVICE__TEST5_WITH_UNICODE`  |      | aboba                          | `false`          |
| `TEST_SERVICE__TEST`                | true | e.g. `value`                   |                  |
| `TEST_SERVICE__STRING_WITH_DEFAULT` |      |                                | `kekek`          |
| `TEST_SERVICE__TEST2`               |      | e.g. `123`                     | `1000`           |
| `TEST_SERVICE__TEST3_SET`           |      | e.g. `false`                   | `null`           |
| `TEST_SERVICE__TEST4_NOT_SET`       |      |                                | `null`           |
[anchor]: <> (anchors.envs.end)
"#
        )
        .unwrap();

        let config = default_config_example_file_toml();

        let collector = EnvCollector::<TestSettings>::new(
            "TEST_SERVICE".to_string(),
            markdown.path().to_path_buf(),
            config.path().to_path_buf(),
            PrefixFilter::blacklist(&["TEST_SERVICE__DATABASE"]),
            None,
            true,
        );
        let options = EnvCollectorOptions::default();

        let incorrect = collector.verify_markdown(&options).unwrap();
        let mut expected_unused_keys = HashSet::from(["SOME_EXTRA_VARS", "SOME_EXTRA_VARS2"]);
        for reported in incorrect {
            match reported {
                ReportedVariable::Incorrect(env_variable) => {
                    panic!("must not have incorrect variables, got: {env_variable:?}")
                }
                ReportedVariable::Unused(env_variable) => {
                    assert!(
                        expected_unused_keys.remove(env_variable.key.as_str()),
                        "reported unused variable that was not expected: {env_variable:?}"
                    );
                }
            }
        }
        assert!(
            expected_unused_keys.is_empty(),
            "did not report these unused variables: {expected_unused_keys:?}"
        );

        collector.update_markdown(&options).unwrap();
        let incorrect = collector.verify_markdown(&options).unwrap();
        assert_eq!(incorrect, vec![]);

        let markdown_content = std::fs::read_to_string(markdown.path()).unwrap();
        assert_eq!(
            markdown_content,
            r#"
[anchor]: <> (anchors.envs.start)

| Variable                            | Req&#x200B;uir&#x200B;ed | Description  | Default value |
| ----------------------------------- | ------------------------ | ------------ | ------------- |
| `TEST_SERVICE__TEST5_WITH_UNICODE`  |                          | aboba        | `false`       |
| `TEST_SERVICE__TEST`                | true                     | e.g. `value` |               |
| `TEST_SERVICE__STRING_WITH_DEFAULT` |                          |              | `kekek`       |
| `TEST_SERVICE__TEST2`               |                          | e.g. `123`   | `1000`        |
| `TEST_SERVICE__TEST3_SET`           |                          | e.g. `false` | `null`        |
| `TEST_SERVICE__TEST4_NOT_SET`       |                          |              | `null`        |

[anchor]: <> (anchors.envs.end)
"#
        );
    }
}
