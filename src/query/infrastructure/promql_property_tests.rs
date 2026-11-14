//! Property tests for the PromQL subset parser.

use super::promql_parser::PromqlParser;
use crate::query::domain::metric_expression::{Expr, ExpressionParser};
use proptest::prelude::*;

const PROMQL_KEYWORDS: &[&str] = &[
    "sum", "avg", "min", "max", "count", "by", "without", "on", "ignoring", "bool", "and", "or",
    "unless", "offset", "inf", "nan",
];

fn not_keyword(word: &str) -> bool {
    !PROMQL_KEYWORDS.contains(&word)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any string input gives Ok or Err from PromqlParser, never a panic.
    #[test]
    fn promql_parser_never_panics(
        input in prop_oneof![any::<String>(), "\\PC{0,200}"],
    ) {
        let _ = PromqlParser.parse(&input);
    }

    /// An expression longer than 16 KiB is always rejected.
    #[test]
    fn promql_parser_rejects_over_16_kib(extra in 1usize..64) {
        let input = "a".repeat(16 * 1024 + extra);
        prop_assert!(PromqlParser.parse(&input).is_err());
    }

    /// A plain metric name with equality matchers parses to a selector with the same name and matcher count.
    #[test]
    fn promql_parser_accepts_generated_selectors(
        name in "[a-z][a-z0-9_]{0,15}".prop_filter("PromQL keyword", |word| not_keyword(word)),
        matchers in prop::collection::btree_map(
            "[a-z][a-z0-9_]{0,7}".prop_filter("PromQL keyword", |word| not_keyword(word)),
            "[a-z0-9]{0,8}",
            0..3,
        ),
    ) {
        let count = matchers.len();
        let expression = if matchers.is_empty() {
            name.clone()
        } else {
            let body = matchers
                .iter()
                .map(|(label, value)| format!("{label}=\"{value}\""))
                .collect::<Vec<_>>()
                .join(",");
            format!("{name}{{{body}}}")
        };
        match PromqlParser.parse(&expression) {
            Ok(Expr::Select(selector)) => {
                prop_assert_eq!(selector.name, Some(name));
                prop_assert_eq!(selector.matchers.len(), count);
            }
            other => prop_assert!(false, "expected a selector for {expression:?}, got {other:?}"),
        }
    }
}
