//! Schema extraction and validation for Arrow/Parquet data.

use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use crate::error::{Error, Result};

/// Simplified schema representation for storage and comparison.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaInfo {
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldInfo {
    pub name: String,
    pub dtype: String,
    pub nullable: bool,
}

impl From<&Schema> for SchemaInfo {
    fn from(schema: &Schema) -> Self {
        Self {
            fields: schema
                .fields()
                .iter()
                .map(|f| FieldInfo {
                    name: f.name().clone(),
                    dtype: format_data_type(f.data_type()),
                    nullable: f.is_nullable(),
                })
                .collect(),
        }
    }
}

impl SchemaInfo {
    /// Convert back to Arrow schema.
    pub fn to_arrow_schema(&self) -> Schema {
        let fields: Vec<Field> = self
            .fields
            .iter()
            .map(|f| Field::new(&f.name, parse_data_type(&f.dtype), f.nullable))
            .collect();
        Schema::new(fields)
    }

    /// Check if this schema contains all required columns.
    pub fn contains_columns(&self, required: &[&str]) -> bool {
        required
            .iter()
            .all(|col| self.fields.iter().any(|f| f.name == *col))
    }

    /// Get column names.
    pub fn column_names(&self) -> Vec<&str> {
        self.fields.iter().map(|f| f.name.as_str()).collect()
    }
}

/// Format Arrow DataType as a string for storage.
fn format_data_type(dt: &DataType) -> String {
    match dt {
        DataType::Null => "null".to_string(),
        DataType::Boolean => "bool".to_string(),
        DataType::Int8 => "int8".to_string(),
        DataType::Int16 => "int16".to_string(),
        DataType::Int32 => "int32".to_string(),
        DataType::Int64 => "int64".to_string(),
        DataType::UInt8 => "uint8".to_string(),
        DataType::UInt16 => "uint16".to_string(),
        DataType::UInt32 => "uint32".to_string(),
        DataType::UInt64 => "uint64".to_string(),
        DataType::Float16 => "float16".to_string(),
        DataType::Float32 => "float32".to_string(),
        DataType::Float64 => "float64".to_string(),
        DataType::Utf8 => "utf8".to_string(),
        DataType::LargeUtf8 => "large_utf8".to_string(),
        DataType::Binary => "binary".to_string(),
        DataType::LargeBinary => "large_binary".to_string(),
        DataType::Date32 => "date32".to_string(),
        DataType::Date64 => "date64".to_string(),
        DataType::Timestamp(unit, tz) => {
            let unit_str = match unit {
                TimeUnit::Second => "s",
                TimeUnit::Millisecond => "ms",
                TimeUnit::Microsecond => "us",
                TimeUnit::Nanosecond => "ns",
            };
            match tz {
                Some(tz) => format!("timestamp[{}, {}]", unit_str, tz),
                None => format!("timestamp[{}]", unit_str),
            }
        }
        DataType::Time32(unit) => {
            let unit_str = match unit {
                TimeUnit::Second => "s",
                TimeUnit::Millisecond => "ms",
                _ => "?",
            };
            format!("time32[{}]", unit_str)
        }
        DataType::Time64(unit) => {
            let unit_str = match unit {
                TimeUnit::Microsecond => "us",
                TimeUnit::Nanosecond => "ns",
                _ => "?",
            };
            format!("time64[{}]", unit_str)
        }
        DataType::Duration(unit) => {
            let unit_str = match unit {
                TimeUnit::Second => "s",
                TimeUnit::Millisecond => "ms",
                TimeUnit::Microsecond => "us",
                TimeUnit::Nanosecond => "ns",
            };
            format!("duration[{}]", unit_str)
        }
        DataType::List(inner) => format!("list<{}>", format_data_type(inner.data_type())),
        DataType::LargeList(inner) => {
            format!("large_list<{}>", format_data_type(inner.data_type()))
        }
        DataType::FixedSizeList(inner, size) => {
            format!("fixed_list<{}, {}>", format_data_type(inner.data_type()), size)
        }
        DataType::Struct(fields) => {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|f| format!("{}: {}", f.name(), format_data_type(f.data_type())))
                .collect();
            format!("struct<{}>", field_strs.join(", "))
        }
        DataType::Dictionary(key, value) => {
            format!(
                "dict<{}, {}>",
                format_data_type(key),
                format_data_type(value)
            )
        }
        _ => format!("{:?}", dt),
    }
}

