use std::collections::BTreeSet;

use rootcause::{Result, prelude::*};

use crate::shared::view_models::STATE_LABEL_KEY;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NormalizedLabel {
    pub(crate) key: String,
    pub(crate) value: Option<String>,
}

pub(crate) fn normalize_key(value: impl Into<String>) -> Result<String> {
    let value = value.into().trim().to_owned();
    if value.is_empty() {
        bail!("label key cannot be empty");
    }
    if value.contains('=') {
        bail!("label key cannot contain '='");
    }
    Ok(value)
}

pub(crate) fn normalize_value(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

pub(crate) fn validate_pair(key: &str, value: Option<&str>) -> Result<()> {
    if key == STATE_LABEL_KEY && value.is_none() {
        bail!("state label requires a value");
    }
    Ok(())
}

pub(crate) fn normalize_initial_labels<I>(labels: I) -> Result<Vec<NormalizedLabel>>
where
    I: IntoIterator<Item = (String, Option<String>)>,
{
    let mut normalized = Vec::new();
    let mut keys = BTreeSet::new();
    for (key, value) in labels {
        let key = normalize_key(key)?;
        let value = normalize_value(value);
        validate_pair(&key, value.as_deref())?;
        if key == STATE_LABEL_KEY {
            bail!("initial labels cannot include 'state'; use the state selector");
        }
        if !keys.insert(key.clone()) {
            bail!("duplicate initial label key '{key}'");
        }
        normalized.push(NormalizedLabel { key, value });
    }
    Ok(normalized)
}

pub(crate) fn format_label(key: &str, value: Option<&str>) -> String {
    match value {
        Some(value) => format!("{key}={value}"),
        None => key.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_rejects_empty_or_composite_keys() {
        assert_eq!(normalize_key(" priority ").unwrap(), "priority");
        assert!(normalize_key("severity=high").is_err());
        assert!(validate_pair(STATE_LABEL_KEY, None).is_err());
    }

    #[test]
    fn initial_labels_are_normalized_and_deduplicated() {
        let labels = normalize_initial_labels([
            (" priority ".to_owned(), Some(" high ".to_owned())),
            ("needs-verification".to_owned(), Some("  ".to_owned())),
        ])
        .unwrap();

        assert_eq!(
            labels,
            vec![
                NormalizedLabel {
                    key: "priority".to_owned(),
                    value: Some("high".to_owned()),
                },
                NormalizedLabel {
                    key: "needs-verification".to_owned(),
                    value: None,
                },
            ]
        );

        let err = normalize_initial_labels([
            ("area".to_owned(), Some("frontend".to_owned())),
            (" area ".to_owned(), Some("backend".to_owned())),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("duplicate initial label key"));

        let err = normalize_initial_labels([(STATE_LABEL_KEY.to_owned(), Some("open".to_owned()))])
            .unwrap_err();
        assert!(err.to_string().contains("use the state selector"));
    }
}
