//! The role a Sift process runs as, which decides the data layout it opens.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageRole {
    All,
    Agent,
    Gateway,
    Query,
    Store,
    Control,
    Operator,
}

impl StorageRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Agent => "agent",
            Self::Gateway => "gateway",
            Self::Query => "query",
            Self::Store => "store",
            Self::Control => "control",
            Self::Operator => "operator",
        }
    }
}
