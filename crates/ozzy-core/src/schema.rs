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
            format!(
                "fixed_list<{}, {}>",
                format_data_type(inner.data_type()),
                size
            )
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
            let unit = parse_time_unit(parts[0]);
            let tz = parts.get(1).map(|s| Arc::from(*s));
            DataType::Timestamp(unit, tz)
        }
        s if s.starts_with("time32[") => {
            let inner = &s[7..s.len() - 1];
            DataType::Time32(parse_time_unit(inner))
        }
        s if s.starts_with("time64[") => {
            let inner = &s[7..s.len() - 1];
            DataType::Time64(parse_time_unit(inner))
        }
        s if s.starts_with("duration[") => {
            let inner = &s[9..s.len() - 1];
            DataType::Duration(parse_time_unit(inner))
        }
        s if s.starts_with("list<") => {
            let inner = &s[5..s.len() - 1];
            let inner_type = parse_data_type(inner);
            DataType::List(Arc::new(Field::new("item", inner_type, true)))
        }
        s if s.starts_with("large_list<") => {
            let inner = &s[11..s.len() - 1];
            let inner_type = parse_data_type(inner);
            DataType::LargeList(Arc::new(Field::new("item", inner_type, true)))
        }
        s if s.starts_with("fixed_list<") => {
            // fixed_list<type, size>
            let inner = &s[11..s.len() - 1];
            if let Some(comma_pos) = inner.rfind(", ") {
                let type_str = &inner[..comma_pos];
                let size_str = &inner[comma_pos + 2..];
                if let Ok(size) = size_str.parse::<i32>() {
                    let inner_type = parse_data_type(type_str);
                    return DataType::FixedSizeList(
                        Arc::new(Field::new("item", inner_type, true)),
                        size,
                    );
                }
            }
            eprintln!(
                "Warning: Could not parse fixed_list type '{}', falling back to Utf8",
                s
            );
            DataType::Utf8
        }
        s if s.starts_with("dict<") => {
            // dict<key_type, value_type>
            let inner = &s[5..s.len() - 1];
            if let Some(comma_pos) = inner.find(", ") {
                let key_str = &inner[..comma_pos];
                let value_str = &inner[comma_pos + 2..];
                let key_type = parse_data_type(key_str);
                let value_type = parse_data_type(value_str);
                return DataType::Dictionary(Box::new(key_type), Box::new(value_type));
            }
            eprintln!(
                "Warning: Could not parse dict type '{}', falling back to Utf8",
                s
            );
            DataType::Utf8
        }
        _ => {
            // Log unknown types so users can identify the source of schema mismatches
            eprintln!("Warning: Unknown data type '{}', falling back to Utf8", s);
            DataType::Utf8
        }
    }
}

/// Parse time unit from string.
fn parse_time_unit(s: &str) -> TimeUnit {
    match s {
        "s" => TimeUnit::Second,
        "ms" => TimeUnit::Millisecond,
        "us" => TimeUnit::Microsecond,
        "ns" => TimeUnit::Nanosecond,
        _ => {
            eprintln!(
                "Warning: Unknown time unit '{}', defaulting to Nanosecond",
                s
            );
            TimeUnit::Nanosecond
        }
    }
}

/// Extract schema from a Parquet file.
pub fn extract_parquet_schema(path: &Path) -> Result<SchemaInfo> {
    let file =
        File::open(path).map_err(|e| Error::FileNotFound(format!("{}: {}", path.display(), e)))?;

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

    let count: i64 = metadata.row_groups().iter().map(|rg| rg.num_rows()).sum();

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

// ─────────────────────────────────────────────────────────────────────────────
// Pipeline Validation
// ─────────────────────────────────────────────────────────────────────────────

/// Result of validating a pipeline step.
#[derive(Debug)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationResult {
    pub fn ok() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            valid: false,
            errors: vec![msg.into()],
            warnings: Vec::new(),
        }
    }

    pub fn with_warning(mut self, msg: impl Into<String>) -> Self {
        self.warnings.push(msg.into());
        self
    }

    pub fn merge(mut self, other: ValidationResult) -> Self {
        self.valid = self.valid && other.valid;
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
        self
    }
}

/// Validate that a data source schema is suitable for a transform.
///
/// This checks that all columns required by the transform exist in the input schema.
pub fn validate_transform_input(
    input_schema: &SchemaInfo,
    required_columns: &[&str],
    transform_name: &str,
) -> ValidationResult {
    let mut result = ValidationResult::ok();

    for col in required_columns {
        if !input_schema.fields.iter().any(|f| f.name == *col) {
            result.valid = false;
            result.errors.push(format!(
                "Transform '{}' requires column '{}' which is not in the input schema. Available: {:?}",
                transform_name,
                col,
                input_schema.column_names()
            ));
        }
    }

    result
}

/// Validate an entire pipeline from data source through transforms.
pub fn validate_pipeline(
    data_source_schema: &SchemaInfo,
    transform_requirements: &[(&str, Vec<&str>)], // (transform_name, required_columns)
) -> ValidationResult {
    let mut result = ValidationResult::ok();
    let current_schema = data_source_schema.clone();

    for (i, (transform_name, required_cols)) in transform_requirements.iter().enumerate() {
        let step_result = validate_transform_input(&current_schema, required_cols, transform_name);

        if !step_result.valid {
            result.valid = false;
            for err in step_result.errors {
                result.errors.push(format!("Step {}: {}", i + 1, err));
            }
        }
        result.warnings.extend(step_result.warnings);

        // For now, assume transforms pass through all columns
        // In a full implementation, we'd track schema changes through transforms
    }

    result
}

/// Check if two schemas are compatible (one is a superset of the other).
pub fn schemas_compatible(required: &SchemaInfo, provided: &SchemaInfo) -> bool {
    required.fields.iter().all(|req_field| {
        provided.fields.iter().any(|prov_field| {
            prov_field.name == req_field.name && prov_field.dtype == req_field.dtype
        })
    })
}

/// Get the difference between two schemas.
pub fn schema_diff(from: &SchemaInfo, to: &SchemaInfo) -> SchemaDiff {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    // Find added and changed columns
    for to_field in &to.fields {
        if let Some(from_field) = from.fields.iter().find(|f| f.name == to_field.name) {
            if from_field.dtype != to_field.dtype || from_field.nullable != to_field.nullable {
                changed.push(FieldChange {
                    name: to_field.name.clone(),
                    from_dtype: from_field.dtype.clone(),
                    to_dtype: to_field.dtype.clone(),
                    from_nullable: from_field.nullable,
                    to_nullable: to_field.nullable,
                });
            }
        } else {
            added.push(to_field.clone());
        }
    }

    // Find removed columns
    for from_field in &from.fields {
        if !to.fields.iter().any(|f| f.name == from_field.name) {
            removed.push(from_field.clone());
        }
    }

    SchemaDiff {
        added,
        removed,
        changed,
    }
}

/// Represents the difference between two schemas.
#[derive(Debug)]
pub struct SchemaDiff {
    pub added: Vec<FieldInfo>,
    pub removed: Vec<FieldInfo>,
    pub changed: Vec<FieldChange>,
}

impl SchemaDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    pub fn has_breaking_changes(&self) -> bool {
        !self.removed.is_empty() || !self.changed.is_empty()
    }
}

/// Represents a change to a single field.
#[derive(Debug)]
pub struct FieldChange {
    pub name: String,
    pub from_dtype: String,
    pub to_dtype: String,
    pub from_nullable: bool,
    pub to_nullable: bool,
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
