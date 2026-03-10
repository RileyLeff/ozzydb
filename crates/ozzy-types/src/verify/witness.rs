//! Initial witness families for Phase 1.

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordWitness {
    pub present_fields: Vec<String>,
    pub absent_optional_fields: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
