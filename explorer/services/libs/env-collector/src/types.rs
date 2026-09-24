use derive_builder::Builder;
use std::path::PathBuf;

#[derive(Builder, Debug, Clone, PartialEq, Eq)]
#[builder(pattern = "mutable")]
pub struct EnvCollectorSettings {
    #[builder(setter(into))]
    pub service_name: String,
    #[builder(setter(into))]
    pub markdown_path: PathBuf,
    #[builder(setter(into))]
    pub config_path: PathBuf,
    #[builder(default = "PrefixFilter::Empty")]
    pub vars_filter: PrefixFilter,
    #[builder(default = "None", setter(into))]
    pub anchor_postfix: Option<String>,
    #[builder(default = "true")]
    pub format_markdown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixFilter {
    Whitelist(Vec<String>),
    Blacklist(Vec<String>),
    Empty,
}

impl PrefixFilter {
    pub fn whitelist(allow_only: &[&str]) -> Self {
        let list = allow_only.iter().map(|s| s.to_string()).collect();
        Self::Whitelist(list)
    }

    pub fn blacklist(vars_filter: &[&str]) -> Self {
        let list = vars_filter.iter().map(|s| s.to_string()).collect();
        Self::Blacklist(list)
    }

    pub fn filter(&self, string: &str) -> bool {
        let list = match self {
            PrefixFilter::Whitelist(list) | PrefixFilter::Blacklist(list) => list,
            PrefixFilter::Empty => &vec![],
        };
        let input_matches_some_prefix = list.iter().any(|prefix| string.starts_with(prefix));
        match self {
            PrefixFilter::Whitelist(_) => input_matches_some_prefix,
            PrefixFilter::Blacklist(_) => !input_matches_some_prefix,
            PrefixFilter::Empty => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_works() {
        let list = vec!["LOL", "KEK__"];
        let blacklist = PrefixFilter::blacklist(&list);
        assert!(!blacklist.filter("LOL"));
        assert!(!blacklist.filter("LOL_KEK"));
        assert!(blacklist.filter("KEK"));
        assert!(blacklist.filter("KEK_"));
        assert!(!blacklist.filter("KEK__"));
        assert!(!blacklist.filter("KEK__KKEKEKEKEK"));
        assert!(blacklist.filter("hesoyam"));

        let whitelist = PrefixFilter::whitelist(&list);
        assert!(whitelist.filter("LOL"));
        assert!(whitelist.filter("LOL_KEK"));
        assert!(!whitelist.filter("KEK"));
        assert!(!whitelist.filter("KEK_"));
        assert!(whitelist.filter("KEK__"));
        assert!(whitelist.filter("KEK__KKEKEKEKEK"));
        assert!(!whitelist.filter("hesoyam"));
    }

    #[test]
    fn settings_builder_works() {
        EnvCollectorSettingsBuilder::default()
            .build()
            .expect_err("should fail since service name is not set");
        EnvCollectorSettingsBuilder::default()
            .service_name("test")
            .build()
            .expect_err("should fail since markdown path is not set");
        EnvCollectorSettingsBuilder::default()
            .service_name("test")
            .markdown_path("test.md")
            .build()
            .expect_err("should fail since config path is not set");

        let settings = EnvCollectorSettingsBuilder::default()
            .service_name("test")
            .markdown_path("test.md")
            .config_path("test.toml")
            .build()
            .expect("wrong settings");
        let expected = EnvCollectorSettings {
            service_name: "test".to_string(),
            markdown_path: PathBuf::from("test.md"),
            config_path: PathBuf::from("test.toml"),
            vars_filter: PrefixFilter::Empty,
            anchor_postfix: None,
            format_markdown: true,
        };
        assert_eq!(settings, expected);

        let settings = EnvCollectorSettingsBuilder::default()
            .service_name("test".to_string())
            .markdown_path("test.md")
            .config_path("test.toml")
            .vars_filter(PrefixFilter::whitelist(&["LOL"]))
            .anchor_postfix(Some("cool_postfix".into()))
            .format_markdown(false)
            .build()
            .expect("wrong settings");
        let expected = EnvCollectorSettings {
            service_name: "test".to_string(),
            markdown_path: PathBuf::from("test.md"),
            config_path: PathBuf::from("test.toml"),
            vars_filter: PrefixFilter::whitelist(&["LOL"]),
            anchor_postfix: Some("cool_postfix".to_string()),
            format_markdown: false,
        };
        assert_eq!(settings, expected);
    }
}
