//! Database model: typed properties, saved views, and value validation.
//!
//! A database is a page whose child pages are its records. The schema describes the properties
//! every record may have; values live next to the records. Filtering and sorting are evaluated on
//! the client (the server will never see plaintext), so this module only defines and validates the
//! data, it does not run queries.

use crate::ids::PageId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = 1;
/// Pseudo-property id referring to a record's title in sorts and filters.
pub const TITLE_PROPERTY: &str = "title";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PropertyKind {
    Text,
    Number,
    Checkbox,
    Select,
    MultiSelect,
    Date,
    Url,
    Relation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: PropertyKind,
    /// Choices for `select` / `multiSelect`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Target database of a `relation` property.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation_database: Option<PageId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    pub property: String,
    pub direction: SortDirection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterOp {
    Contains,
    NotContains,
    Equals,
    NotEquals,
    Gt,
    Gte,
    Lt,
    Lte,
    IsEmpty,
    IsNotEmpty,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub property: String,
    pub op: FilterOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterMode {
    #[default]
    And,
    Or,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub sorts: Vec<Sort>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub filter_mode: FilterMode,
    /// Property ids hidden in this view.
    #[serde(default)]
    pub hidden: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseSchema {
    pub version: u32,
    pub properties: Vec<Property>,
    pub views: Vec<View>,
}

impl Default for DatabaseSchema {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            properties: Vec::new(),
            views: vec![View {
                id: "default".into(),
                name: "Table".into(),
                sorts: Vec::new(),
                filters: Vec::new(),
                filter_mode: FilterMode::And,
                hidden: Vec::new(),
            }],
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SchemaError {
    #[error("schema version {found} is newer than supported version {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("property names must not be empty")]
    EmptyName,
    #[error("duplicate id “{0}”")]
    DuplicateId(String),
    #[error("a database needs at least one view")]
    NoViews,
    #[error("“{0}” refers to a property that does not exist")]
    UnknownProperty(String),
    #[error("property “{0}” is a relation without a target database")]
    RelationWithoutTarget(String),
    #[error("{0}")]
    InvalidValue(String),
}

impl DatabaseSchema {
    pub fn property(&self, id: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.id == id)
    }

    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.version > SCHEMA_VERSION {
            return Err(SchemaError::UnsupportedVersion {
                found: self.version,
                supported: SCHEMA_VERSION,
            });
        }
        let mut ids = HashSet::new();
        for p in &self.properties {
            if p.name.trim().is_empty() {
                return Err(SchemaError::EmptyName);
            }
            if p.id == TITLE_PROPERTY || !ids.insert(p.id.as_str()) {
                return Err(SchemaError::DuplicateId(p.id.clone()));
            }
            if p.kind == PropertyKind::Relation && p.relation_database.is_none() {
                return Err(SchemaError::RelationWithoutTarget(p.name.clone()));
            }
            let mut opts = HashSet::new();
            for o in &p.options {
                if !opts.insert(o.id.as_str()) {
                    return Err(SchemaError::DuplicateId(o.id.clone()));
                }
            }
        }
        if self.views.is_empty() {
            return Err(SchemaError::NoViews);
        }
        let known = |id: &str| id == TITLE_PROPERTY || ids.contains(id);
        let mut view_ids = HashSet::new();
        for v in &self.views {
            if !view_ids.insert(v.id.as_str()) {
                return Err(SchemaError::DuplicateId(v.id.clone()));
            }
            let refs = v
                .sorts
                .iter()
                .map(|s| &s.property)
                .chain(v.filters.iter().map(|f| &f.property))
                .chain(v.hidden.iter());
            for r in refs {
                if !known(r) {
                    return Err(SchemaError::UnknownProperty(r.clone()));
                }
            }
        }
        Ok(())
    }
}

/// Checks that `value` is acceptable for `prop`. `null` (an empty cell) is always valid.
pub fn validate_value(prop: &Property, value: &Value) -> Result<(), SchemaError> {
    let bad = |what: &str| {
        Err(SchemaError::InvalidValue(format!(
            "“{}” expects {what}",
            prop.name
        )))
    };
    if value.is_null() {
        return Ok(());
    }
    let option_ok = |v: &Value| {
        v.as_str()
            .is_some_and(|s| prop.options.iter().any(|o| o.id == s))
    };
    match prop.kind {
        PropertyKind::Text => {
            if value.is_string() {
                Ok(())
            } else {
                bad("text")
            }
        }
        PropertyKind::Url => match value.as_str() {
            Some(s) if s.is_empty() || s.contains("://") || s.starts_with("mailto:") => Ok(()),
            _ => bad("a link such as https://…"),
        },
        PropertyKind::Number => {
            if value.as_f64().is_some_and(f64::is_finite) {
                Ok(())
            } else {
                bad("a number")
            }
        }
        PropertyKind::Checkbox => {
            if value.is_boolean() {
                Ok(())
            } else {
                bad("true or false")
            }
        }
        PropertyKind::Select => {
            if option_ok(value) {
                Ok(())
            } else {
                bad("one of its options")
            }
        }
        PropertyKind::MultiSelect => match value.as_array() {
            Some(a) if a.iter().all(option_ok) => Ok(()),
            _ => bad("a list of its options"),
        },
        PropertyKind::Date => match value.as_str() {
            Some(s) if is_iso_date(s) => Ok(()),
            _ => bad("a date (YYYY-MM-DD)"),
        },
        PropertyKind::Relation => match value.as_array() {
            Some(a)
                if a.iter()
                    .all(|v| v.as_str().is_some_and(|s| s.parse::<PageId>().is_ok())) =>
            {
                Ok(())
            }
            _ => bad("a list of page ids"),
        },
    }
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| s[r].parse::<u32>().ok();
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn prop(kind: PropertyKind) -> Property {
        Property {
            id: "p".into(),
            name: "P".into(),
            kind,
            options: vec![SelectOption {
                id: "a".into(),
                name: "A".into(),
                color: None,
            }],
            relation_database: None,
        }
    }

    #[test]
    fn values_are_checked_against_their_property_type() {
        let ok = |k, v: Value| validate_value(&prop(k), &v).is_ok();
        assert!(ok(PropertyKind::Text, json!("hi")) && !ok(PropertyKind::Text, json!(1)));
        assert!(ok(PropertyKind::Number, json!(1.5)) && !ok(PropertyKind::Number, json!("1")));
        assert!(ok(PropertyKind::Checkbox, json!(true)) && !ok(PropertyKind::Checkbox, json!("x")));
        assert!(ok(PropertyKind::Select, json!("a")) && !ok(PropertyKind::Select, json!("zzz")));
        assert!(
            ok(PropertyKind::MultiSelect, json!(["a"]))
                && !ok(PropertyKind::MultiSelect, json!(["a", "b"]))
        );
        assert!(
            ok(PropertyKind::Date, json!("2024-02-29"))
                && !ok(PropertyKind::Date, json!("2023-02-29"))
        );
        assert!(
            !ok(PropertyKind::Date, json!("2024-13-01"))
                && !ok(PropertyKind::Date, json!("tomorrow"))
        );
        assert!(
            ok(PropertyKind::Url, json!("https://x.y")) && !ok(PropertyKind::Url, json!("nope"))
        );
        let id = PageId::new().to_string();
        assert!(
            ok(PropertyKind::Relation, json!([id])) && !ok(PropertyKind::Relation, json!(["x"]))
        );
        for k in [
            PropertyKind::Text,
            PropertyKind::Number,
            PropertyKind::Date,
            PropertyKind::Select,
        ] {
            assert!(ok(k, Value::Null), "empty cells are always valid");
        }
    }

    #[test]
    fn schema_validation_catches_structural_problems() {
        let mut s = DatabaseSchema::default();
        assert!(s.validate().is_ok());

        s.properties.push(prop(PropertyKind::Text));
        s.properties.push(prop(PropertyKind::Number));
        assert_eq!(s.validate(), Err(SchemaError::DuplicateId("p".into())));
        s.properties.pop();

        s.views[0].sorts.push(Sort {
            property: "ghost".into(),
            direction: SortDirection::Asc,
        });
        assert_eq!(
            s.validate(),
            Err(SchemaError::UnknownProperty("ghost".into()))
        );
        s.views[0].sorts[0].property = TITLE_PROPERTY.into();
        assert!(s.validate().is_ok());

        s.properties[0].kind = PropertyKind::Relation;
        assert!(matches!(
            s.validate(),
            Err(SchemaError::RelationWithoutTarget(_))
        ));
        s.properties[0].kind = PropertyKind::Text;

        s.views.clear();
        assert_eq!(s.validate(), Err(SchemaError::NoViews));
        s.version = 99;
        assert!(matches!(
            s.validate(),
            Err(SchemaError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn schema_json_round_trip_uses_camel_case() {
        let mut s = DatabaseSchema::default();
        s.properties.push(prop(PropertyKind::MultiSelect));
        s.views[0].filter_mode = FilterMode::Or;
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            json.contains("\"filterMode\":\"or\"") && json.contains("\"type\":\"multiSelect\"")
        );
        assert_eq!(serde_json::from_str::<DatabaseSchema>(&json).unwrap(), s);
    }
}
