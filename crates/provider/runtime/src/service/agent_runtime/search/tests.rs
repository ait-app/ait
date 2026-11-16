//! Cases from Paseo agent-history-search.test.ts and protocol/search/text-match.test.ts.

use super::*;

#[test]
fn matches_upstream_history_tests_and_generated_distance_oracles() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/paseo-history-search.json"
    ))
    .unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(fixture["source"]["upstreamTestsExecuted"], 9);
    assert_eq!(cases.len(), 12);
    for case in cases {
        let fields = case["fields"].as_array().unwrap();
        let actual = Query::new(case["query"].as_str().unwrap())
            .matches(std::array::from_fn(|index| fields[index].as_str().unwrap()));
        assert_eq!(
            actual,
            case["expected"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
    }
    for case in fixture["distances"].as_array().unwrap() {
        let query = case["query"]
            .as_str()
            .unwrap()
            .encode_utf16()
            .collect::<Vec<_>>();
        let actual = within_edit_budget(
            &query,
            case["word"].as_str().unwrap().as_bytes(),
            usize::try_from(case["budget"].as_u64().unwrap()).unwrap(),
        );
        assert_eq!(actual, case["expected"].as_bool().unwrap(), "{case}");
    }
}

#[test]
fn history_search_matches_all_recalled_names_and_every_query_token() {
    let fields = [
        "Add Stripe billing",
        "Reshape entitlements",
        "add-stripe-billing",
        "getpaseo/paseo",
    ];
    for query in [
        "stripe",
        "entitlements",
        "billing",
        "paseo",
        "stripe entitlements",
        "bulling",
        "   ",
    ] {
        assert!(Query::new(query).matches(fields), "{query}");
    }
    assert!(!Query::new("stripe rosetta").matches(fields));
    assert!(Query::new("stripe main").matches(["Add Stripe billing", "", "main", ""]));
}

#[test]
fn subsequences_stay_within_words_and_short_typos_only_allow_transposition() {
    assert!(!Query::new("terminal").matches([
        "Let me diagnose this problem in a diagnose this problem and",
        "",
        "",
        ""
    ]));
    assert!(Query::new("trmnl").matches(["Fix terminal resizing", "", "", ""]));
    assert!(Query::new("mian").matches(["", "", "main", ""]));
    assert!(!Query::new("rain").matches(["", "", "main", ""]));
    assert!(!Query::new("gat").matches(["", "", "git", ""]));
}

#[test]
fn longer_tokens_accept_bounded_edits_in_word_prefixes() {
    for (query, candidate, matched) in [
        ("confug", "configuration", true),
        ("bulling", "billing", true),
        ("termnial", "terminal", true),
        ("termxxal", "terminal", true),
        ("texxxxal", "terminal", false),
        ("upstram", "upstream", true),
        ("工作区", "修复工作区", true),
        ("labeldesign", "Label as Design", false),
        ("pasbab", "paseo-babysit", true),
    ] {
        assert_eq!(
            Query::new(query).matches([candidate, "", "", ""]),
            matched,
            "{query}: {candidate}"
        );
    }
}

#[test]
fn bounded_edit_distance_handles_insert_delete_replace_and_swaps() {
    for (query, word, budget, matched) in [
        ("billing", "bulling", 1, true),
        ("billing", "biling", 1, true),
        ("biling", "billing", 1, true),
        ("terminal", "termnial", 1, true),
        ("abcde", "axcye", 1, false),
        ("abcde", "axcye", 2, true),
        ("abcde", "axxxe", 2, false),
        ("abcde", "abc", 1, false),
    ] {
        assert_eq!(
            within_edit_budget(
                &query.encode_utf16().collect::<Vec<_>>(),
                word.as_bytes(),
                budget
            ),
            matched,
            "{query}: {word}"
        );
    }
}
