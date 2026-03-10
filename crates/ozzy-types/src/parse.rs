//! Minimal v1 parser for authored type expressions and type references.

use std::collections::BTreeMap;

use ordered_float::OrderedFloat;
use thiserror::Error;

use crate::syntax::{
    BuiltinConstructor, BuiltinType, ConstructorExpr, Literal, RecordExpr, RecordField, TypeExpr,
    TypeRefExpr,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TypeParseError {
    #[error("unexpected end of input")]
    UnexpectedEof,
    #[error("unexpected token near '{near}'")]
    UnexpectedToken { near: String },
    #[error("invalid identifier '{ident}'")]
    InvalidIdentifier { ident: String },
    #[error("unknown constructor '{name}'")]
    UnknownConstructor { name: String },
    #[error("invalid builtin or type reference '{name}'")]
    InvalidTypeReference { name: String },
    #[error("unterminated string literal")]
    UnterminatedString,
    #[error("invalid numeric literal '{literal}'")]
    InvalidNumber { literal: String },
    #[error("trailing input near '{near}'")]
    TrailingInput { near: String },
}

pub fn parse_type_ref(input: &str) -> Result<TypeRefExpr, TypeParseError> {
    let mut parser = Parser::new(input);
    parser.skip_ws();
    let ident = parser.parse_identifier()?;
    let version = if parser.consume_char('@') {
        Some(parser.parse_version()?)
    } else {
        None
    };
    parser.skip_ws();
    if !parser.is_eof() {
        return Err(TypeParseError::TrailingInput {
            near: parser.remaining_preview(),
        });
    }
    Ok(TypeRefExpr::new(ident, version))
}

pub fn parse_type_expr(input: &str) -> Result<TypeExpr, TypeParseError> {
    let mut parser = Parser::new(input);
    parser.skip_ws();
    let expr = parser.parse_intersection()?;
    parser.skip_ws();
    if !parser.is_eof() {
        return Err(TypeParseError::TrailingInput {
            near: parser.remaining_preview(),
        });
    }
    Ok(expr)
}

struct Parser<'a> {
    input: &'a str,
    idx: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, idx: 0 }
    }

    fn is_eof(&self) -> bool {
        self.idx >= self.input.len()
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.idx..]
    }

    fn remaining_preview(&self) -> String {
        self.remaining().chars().take(16).collect()
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.bump_char();
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn bump_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.idx += ch.len_utf8();
        Some(ch)
    }

    fn consume_char(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.bump_char();
            true
        } else {
            false
        }
    }

    fn expect_char(&mut self, expected: char) -> Result<(), TypeParseError> {
        self.skip_ws();
        match self.bump_char() {
            Some(ch) if ch == expected => Ok(()),
            Some(_) => Err(TypeParseError::UnexpectedToken {
                near: self.remaining_preview(),
            }),
            None => Err(TypeParseError::UnexpectedEof),
        }
    }

    fn parse_intersection(&mut self) -> Result<TypeExpr, TypeParseError> {
        let mut parts = vec![self.parse_atom()?];
        loop {
            self.skip_ws();
            if !self.consume_char('&') {
                break;
            }
            self.skip_ws();
            parts.push(self.parse_atom()?);
        }
        if parts.len() == 1 {
            Ok(parts.pop().expect("one part exists"))
        } else {
            Ok(TypeExpr::Intersection(parts))
        }
    }

    fn parse_atom(&mut self) -> Result<TypeExpr, TypeParseError> {
        self.skip_ws();
        match self.peek_char() {
            Some('(') => {
                self.bump_char();
                let expr = self.parse_intersection()?;
                self.skip_ws();
                self.expect_char(')')?;
                Ok(expr)
            }
            Some('{') => self.parse_record_literal(),
            Some(_) => self.parse_ident_led_expr(),
            None => Err(TypeParseError::UnexpectedEof),
        }
    }

    fn parse_ident_led_expr(&mut self) -> Result<TypeExpr, TypeParseError> {
        let ident = self.parse_identifier()?;
        self.skip_ws();

        if ident == "collection" && self.consume_char('<') {
            self.skip_ws();
            let item = self.parse_intersection()?;
            self.skip_ws();
            self.expect_char('>')?;
            return Ok(TypeExpr::Collection(Box::new(item)));
        }

        if ident == "table" && self.consume_char('<') {
            self.skip_ws();
            let row = self.parse_intersection()?;
            self.skip_ws();
            self.expect_char('>')?;
            return Ok(TypeExpr::Table(Box::new(row)));
        }

        if ident == "record" && self.peek_char() == Some('{') {
            return self.parse_record_literal();
        }

        if self.peek_char() == Some('(') {
            self.bump_char();
            let constructor = self.parse_constructor(ident)?;
            return Ok(TypeExpr::Constructor(constructor));
        }

        let version = if self.consume_char('@') {
            Some(self.parse_version()?)
        } else {
            None
        };

        if version.is_none() {
            if let Some(builtin) = BuiltinType::parse(&ident) {
                return Ok(TypeExpr::Builtin(builtin));
            }
        }

        Ok(TypeExpr::Ref(TypeRefExpr::new(ident, version)))
    }

    fn parse_record_literal(&mut self) -> Result<TypeExpr, TypeParseError> {
        self.expect_char('{')?;
        let mut fields = Vec::new();
        let mut open = false;

        loop {
            self.skip_ws();
            if self.consume_char('}') {
                break;
            }

            if self.remaining().starts_with("...") {
                self.idx += 3;
                open = true;
                self.skip_ws();
                self.consume_char(',');
                self.skip_ws();
                self.expect_char('}')?;
                break;
            }

            let name = self.parse_identifier()?;
            self.skip_ws();
            let optional = if self.consume_char('?') { true } else { false };
            self.expect_char(':')?;
            self.skip_ws();
            let ty = self.parse_intersection()?;
            fields.push(RecordField { name, ty, optional });
            self.skip_ws();
            if self.consume_char(',') {
                continue;
            }
            self.expect_char('}')?;
            break;
        }

        Ok(TypeExpr::Record(RecordExpr { fields, open }))
    }

    fn parse_constructor(&mut self, ident: String) -> Result<ConstructorExpr, TypeParseError> {
        let name = match ident.as_str() {
            "csv" => BuiltinConstructor::Csv,
            "unit" => BuiltinConstructor::Unit,
            "min" => BuiltinConstructor::Min,
            "max" => BuiltinConstructor::Max,
            "enum" => BuiltinConstructor::Enum,
            "nullable" => BuiltinConstructor::Nullable,
            _ => return Err(TypeParseError::UnknownConstructor { name: ident }),
        };

        let mut args = BTreeMap::new();
        self.skip_ws();
        if self.consume_char(')') {
            return Ok(ConstructorExpr { name, args });
        }

        loop {
            self.skip_ws();
            let arg_name = self.parse_identifier()?;
            self.skip_ws();
            self.expect_char('=')?;
            self.skip_ws();
            let value = self.parse_literal()?;
            args.insert(arg_name, value);
            self.skip_ws();
            if self.consume_char(',') {
                continue;
            }
            self.expect_char(')')?;
            break;
        }

        Ok(ConstructorExpr { name, args })
    }

    fn parse_literal(&mut self) -> Result<Literal, TypeParseError> {
        self.skip_ws();
        match self.peek_char() {
            Some('"') => self.parse_string().map(Literal::String),
            Some('[') => self.parse_list(),
            Some(ch) if ch.is_ascii_digit() || ch == '-' => self.parse_number(),
            Some(_) => {
                let ident = self.parse_identifier()?;
                match ident.as_str() {
                    "true" => Ok(Literal::Bool(true)),
                    "false" => Ok(Literal::Bool(false)),
                    _ => Err(TypeParseError::UnexpectedToken { near: ident }),
                }
            }
            None => Err(TypeParseError::UnexpectedEof),
        }
    }

    fn parse_list(&mut self) -> Result<Literal, TypeParseError> {
        self.expect_char('[')?;
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.consume_char(']') {
                break;
            }
            items.push(self.parse_literal()?);
            self.skip_ws();
            if self.consume_char(',') {
                continue;
            }
            self.expect_char(']')?;
            break;
        }
        Ok(Literal::List(items))
    }

    fn parse_number(&mut self) -> Result<Literal, TypeParseError> {
        let start = self.idx;
        if self.peek_char() == Some('-') {
            self.bump_char();
        }
        let mut saw_dot = false;
        let mut saw_digit = false;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                saw_digit = true;
                self.bump_char();
            } else if ch == '.' && !saw_dot {
                saw_dot = true;
                self.bump_char();
            } else {
                break;
            }
        }
        let literal = &self.input[start..self.idx];
        if !saw_digit {
            return Err(TypeParseError::InvalidNumber {
                literal: literal.to_string(),
            });
        }
        if saw_dot {
            let value: f64 = literal.parse().map_err(|_| TypeParseError::InvalidNumber {
                literal: literal.to_string(),
            })?;
            Ok(Literal::Float(OrderedFloat(value)))
        } else {
            let value: i64 = literal.parse().map_err(|_| TypeParseError::InvalidNumber {
                literal: literal.to_string(),
            })?;
            Ok(Literal::Integer(value))
        }
    }

    fn parse_string(&mut self) -> Result<String, TypeParseError> {
        self.expect_char('"')?;
        let mut out = String::new();
        loop {
            match self.bump_char() {
                Some('"') => break,
                Some('\\') => match self.bump_char() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => return Err(TypeParseError::UnterminatedString),
                },
                Some(ch) => out.push(ch),
                None => return Err(TypeParseError::UnterminatedString),
            }
        }
        Ok(out)
    }

    fn parse_identifier(&mut self) -> Result<String, TypeParseError> {
        self.skip_ws();
        let start = self.idx;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/') {
                self.bump_char();
            } else {
                break;
            }
        }
        if self.idx == start {
            return Err(TypeParseError::UnexpectedToken {
                near: self.remaining_preview(),
            });
        }
        let ident = &self.input[start..self.idx];
        if ident.is_empty() {
            return Err(TypeParseError::InvalidIdentifier {
                ident: ident.to_string(),
            });
        }
        Ok(ident.to_string())
    }

    fn parse_version(&mut self) -> Result<String, TypeParseError> {
        let start = self.idx;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/') {
                self.bump_char();
            } else {
                break;
            }
        }
        if self.idx == start {
            return Err(TypeParseError::UnexpectedToken {
                near: self.remaining_preview(),
            });
        }
        Ok(self.input[start..self.idx].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{BuiltinConstructor, BuiltinType};

    #[test]
    fn parses_record_table_intersection() {
        let expr = parse_type_expr(
            "csv(delimiter=\",\", header=true) & table<{ species: string, wp: float64 & max(value=0) }>",
        )
        .expect("expression should parse");
        match expr {
            TypeExpr::Intersection(parts) => assert_eq!(parts.len(), 2),
            other => panic!("expected intersection, got {other:?}"),
        }
    }

    #[test]
    fn parses_collection_and_pinned_ref() {
        let expr = parse_type_expr("collection<std/WaterPotentialRow@2>").expect("should parse");
        match expr {
            TypeExpr::Collection(item) => match *item {
                TypeExpr::Ref(type_ref) => {
                    assert_eq!(type_ref.name, "std/WaterPotentialRow");
                    assert_eq!(type_ref.version.as_deref(), Some("2"));
                }
                other => panic!("expected ref, got {other:?}"),
            },
            other => panic!("expected collection, got {other:?}"),
        }
    }

    #[test]
    fn parses_type_ref() {
        let type_ref = parse_type_ref("std/RawCsv@1").expect("type ref should parse");
        assert_eq!(type_ref.name, "std/RawCsv");
        assert_eq!(type_ref.version.as_deref(), Some("1"));
    }

    #[test]
    fn rejects_trailing_input() {
        let err = parse_type_expr("float64 )").expect_err("should fail");
        assert!(matches!(err, TypeParseError::TrailingInput { .. }));
    }

    #[test]
    fn keeps_bare_builtins_as_builtins() {
        let expr = parse_type_expr("float64").expect("builtin should parse");
        assert_eq!(expr, TypeExpr::Builtin(BuiltinType::Float64));
    }

    #[test]
    fn parses_constructor_with_list_literal() {
        let expr = parse_type_expr("enum(values=[\"a\", \"b\"])").expect("should parse");
        match expr {
            TypeExpr::Constructor(ConstructorExpr {
                name: BuiltinConstructor::Enum,
                args,
            }) => assert!(args.contains_key("values")),
            other => panic!("expected enum constructor, got {other:?}"),
        }
    }
}