/// Parse a string data type back to Arrow DataType.
fn parse_data_type(s: &str) -> DataType {
    match s {
        "null" => DataType::Null,
        "bool" => DataType::Boolean,
        "int8" => DataType::Int8,
        "int16" => DataType::Int16,
        "int32" => DataType::Int32,
        "int64" => DataType::Int64,
        "uint8" => DataType::UInt8,
        "uint16" => DataType::UInt16,
        "uint32" => DataType::UInt32,
        "uint64" => DataType::UInt64,
        "float16" => DataType::Float16,
        "float32" => DataType::Float32,
        "float64" => DataType::Float64,
        "utf8" => DataType::Utf8,
        "large_utf8" => DataType::LargeUtf8,
        "binary" => DataType::Binary,
        "large_binary" => DataType::LargeBinary,
        "date32" => DataType::Date32,
        "date64" => DataType::Date64,
        s if s.starts_with("timestamp[") => {
            // Parse timestamp[unit, tz] or timestamp[unit]
            let inner = &s[10..s.len() - 1];
            let parts: Vec<&str> = inner.split(", ").collect();
            let unit = match parts[0] {
                "s" => TimeUnit::Second,
                "ms" => TimeUnit::Millisecond,
                "us" => TimeUnit::Microsecond,
                "ns" => TimeUnit::Nanosecond,
                _ => TimeUnit::Nanosecond,
            };
            let tz = parts.get(1).map(|s| Arc::from(*s));
            DataType::Timestamp(unit, tz)
        }
        _ => DataType::Utf8, // Default fallback
    }
}

/// Extract schema from a Parquet file.
pub fn extract_parquet_schema(path: &Path) -> Result<SchemaInfo> {
    let file = File::open(path).map_err(|e| Error::FileNotFound(format!("{}: {}", path.display(), e)))?;

    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| Error::InvalidParquet(format!("{}: {}", path.display(), e)))?;

    let schema = reader.schema();
    Ok(SchemaInfo::from(schema.as_ref()))
}

/// Get row count from a Parquet file.
pub fn get_parquet_row_count(path: &Path) -> Result<u64> {
    let file = File::open(path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let metadata = reader.metadata();

    let count: i64 = metadata
        .row_groups()
        .iter()
        .map(|rg| rg.num_rows())
        .sum();

    Ok(count as u64)
}

/// Validate that a transform's output schema is compatible with the next transform's input.
pub fn validate_schema_compatibility(
    output_schema: &SchemaInfo,
    required_columns: &[&str],
) -> Result<()> {
    for col in required_columns {
        if !output_schema.fields.iter().any(|f| f.name == *col) {
            return Err(Error::SchemaMismatch(format!(
                "Required column '{}' not found in schema. Available columns: {:?}",
                col,
                output_schema.column_names()
            )));
        }
    }
    Ok(())
}

/// Print schema in a human-readable format.
pub fn format_schema(schema: &SchemaInfo) -> String {
    let mut output = String::new();
    output.push_str("Schema:\n");
    for field in &schema.fields {
        let nullable = if field.nullable { "?" } else { "" };
        output.push_str(&format!("  {}: {}{}\n", field.name, field.dtype, nullable));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_data_type_roundtrip() {
        let types = vec![
            DataType::Int64,
            DataType::Float64,
            DataType::Utf8,
            DataType::Boolean,
            DataType::Date32,
        ];

        for dt in types {
            let formatted = format_data_type(&dt);
            let parsed = parse_data_type(&formatted);
            assert_eq!(dt, parsed, "Roundtrip failed for {:?}", dt);
        }
    }

    #[test]
    fn test_schema_contains_columns() {
        let schema = SchemaInfo {
            fields: vec![
                FieldInfo {
                    name: "a".to_string(),
                    dtype: "int64".to_string(),
                    nullable: false,
                },
                FieldInfo {
                    name: "b".to_string(),
                    dtype: "float64".to_string(),
                    nullable: true,
                },
                FieldInfo {
                    name: "c".to_string(),
                    dtype: "utf8".to_string(),
                    nullable: false,
                },
            ],
        };

        assert!(schema.contains_columns(&["a", "b"]));
        assert!(schema.contains_columns(&["a"]));
        assert!(!schema.contains_columns(&["a", "d"]));
    }
}
