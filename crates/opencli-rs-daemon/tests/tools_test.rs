use opencli_rs_daemon::tools::{find_by_name, load_tools, search, summary, Tool, ToolFrontmatter};
use std::collections::HashMap;

fn create_dummy_tools() -> Vec<Tool> {
    let mut install = HashMap::new();
    install.insert("mac".to_string(), "brew install rg".to_string());

    vec![
        Tool {
            name: "ripgrep".to_string(),
            binary: "rg".to_string(),
            description: "Fast line-oriented regex search".to_string(),
            homepage: None,
            tags: vec!["search".to_string(), "grep".to_string()],
            install: install.clone(),
            body: "Fast search tool.".to_string(),
        },
        Tool {
            name: "jq".to_string(),
            binary: "jq".to_string(),
            description: "Command-line JSON processor".to_string(),
            homepage: None,
            tags: vec!["json".to_string(), "parser".to_string()],
            install,
            body: "JSON processing tool.".to_string(),
        },
    ]
}

#[test]
fn test_search() {
    let tools = create_dummy_tools();

    let results = search("regex", &tools);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "ripgrep");

    let results = search("JSON", &tools);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "jq");

    let results = search("parser", &tools);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "jq");

    let results = search("rg", &tools);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "ripgrep");

    let results = search("   ", &tools);
    assert_eq!(results.len(), 2);
}

#[test]
fn test_find_by_name() {
    let tools = create_dummy_tools();

    let result = find_by_name("ripgrep", &tools);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name, "ripgrep");

    let result = find_by_name("jq", &tools);
    assert!(result.is_some());

    let result = find_by_name("unknown", &tools);
    assert!(result.is_none());
}

#[test]
fn test_summary() {
    let tools = create_dummy_tools();
    let summaries = summary(&tools);

    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].name, "ripgrep");
    assert_eq!(summaries[0].description, "Fast line-oriented regex search");
}
