//! The `--pod-label key=value` selector and the set of Pod UIDs that match it.
//! A background task refreshes the set from the Kubernetes API; the CRI source
//! reads it to decide which Pod log files to collect.

use std::collections::HashSet;
use std::sync::{Arc, PoisonError, RwLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PodLabelSelector {
    pub key: String,
    pub value: String,
}

impl PodLabelSelector {
    /// Parses `key=value`: exactly one `=`, a nonempty key and value, and no whitespace.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut parts = text.split('=');
        let (Some(key), Some(value), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(format!("pod label `{text}` must be key=value"));
        };
        if key.is_empty() || value.is_empty() || text.chars().any(char::is_whitespace) {
            return Err(format!("pod label `{text}` must be key=value"));
        }
        Ok(Self {
            key: key.to_string(),
            value: value.to_string(),
        })
    }
}

/// The UIDs of the Pods that carry the selected label. Empty until the first
/// successful list, so nothing is collected before the set is known.
#[derive(Clone, Default)]
pub struct PodLabelSet(Arc<RwLock<HashSet<String>>>);

impl PodLabelSet {
    pub fn replace(&self, uids: impl IntoIterator<Item = String>) {
        let next: HashSet<String> = uids.into_iter().collect();
        let mut guard = self.0.write().unwrap_or_else(PoisonError::into_inner);
        *guard = next;
    }

    pub fn contains(&self, pod_uid: &str) -> bool {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(pod_uid)
    }
}

impl std::fmt::Debug for PodLabelSet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.0.read().unwrap_or_else(PoisonError::into_inner).len();
        write!(formatter, "PodLabelSet({count} pods)")
    }
}

impl PartialEq for PodLabelSet {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for PodLabelSet {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_parses_key_value() {
        let selector = PodLabelSelector::parse("sift.axiom.dev/logs=true").unwrap();
        assert_eq!(selector.key, "sift.axiom.dev/logs");
        assert_eq!(selector.value, "true");
    }

    #[test]
    fn selector_rejects_bad_text() {
        for text in ["", "a", "=b", "a=", "a=b=c", "a b=c"] {
            assert!(PodLabelSelector::parse(text).is_err(), "{text:?} must fail");
        }
    }

    #[test]
    fn set_replace_and_contains() {
        let set = PodLabelSet::default();
        assert!(!set.contains("u1"));
        set.replace(["u1".to_string()]);
        assert!(set.contains("u1"));
        assert!(!set.contains("u2"));
        set.replace(["u2".to_string()]);
        assert!(!set.contains("u1"));
        assert!(set.contains("u2"));
    }
}
