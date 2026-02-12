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

/// Extract the content between a prefix and its closing delimiter.
///
/// Returns `None` if the string doesn't end with the expected closing char
/// or is too short to contain any inner content.
fn extract_inner<'a>(s: &'a str, prefix_len: usize, close: char) -> Option<&'a str> {
    if s.len() > prefix_len && s.ends_with(close) {
        Some(&s[prefix_len..s.len() - 1])
    } else {
        None
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
            if let Some(inner) = extract_inner(s, 10, ']') {
                let parts: Vec<&str> = inner.split(", ").collect();
                let unit = parse_time_unit(parts[0]);
                let tz = parts.get(1).map(|s| Arc::from(*s));
                DataType::Timestamp(unit, tz)
            } else {
                eprintln!("Warning: malformed type '{}', falling back to Utf8", s);
                DataType::Utf8
            }
        }
        s if s.starts_with("time32[") => {
            if let Some(inner) = extract_inner(s, 7, ']') {
                DataType::Time32(parse_time_unit(inner))
            } else {
                eprintln!("Warning: malformed type '{}', falling back to Utf8", s);
                DataType::Utf8
            }
        }
        s if s.starts_with("time64[") => {
            if let Some(inner) = extract_inner(s, 7, ']') {
                DataType::Time64(parse_time_unit(inner))
            } else {
                eprintln!("Warning: malformed type '{}', falling back to Utf8", s);
                DataType::Utf8
            }
        }
        s if s.starts_with("duration[") => {
            if let Some(inner) = extract_inner(s, 9, ']') {
                DataType::Duration(parse_time_unit(inner))
            } else {
                eprintln!("Warning: malformed type '{}', falling back to Utf8", s);
                DataType::Utf8
            }
        }
        s if s.starts_with("list<") => {
            if let Some(inner) = extract_inner(s, 5, '>') {
                let inner_type = parse_data_type(inner);
                DataType::List(Arc::new(Field::new("item", inner_type, true)))
            } else {
                eprintln!("Warning: malformed type '{}', falling back to Utf8", s);
                DataType::Utf8
            }
        }
        s if s.starts_with("large_list<") => {
            if let Some(inner) = extract_inner(s, 11, '>') {
                let inner_type = parse_data_type(inner);
                DataType::LargeList(Arc::new(Field::new("item", inner_type, true)))
            } else {
                eprintln!("Warning: malformed type '{}', falling back to Utf8", s);
                DataType::Utf8
            }
        }
        s if s.starts_with("fixed_list<") => {
            // fixed_list<type, size>
            let Some(inner) = extract_inner(s, 11, '>') else {
                eprintln!("Warning: malformed type '{}', falling back to Utf8", s);
                return DataType::Utf8;
            };
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
        s if s.starts_with("struct<") => {
            // struct<field1: type1, field2: type2>
            let Some(inner) = extract_inner(s, 7, '>') else {
                eprintln!("Warning: malformed type '{}', falling back to Utf8", s);
                return DataType::Utf8;
            };
            let mut fields = Vec::new();
            let mut depth = 0;
            let mut current = String::new();
            for c in inner.chars() {
                match c {
                    '<' | '[' => {
                        depth += 1;
                        current.push(c);
                    }
                    '>' | ']' => {
                        depth -= 1;
                        current.push(c);
                    }
                    ',' if depth == 0 => {
                        if let Some(colon) = current.find(": ") {
                            let name = current[..colon].trim().to_string();
                            let dtype = parse_data_type(current[colon + 2..].trim());
                            fields.push(Arc::new(Field::new(name, dtype, true)));
                        }
                        current.clear();
                    }
                    _ => current.push(c),
                }
            }
            if !current.trim().is_empty() {
                if let Some(colon) = current.find(": ") {
                    let name = current[..colon].trim().to_string();
                    let dtype = parse_data_type(current[colon + 2..].trim());
                    fields.push(Arc::new(Field::new(name, dtype, true)));
                }
            }
            if fields.is_empty() {
                eprintln!(
                    "Warning: Could not parse struct type '{}', falling back to Utf8",
                    s
                );
                DataType::Utf8
            } else {
                DataType::Struct(fields.into())
            }
        }
        s if s.starts_with("dict<") => {
            // dict<key_type, value_type> -- use depth-aware split for nested types
            let Some(inner) = extract_inner(s, 5, '>') else {
                eprintln!("Warning: malformed type '{}', falling back to Utf8", s);
                return DataType::Utf8;
            };
            let mut depth = 0;
            let mut split_pos = None;
            for (i, c) in inner.char_indices() {
                match c {
                    '<' | '[' => depth += 1,
                    '>' | ']' => depth -= 1,
                    ',' if depth == 0 => {
                        split_pos = Some(i);
                        break;
                    }
                    _ => {}
                }
            }
            if let Some(pos) = split_pos {
                let key_str = inner[..pos].trim();
                let value_str = inner[pos + 1..].trim();
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
        .map_err(|e| Error::SchemaError(format!("{}: {}", path.display(), e)))?;

    let schema = reader.schema();
    Ok(SchemaInfo::from(schema.as_ref()))
}

/// Get row count from a Parquet file.
pub fn get_parquet_row_count(path: &Path) -> Result<u64> {
    let file = File::open(path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let metadata = reader.metadata();

    let count: i64 = metadata.row_groups().iter().map(|rg| rg.num_rows()).sum();

    Ok(count.try_into().unwrap_or(0))
}

/// Validate that a transform's output schema is compatible with the next transform's input.
pub fn validate_schema_compatibility(
    output_schema: &SchemaInfo,
    required_columns: &[&str],
) -> Result<()> {
    for col in required_columns {
        if !output_schema.fields.iter().any(|f| f.name == *col) {
            return Err(Error::SchemaError(format!(
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

        // LIMITATION: We validate each transform against the original data source
        // schema, not against the output of the preceding transform. Accurately
        // tracking schema evolution requires transforms to declare output_schema
        // (adds/removes/renames columns). Until that metadata is reliably present,
        // multi-step pipelines may pass validation even when an intermediate
        // transform drops a column that a later transform requires.
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
    fn test_struct_roundtrip() {
        let struct_type = DataType::Struct(
            vec![
                Arc::new(Field::new("name", DataType::Utf8, true)),
                Arc::new(Field::new("age", DataType::Int32, true)),
            ]
            .into(),
        );
        let formatted = format_data_type(&struct_type);
        assert_eq!(formatted, "struct<name: utf8, age: int32>");
        let parsed = parse_data_type(&formatted);
        assert_eq!(struct_type, parsed, "Struct roundtrip failed");
    }

    #[test]
    fn test_nested_struct_roundtrip() {
        let inner_struct =
            DataType::Struct(vec![Arc::new(Field::new("x", DataType::Float64, true))].into());
        let outer_struct = DataType::Struct(
            vec![
                Arc::new(Field::new("id", DataType::Int64, true)),
                Arc::new(Field::new("inner", inner_struct, true)),
            ]
            .into(),
        );
        let formatted = format_data_type(&outer_struct);
        let parsed = parse_data_type(&formatted);
        assert_eq!(outer_struct, parsed, "Nested struct roundtrip failed");
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

    #[test]
    fn test_dict_roundtrip() {
        let dict_type = DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8));
        let formatted = format_data_type(&dict_type);
        assert_eq!(formatted, "dict<int32, utf8>");
        let parsed = parse_data_type(&formatted);
        assert_eq!(dict_type, parsed, "Dict roundtrip failed");
    }

    #[test]
    fn test_dict_with_nested_struct_roundtrip() {
        let struct_type = DataType::Struct(
            vec![
                Arc::new(Field::new("a", DataType::Int64, true)),
                Arc::new(Field::new("b", DataType::Float64, true)),
            ]
            .into(),
        );
        let dict_type = DataType::Dictionary(Box::new(DataType::Int32), Box::new(struct_type));
        let formatted = format_data_type(&dict_type);
        let parsed = parse_data_type(&formatted);
        assert_eq!(
            dict_type, parsed,
            "Dict with nested struct roundtrip failed"
        );
    }

    #[test]
    fn test_struct_with_bracket_types_roundtrip() {
        // Verifies that timestamp[s, UTC] inside struct<> doesn't split on the inner comma
        let struct_type = DataType::Struct(
            vec![
                Arc::new(Field::new(
                    "ts",
                    DataType::Timestamp(TimeUnit::Second, Some(Arc::from("UTC"))),
                    true,
                )),
                Arc::new(Field::new("val", DataType::Int64, true)),
            ]
            .into(),
        );
        let formatted = format_data_type(&struct_type);
        assert_eq!(formatted, "struct<ts: timestamp[s, UTC], val: int64>");
        let parsed = parse_data_type(&formatted);
        assert_eq!(
            struct_type, parsed,
            "Struct with bracket types roundtrip failed"
        );
    }

    #[test]
    fn test_malformed_type_falls_back_to_utf8() {
        // Missing closing bracket
        assert_eq!(parse_data_type("timestamp"), DataType::Utf8);
        assert_eq!(parse_data_type("list"), DataType::Utf8);
        assert_eq!(parse_data_type("list<"), DataType::Utf8);
        assert_eq!(parse_data_type("struct<"), DataType::Utf8);
        assert_eq!(parse_data_type("dict<"), DataType::Utf8);
        assert_eq!(parse_data_type("time32["), DataType::Utf8);
        assert_eq!(parse_data_type("duration["), DataType::Utf8);
    }

    #[test]
    fn test_extract_inner_helper() {
        assert_eq!(extract_inner("list<int64>", 5, '>'), Some("int64"));
        assert_eq!(extract_inner("time32[ms]", 7, ']'), Some("ms"));
        assert_eq!(extract_inner("list<", 5, '>'), None);
        assert_eq!(extract_inner("list", 5, '>'), None);
        assert_eq!(extract_inner("", 5, '>'), None);
    }

    // ===== Review Series 1 Wrapup Tests =====

    #[test]
    fn test_list_of_struct_roundtrip() {
        // Nested: list<struct<a: int64, b: utf8>>
        let inner = DataType::Struct(
            vec![
                Arc::new(Field::new("a", DataType::Int64, true)),
                Arc::new(Field::new("b", DataType::Utf8, true)),
            ]
            .into(),
        );
        let list_type = DataType::List(Arc::new(Field::new("item", inner, true)));
        let formatted = format_data_type(&list_type);
        let parsed = parse_data_type(&formatted);
        assert_eq!(list_type, parsed, "list<struct<...>> roundtrip failed");
    }

    #[test]
    fn test_struct_with_multiple_bracket_types() {
        // struct<ts: timestamp[s, UTC], dur: duration[ms], val: int64>
        let struct_type = DataType::Struct(
            vec![
                Arc::new(Field::new(
                    "ts",
                    DataType::Timestamp(TimeUnit::Second, Some(Arc::from("UTC"))),
                    true,
                )),
                Arc::new(Field::new(
                    "dur",
                    DataType::Duration(TimeUnit::Millisecond),
                    true,
                )),
                Arc::new(Field::new("val", DataType::Int64, true)),
            ]
            .into(),
        );
        let formatted = format_data_type(&struct_type);
        assert_eq!(
            formatted,
            "struct<ts: timestamp[s, UTC], dur: duration[ms], val: int64>"
        );
        let parsed = parse_data_type(&formatted);
        assert_eq!(struct_type, parsed);
    }

    #[test]
    fn test_dict_with_list_value_roundtrip() {
        // dict<int32, list<utf8>> - nested type in dict value position
        let list_type = DataType::List(Arc::new(Field::new("item", DataType::Utf8, true)));
        let dict_type = DataType::Dictionary(Box::new(DataType::Int32), Box::new(list_type));
        let formatted = format_data_type(&dict_type);
        let parsed = parse_data_type(&formatted);
        assert_eq!(
            dict_type, parsed,
            "dict<int32, list<utf8>> roundtrip failed"
        );
    }

    #[test]
    fn test_timestamp_all_units_roundtrip() {
        let units = vec![
            (TimeUnit::Second, "s"),
            (TimeUnit::Millisecond, "ms"),
            (TimeUnit::Microsecond, "us"),
            (TimeUnit::Nanosecond, "ns"),
        ];
        for (unit, _label) in units {
            // Without timezone
            let dt = DataType::Timestamp(unit, None);
            let formatted = format_data_type(&dt);
            let parsed = parse_data_type(&formatted);
            assert_eq!(dt, parsed, "timestamp without tz failed for {:?}", unit);

            // With timezone
            let dt_tz = DataType::Timestamp(unit, Some(Arc::from("America/New_York")));
            let formatted_tz = format_data_type(&dt_tz);
            let parsed_tz = parse_data_type(&formatted_tz);
            assert_eq!(dt_tz, parsed_tz, "timestamp with tz failed for {:?}", unit);
        }
    }

    #[test]
    fn test_time32_time64_duration_roundtrip() {
        let dt32 = DataType::Time32(TimeUnit::Millisecond);
        assert_eq!(dt32, parse_data_type(&format_data_type(&dt32)));

        let dt64 = DataType::Time64(TimeUnit::Nanosecond);
        assert_eq!(dt64, parse_data_type(&format_data_type(&dt64)));

        let dur = DataType::Duration(TimeUnit::Microsecond);
        assert_eq!(dur, parse_data_type(&format_data_type(&dur)));
    }

    #[test]
    fn test_large_list_roundtrip() {
        let dt = DataType::LargeList(Arc::new(Field::new("item", DataType::Float64, true)));
        let formatted = format_data_type(&dt);
        assert_eq!(formatted, "large_list<float64>");
        let parsed = parse_data_type(&formatted);
        assert_eq!(dt, parsed);
    }

    #[test]
    fn test_schema_diff_detects_changes() {
        let from = SchemaInfo {
            fields: vec![
                FieldInfo {
                    name: "a".into(),
                    dtype: "int64".into(),
                    nullable: false,
                },
                FieldInfo {
                    name: "b".into(),
                    dtype: "utf8".into(),
                    nullable: true,
                },
            ],
        };
        let to = SchemaInfo {
            fields: vec![
                FieldInfo {
                    name: "a".into(),
                    dtype: "float64".into(),
                    nullable: false,
                },
                FieldInfo {
                    name: "c".into(),
                    dtype: "utf8".into(),
                    nullable: true,
                },
            ],
        };
        let diff = schema_diff(&from, &to);
        assert_eq!(diff.added.len(), 1); // "c" added
        assert_eq!(diff.removed.len(), 1); // "b" removed
        assert_eq!(diff.changed.len(), 1); // "a" type changed
        assert_eq!(diff.added[0].name, "c");
        assert_eq!(diff.removed[0].name, "b");
        assert_eq!(diff.changed[0].name, "a");
    }

    #[test]
    fn test_validate_pipeline_missing_column() {
        let schema = SchemaInfo {
            fields: vec![
                FieldInfo {
                    name: "id".into(),
                    dtype: "int64".into(),
                    nullable: false,
                },
                FieldInfo {
                    name: "value".into(),
                    dtype: "float64".into(),
                    nullable: true,
                },
            ],
        };
        let result = validate_pipeline(&schema, &[("transform1", vec!["id", "missing_col"])]);
        assert!(!result.valid);
        assert!(result.errors[0].contains("missing_col"));
    }
}
