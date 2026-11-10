//! Parse the declared PromQL subset, then discard the third-party AST.
use crate::query::domain::metric_expression::{
    Expr, ExpressionParser, LabelSelection, MatchOp, Matcher, Selector,
};
use promql_parser::{
    label::MatchOp as WireMatchOp,
    parser::{self, Expr as WireExpr, LabelModifier, VectorMatchCardinality, VectorSelector},
};

pub(crate) struct PromqlParser;

fn labels(modifier: &LabelModifier) -> LabelSelection {
    LabelSelection {
        include: modifier.is_include(),
        names: modifier.labels().labels.clone(),
    }
}

fn selector(value: &VectorSelector, window: Option<i64>) -> Result<Selector, String> {
    if value.offset.is_some() || value.at.is_some() || !value.matchers.or_matchers.is_empty() {
        return Err("offset, @ modifiers, and OR selectors are not supported".into());
    }
    Ok(Selector {
        name: value.name.clone(),
        window_nanos: window,
        matchers: value
            .matchers
            .matchers
            .iter()
            .map(|m| Matcher {
                name: m.name.clone(),
                value: m.value.clone(),
                op: match &m.op {
                    WireMatchOp::Equal => MatchOp::Equal,
                    WireMatchOp::NotEqual => MatchOp::NotEqual,
                    WireMatchOp::Re(regex) => MatchOp::Regex(regex.clone()),
                    WireMatchOp::NotRe(regex) => MatchOp::NotRegex(regex.clone()),
                },
            })
            .collect(),
    })
}

fn convert(value: &WireExpr, depth: usize) -> Result<Expr, String> {
    if depth > 32 {
        return Err("expression nesting exceeds 32 levels".into());
    }
    let child = |input: &WireExpr| convert(input, depth + 1).map(Box::new);
    match value {
        WireExpr::NumberLiteral(value) => Ok(Expr::Scalar(value.val)),
        WireExpr::Paren(value) => convert(&value.expr, depth + 1),
        WireExpr::Unary(value) => Ok(Expr::Negate(child(&value.expr)?)),
        WireExpr::VectorSelector(value) => Ok(Expr::Select(selector(value, None)?)),
        WireExpr::Aggregate(value) => {
            let op = value.op.to_string();
            if !["sum", "avg", "min", "max", "count"].contains(&op.as_str())
                || value.param.is_some()
            {
                return Err(format!("aggregation `{op}` is not supported"));
            }
            Ok(Expr::Aggregate {
                op,
                labels: value.modifier.as_ref().map(labels),
                input: child(&value.expr)?,
            })
        }
        WireExpr::Binary(value) => {
            let op = value.op.to_string();
            if !["+", "-", "*", "/", "==", "!=", ">", "<", ">=", "<="].contains(&op.as_str()) {
                return Err(format!("binary operator `{op}` is not supported"));
            }
            if value.modifier.as_ref().is_some_and(|m| {
                m.card != VectorMatchCardinality::OneToOne
                    || m.fill_values.lhs.is_some()
                    || m.fill_values.rhs.is_some()
            }) {
                return Err("only one-to-one vector matching is supported".into());
            }
            Ok(Expr::Binary {
                op,
                left: child(&value.lhs)?,
                right: child(&value.rhs)?,
                matching: value
                    .modifier
                    .as_ref()
                    .and_then(|m| m.matching.as_ref())
                    .map(labels),
                return_bool: value.modifier.as_ref().is_some_and(|m| m.return_bool),
            })
        }
        WireExpr::Call(call) => match call.func.name {
            "rate" | "increase" if call.args.args.len() == 1 => {
                let WireExpr::MatrixSelector(value) = call.args.args[0].as_ref() else {
                    return Err("rate and increase require a range selector".into());
                };
                let window =
                    i64::try_from(value.range.as_nanos()).map_err(|_| "range duration overflow")?;
                if window == 0 {
                    return Err("range duration must be positive".into());
                }
                Ok(Expr::RangeFunction {
                    increase: call.func.name == "increase",
                    input: selector(&value.vs, Some(window))?,
                })
            }
            "histogram_quantile" if call.args.args.len() == 2 => Ok(Expr::Quantile {
                quantile: child(&call.args.args[0])?,
                buckets: child(&call.args.args[1])?,
            }),
            name => Err(format!("function `{name}` is not supported")),
        },
        _ => Err("subqueries, strings, and bare range results are not supported".into()),
    }
}

impl ExpressionParser for PromqlParser {
    fn parse(&self, expression: &str) -> Result<Expr, String> {
        if expression.len() > 16 * 1024 {
            return Err("metric expression exceeds 16 KiB".into());
        }
        let (mut depth, mut quote, mut escaped) = (0u32, None, false);
        let mut operators = 0usize;
        for byte in expression.bytes() {
            if escaped {
                escaped = false;
                continue;
            }
            if quote.is_some() && byte == b'\\' {
                escaped = true;
                continue;
            }
            if let Some(delimiter) = quote {
                if byte == delimiter {
                    quote = None;
                }
                continue;
            }
            match byte {
                b'+' | b'-' | b'*' | b'/' | b'=' | b'>' | b'<' | b',' => {
                    operators += 1;
                    if operators > 128 {
                        return Err("expression exceeds its operator budget".into());
                    }
                }
                b'\"' | b'\'' | b'`' => quote = Some(byte),
                b'(' | b'[' | b'{' => {
                    depth += 1;
                    if depth > 32 {
                        return Err("expression nesting exceeds 32 levels".into());
                    }
                }
                b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        convert(&parser::parse(expression)?, 0)
    }
}
