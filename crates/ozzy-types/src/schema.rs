//! Schema extraction and validation helpers used by the v4 witness layer.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("file not found: {path}: {source}")]
    FileNotFound {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("schema error: {path}: {message}")]
    InvalidSchema { path: String, message: String },
    #[error("invalid stored data type '{ty}'")]
    InvalidStoredDataType { ty: String },
    #[error("invalid time unit '{unit}'")]
    InvalidTimeUnit { unit: String },
    #[error("invalid fixed-size list type '{ty}'")]
    InvalidFixedSizeList { ty: String },
    #[error("invalid struct type '{ty}'")]
    InvalidStruct { ty: String },
    #[error("invalid dictionary type '{ty}'")]
    InvalidDictionary { ty: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Arrow(#[from] arrow::error::ArrowError),
    #[error(transparent)]
    Parquet(#[from] parquet::errors::ParquetError),
}

pub type Result<T> = std::result::Result<T, SchemaError>;

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
    pub fn try_to_arrow_schema(&self) -> Result<Schema> {
        let mut fields = Vec::with_capacity(self.fields.len());
        for field in &self.fields {
            fields.push(Field::new(
                &field.name,
                parse_data_type(&field.dtype)?,
                field.nullable,
            ));
        }
        Ok(Schema::new(fields))
    }

    pub fn contains_columns(&self, required: &[&str]) -> bool {
        required
            .iter()
            .all(|col| self.fields.iter().any(|f| f.name == *col))
    }

    pub fn column_names(&self) -> Vec<&str> {
        self.fields.iter().map(|f| f.name.as_str()).collect()
    }
}

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
            let unit_str = format_time_unit(unit);
            match tz {
                Some(tz) => format!("timestamp[{}, {}]", unit_str, tz),
                None => format!("timestamp[{}]", unit_str),
            }
        }
        DataType::Time32(unit) => format!("time32[{}]", format_time_unit(unit)),
        DataType::Time64(unit) => format!("time64[{}]", format_time_unit(unit)),
        DataType::Duration(unit) => format!("duration[{}]", format_time_unit(unit)),
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

fn format_time_unit(unit: &TimeUnit) -> &'static str {
    match unit {
        TimeUnit::Second => "s",
        TimeUnit::Millisecond => "ms",
        TimeUnit::Microsecond => "us",
        TimeUnit::Nanosecond => "ns",
    }
}

fn parse_time_unit(s: &str) -> Result<TimeUnit> {
    match s {
        "s" => Ok(TimeUnit::Second),
        "ms" => Ok(TimeUnit::Millisecond),
        "us" => Ok(TimeUnit::Microsecond),
        "ns" => Ok(TimeUnit::Nanosecond),
        _ => Err(SchemaError::InvalidTimeUnit {
            unit: s.to_string(),
        }),
    }
}

fn extract_inner<'a>(s: &'a str, prefix_len: usize, close: char) -> Option<&'a str> {
    if s.len() > prefix_len && s.ends_with(close) {
        Some(&s[prefix_len..s.len() - 1])
    } else {
        None
    }
}

