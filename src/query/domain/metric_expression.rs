//! Sift-owned expression data. The parser and transport types stay outside.
use crate::shared_kernel::event::MetricTemporality;
use std::collections::BTreeMap;

pub(crate) type Labels = BTreeMap<String, String>;

#[derive(Clone, Debug)]
pub(crate) struct Matcher {
    pub name: String,
    pub value: String,
    pub op: MatchOp,
}
#[derive(Clone, Debug)]
pub(crate) enum MatchOp {
    Equal,
    NotEqual,
    Regex(regex::Regex),
    NotRegex(regex::Regex),
}

#[derive(Clone, Debug)]
pub(crate) struct Selector {
    pub name: Option<String>,
    pub matchers: Vec<Matcher>,
    pub window_nanos: Option<i64>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LabelSelection {
    pub include: bool,
    pub names: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Scalar(f64),
    Select(Selector),
    Negate(Box<Expr>),
    Aggregate {
        op: String,
        labels: Option<LabelSelection>,
        input: Box<Expr>,
    },
    RangeFunction {
        increase: bool,
        input: Selector,
    },
    Quantile {
        quantile: Box<Expr>,
        buckets: Box<Expr>,
    },
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
        matching: Option<LabelSelection>,
        return_bool: bool,
    },
}

pub(crate) trait ExpressionParser: Send + Sync {
    fn parse(&self, expression: &str) -> Result<Expr, String>;
}

#[derive(Clone, Debug)]
pub(crate) struct Sample {
    pub time: i64,
    pub start_time: Option<u64>,
    pub value: f64,
    pub stale: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Series {
    pub labels: Labels,
    pub temporality: MetricTemporality,
    pub points: Vec<Sample>,
}

#[derive(Clone, Debug)]
pub(crate) enum Value {
    Scalar(f64),
    Vector(Vec<(Labels, f64)>),
}

impl Expr {
    pub fn selectors(&self, out: &mut Vec<Selector>) {
        match self {
            Self::Select(selector)
            | Self::RangeFunction {
                input: selector, ..
            } => out.push(selector.clone()),
            Self::Negate(input) | Self::Aggregate { input, .. } => input.selectors(out),
            Self::Quantile { quantile, buckets } => {
                quantile.selectors(out);
                buckets.selectors(out);
            }
            Self::Binary { left, right, .. } => {
                left.selectors(out);
                right.selectors(out);
            }
            Self::Scalar(_) => {}
        }
    }
}
