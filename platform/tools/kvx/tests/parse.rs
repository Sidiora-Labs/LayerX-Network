use layerx_platform_kvx::{parse, quote, string_list, unquote};

#[test]
fn parses_sections_keys_and_quoted_keys() {
    let source =
        "\n[alpha]\nname = \"one\"\n\"a b.c\" = \"two\"\n\n[beta]\nitems = [\"x\", \"y\"]\n";
    let document = parse(source).unwrap_or_else(|error| panic!("parse: {error}"));
    assert_eq!(document.sections(), vec!["alpha", "beta"]);
    assert_eq!(document.get("alpha", "name"), Some("\"one\""));
    assert_eq!(document.get("alpha", "a b.c"), Some("\"two\""));
    assert_eq!(
        document.section_entries("alpha"),
        vec![("name", "\"one\""), ("a b.c", "\"two\"")]
    );
    let items = string_list(
        document
            .get("beta", "items")
            .unwrap_or_else(|| panic!("items missing")),
    )
    .unwrap_or_else(|error| panic!("list: {error}"));
    assert_eq!(items, vec!["x".to_owned(), "y".to_owned()]);
}

#[test]
fn refuses_entries_outside_sections() {
    let error = parse("name = \"one\"\n").err();
    assert_eq!(error, Some("line 1 is outside a section".to_owned()));
}

#[test]
fn refuses_duplicate_keys_and_sections() {
    assert!(parse("[a]\nk = \"1\"\nk = \"2\"\n").is_err());
    assert!(parse("[a]\nk = \"1\"\n[a]\nj = \"2\"\n").is_err());
}

#[test]
fn refuses_lines_that_are_not_declarations() {
    assert!(parse("[a]\nnot a declaration\n").is_err());
}

#[test]
fn unquote_round_trips_and_refuses_bare_values() {
    assert_eq!(unquote(&quote("value")), Ok("value".to_owned()));
    assert!(unquote("bare").is_err());
    assert!(unquote("\"inner\"quote\"").is_err());
}

#[test]
fn required_names_the_missing_declaration() {
    let document = parse("[a]\nk = \"1\"\n").unwrap_or_else(|error| panic!("parse: {error}"));
    assert_eq!(
        document.required("a", "missing").err(),
        Some("missing declaration a.missing".to_owned())
    );
}