fn split_top_level(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in inner.char_indices() {
        match ch {
            '<' | '[' => depth += 1,
            '>' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(inner[start..idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(inner[start..].trim());
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

fn parse_data_type(s: &str) -> Result<DataType> {
    match s {
        "null" => Ok(DataType::Null),
        "bool" => Ok(DataType::Boolean),
        "int8" => Ok(DataType::Int8),
        "int16" => Ok(DataType::Int16),
        "int32" => Ok(DataType::Int32),
        "int64" => Ok(DataType::Int64),
        "uint8" => Ok(DataType::UInt8),
        "uint16" => Ok(DataType::UInt16),
        "uint32" => Ok(DataType::UInt32),
        "uint64" => Ok(DataType::UInt64),
        "float16" => Ok(DataType::Float16),
        "float32" => Ok(DataType::Float32),
        "float64" => Ok(DataType::Float64),
        "utf8" => Ok(DataType::Utf8),
        "large_utf8" => Ok(DataType::LargeUtf8),
        "binary" => Ok(DataType::Binary),
        "large_binary" => Ok(DataType::LargeBinary),
        "date32" => Ok(DataType::Date32),
        "date64" => Ok(DataType::Date64),
        s if s.starts_with("timestamp[") => {
            let inner = extract_inner(s, 10, ']')
                .ok_or_else(|| SchemaError::InvalidStoredDataType { ty: s.to_string() })?;
            let parts = split_top_level(inner);
            let unit = parse_time_unit(
                parts
                    .first()
                    .copied()
                    .ok_or_else(|| SchemaError::InvalidStoredDataType { ty: s.to_string() })?,
            )?;
            let tz = parts.get(1).map(|s| Arc::from(*s));
            Ok(DataType::Timestamp(unit, tz))
        }
        s if s.starts_with("time32[") => {
            let inner = extract_inner(s, 7, ']')
                .ok_or_else(|| SchemaError::InvalidStoredDataType { ty: s.to_string() })?;
            Ok(DataType::Time32(parse_time_unit(inner)?))
        }
        s if s.starts_with("time64[") => {
            let inner = extract_inner(s, 7, ']')
                .ok_or_else(|| SchemaError::InvalidStoredDataType { ty: s.to_string() })?;
            Ok(DataType::Time64(parse_time_unit(inner)?))
        }
        s if s.starts_with("duration[") => {
            let inner = extract_inner(s, 9, ']')
                .ok_or_else(|| SchemaError::InvalidStoredDataType { ty: s.to_string() })?;
            Ok(DataType::Duration(parse_time_unit(inner)?))
        }
        s if s.starts_with("list<") => {
            let inner = extract_inner(s, 5, '>')
                .ok_or_else(|| SchemaError::InvalidStoredDataType { ty: s.to_string() })?;
            Ok(DataType::List(Arc::new(Field::new(
                "item",
                parse_data_type(inner)?,
                true,
            ))))
        }
        s if s.starts_with("large_list<") => {
            let inner = extract_inner(s, 11, '>')
                .ok_or_else(|| SchemaError::InvalidStoredDataType { ty: s.to_string() })?;
            Ok(DataType::LargeList(Arc::new(Field::new(
                "item",
                parse_data_type(inner)?,
                true,
            ))))
        }
        s if s.starts_with("fixed_list<") => {
            let inner = extract_inner(s, 11, '>')
                .ok_or_else(|| SchemaError::InvalidFixedSizeList { ty: s.to_string() })?;
            let Some((item, size)) = inner.rsplit_once(",") else {
                return Err(SchemaError::InvalidFixedSizeList { ty: s.to_string() });
            };
            let size = size
                .trim()
                .parse::<i32>()
                .map_err(|_| SchemaError::InvalidFixedSizeList { ty: s.to_string() })?;
            Ok(DataType::FixedSizeList(
                Arc::new(Field::new("item", parse_data_type(item.trim())?, true)),
                size,
            ))
        }
        s if s.starts_with("struct<") => {
            let inner = extract_inner(s, 7, '>')
                .ok_or_else(|| SchemaError::InvalidStruct { ty: s.to_string() })?;
            let mut fields = Vec::new();
            for part in split_top_level(inner) {
                let Some((name, ty)) = part.split_once(':') else {
                    return Err(SchemaError::InvalidStruct { ty: s.to_string() });
                };
                fields.push(Arc::new(Field::new(
                    name.trim(),
                    parse_data_type(ty.trim())?,
                    true,
                )));
            }
            if fields.is_empty() {
                return Err(SchemaError::InvalidStruct { ty: s.to_string() });
            }
            Ok(DataType::Struct(fields.into()))
        }
        s if s.starts_with("dict<") => {
            let inner = extract_inner(s, 5, '>')
                .ok_or_else(|| SchemaError::InvalidDictionary { ty: s.to_string() })?;
            let parts = split_top_level(inner);
            if parts.len() != 2 {
                return Err(SchemaError::InvalidDictionary { ty: s.to_string() });
            }
            Ok(DataType::Dictionary(
                Box::new(parse_data_type(parts[0])?),
                Box::new(parse_data_type(parts[1])?),
            ))
        }
        _ => Err(SchemaError::InvalidStoredDataType { ty: s.to_string() }),
    }
}

pub fn extract_parquet_schema(path: &Path) -> Result<SchemaInfo> {
    let file = File::open(path).map_err(|source| SchemaError::FileNotFound {
        path: path.display().to_string(),
        source,
    })?;

    let reader = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|err| {
        SchemaError::InvalidSchema {
            path: path.display().to_string(),
            message: err.to_string(),
        }
    })?;

    Ok(SchemaInfo::from(reader.schema().as_ref()))
}

pub fn get_parquet_row_count(path: &Path) -> Result<u64> {
    let file = File::open(path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let metadata = reader.metadata();

    let count: i64 = metadata.row_groups().iter().map(|rg| rg.num_rows()).sum();
    count.try_into().map_err(|_| SchemaError::InvalidSchema {
        path: path.display().to_string(),
        message: format!("negative row count {}", count),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_roundtrip_is_strict() {
        let struct_type = DataType::Struct(
            vec![
                Arc::new(Field::new("name", DataType::Utf8, true)),
                Arc::new(Field::new("age", DataType::Int32, true)),
            ]
            .into(),
        );
        let formatted = format_data_type(&struct_type);
        let parsed = parse_data_type(&formatted).expect("roundtrip should parse");
        assert_eq!(struct_type, parsed);
    }

    #[test]
    fn malformed_types_error_instead_of_falling_back() {
        assert!(parse_data_type("timestamp").is_err());
        assert!(parse_data_type("list<").is_err());
        assert!(parse_data_type("struct<").is_err());
        assert!(parse_data_type("dict<").is_err());
    }

    #[test]
    fn schema_info_try_to_arrow_schema_is_strict() {
        let schema = SchemaInfo {
            fields: vec![FieldInfo {
                name: "a".to_string(),
                dtype: "not_a_real_type".to_string(),
                nullable: true,
            }],
        };

        assert!(schema.try_to_arrow_schema().is_err());
    }
}
