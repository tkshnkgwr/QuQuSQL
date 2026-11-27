use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectType {
    Table,
    View,
    Synonym,
    Index,
    Sequence,
    Trigger,
    Other(String),
}

impl ObjectType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Table => "TABLE",
            Self::View => "VIEW",
            Self::Synonym => "SYNONYM",
            Self::Index => "INDEX",
            Self::Sequence => "SEQUENCE",
            Self::Trigger => "TRIGGER",
            Self::Other(s) => s.as_str(),
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "TABLE" => Self::Table,
            "VIEW" => Self::View,
            "SYNONYM" => Self::Synonym,
            "INDEX" => Self::Index,
            "SEQUENCE" => Self::Sequence,
            "TRIGGER" => Self::Trigger,
            other => Self::Other(other.to_string()),
        }
    }

    pub fn standard_types() -> &'static [ObjectType] {
        &[
            ObjectType::Table,
            ObjectType::View,
            ObjectType::Synonym,
            ObjectType::Index,
            ObjectType::Sequence,
            ObjectType::Trigger,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaObject {
    pub owner: String,
    pub name: String,
    pub object_type: ObjectType,
    pub comments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDetail {
    pub name: String,
    pub data_type: String,
    pub length: String,
    pub nullable: bool,
    pub is_pk: bool,
    pub comments: String,
}
