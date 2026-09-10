use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpaqueKind {
    Base64Blob,
    LongString,
    HtmlChunk,
    #[allow(dead_code)]
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    pub name: String,
    pub type_tag: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    pub fields: Vec<FieldSpec>,
}

impl Schema {
    #[allow(dead_code)]
    pub fn field_names(&self) -> Vec<&str> {
        self.fields.iter().map(|f| f.name.as_str()).collect()
    }
}

#[derive(Debug, Clone)]
pub enum CellValue {
    Scalar(Value),
    Nested(Box<Compaction>),
    OpaqueRef {
        ccr_hash: String,
        byte_size: usize,
        kind: OpaqueKind,
    },
    Missing,
}

#[derive(Debug, Clone)]
pub struct Row(pub Vec<CellValue>);

impl Row {
    pub fn new(cells: Vec<CellValue>) -> Self {
        Self(cells)
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct Bucket {
    pub key: Value,
    pub schema: Schema,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone)]
pub enum Compaction {
    Table {
        schema: Schema,
        rows: Vec<Row>,
        original_count: usize,
    },
    Buckets {
        discriminator: String,
        buckets: Vec<Bucket>,
        original_count: usize,
    },
    #[allow(dead_code)]
    OpaqueRef {
        ccr_hash: String,
        byte_size: usize,
        kind: OpaqueKind,
    },
    Untouched(Value),
}

impl Compaction {
    pub fn was_compacted(&self) -> bool {
        matches!(
            self,
            Compaction::Table { .. } | Compaction::Buckets { .. } | Compaction::OpaqueRef { .. }
        )
    }

    #[allow(dead_code)]
    pub fn kept_row_count(&self) -> usize {
        match self {
            Compaction::Table { rows, .. } => rows.len(),
            Compaction::Buckets { buckets, .. } => buckets.iter().map(|b| b.rows.len()).sum(),
            Compaction::OpaqueRef { .. } | Compaction::Untouched(_) => 0,
        }
    }

    #[allow(dead_code)]
    pub fn original_row_count(&self) -> usize {
        match self {
            Compaction::Table { original_count, .. } => *original_count,
            Compaction::Buckets { original_count, .. } => *original_count,
            Compaction::OpaqueRef { .. } | Compaction::Untouched(_) => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_field_names_returns_in_order() {
        let s = Schema {
            fields: vec![
                FieldSpec {
                    name: "id".into(),
                    type_tag: "int".into(),
                    nullable: false,
                },
                FieldSpec {
                    name: "name".into(),
                    type_tag: "string".into(),
                    nullable: false,
                },
            ],
        };
        assert_eq!(s.field_names(), vec!["id", "name"]);
    }

    #[test]
    fn untouched_is_not_compacted() {
        let c = Compaction::Untouched(json!([1, 2, 3]));
        assert!(!c.was_compacted());
        assert_eq!(c.kept_row_count(), 0);
        assert_eq!(c.original_row_count(), 0);
    }

    #[test]
    fn table_row_counts() {
        let c = Compaction::Table {
            schema: Schema { fields: vec![] },
            rows: vec![Row::new(vec![]), Row::new(vec![])],
            original_count: 5,
        };
        assert!(c.was_compacted());
        assert_eq!(c.kept_row_count(), 2);
        assert_eq!(c.original_row_count(), 5);
    }

    #[test]
    fn buckets_aggregate_row_counts() {
        let c = Compaction::Buckets {
            discriminator: "type".into(),
            buckets: vec![
                Bucket {
                    key: json!("user"),
                    schema: Schema { fields: vec![] },
                    rows: vec![Row::new(vec![]), Row::new(vec![])],
                },
                Bucket {
                    key: json!("order"),
                    schema: Schema { fields: vec![] },
                    rows: vec![Row::new(vec![])],
                },
            ],
            original_count: 10,
        };
        assert_eq!(c.kept_row_count(), 3);
        assert_eq!(c.original_row_count(), 10);
    }

    #[test]
    fn cell_missing_distinct_from_scalar_null() {
        let m = CellValue::Missing;
        let n = CellValue::Scalar(Value::Null);
        assert_ne!(format!("{m:?}"), format!("{n:?}"));
    }
}
