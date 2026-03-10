//! Initial witness families for Phase 1.

use std::path::Path;

use ozzy_core::schema::{SchemaInfo, extract_parquet_schema, get_parquet_row_count};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CsvWitness {
    pub delimiter: String,
    pub header: bool,
    pub columns: Vec<String>,
    pub row_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableColumnWitness {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableWitness {
    pub columns: Vec<TableColumnWitness>,
    pub row_count: Option<u64>,
}

impl From<SchemaInfo> for TableWitness {
    fn from(schema: SchemaInfo) -> Self {
        Self {
            columns: schema
                .fields
                .into_iter()
                .map(|field| TableColumnWitness {
                    name: field.name,
                    data_type: field.dtype,
                    nullable: field.nullable,
                })
                .collect(),
            row_count: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum WitnessError {
    #[error(transparent)]
    Core(#[from] ozzy_core::Error),
}

impl TableWitness {
    pub fn from_parquet_file(path: &Path) -> Result<Self, WitnessError> {
        let schema = extract_parquet_schema(path)?;
        let row_count = get_parquet_row_count(path)?;

        Ok(Self {
            row_count: Some(row_count),
            ..Self::from(schema)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordWitness {
    pub present_fields: Vec<String>,
    pub absent_optional_fields: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzy_core::schema::{FieldInfo, SchemaInfo};

    #[test]
    fn csv_witness_tracks_header_and_columns() {
        let witness = CsvWitness {
            delimiter: ",".to_string(),
            header: true,
            columns: vec!["species".to_string(), "wp".to_string()],
            row_count: Some(2),
        };

        assert!(witness.header);
        assert_eq!(witness.columns.len(), 2);
    }

    #[test]
    fn table_witness_converts_from_schema_info() {
        let schema = SchemaInfo {
            fields: vec![
                FieldInfo {
                    name: "species".to_string(),
                    dtype: "utf8".to_string(),
                    nullable: false,
                },
                FieldInfo {
                    name: "wp".to_string(),
                    dtype: "float64".to_string(),
                    nullable: true,
                },
            ],
        };

        let witness = TableWitness::from(schema);
        assert_eq!(witness.columns.len(), 2);
        assert_eq!(witness.columns[0].name, "species");
        assert_eq!(witness.columns[1].data_type, "float64");
    }
}
