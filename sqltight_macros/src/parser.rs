use proc_macro::{Delimiter, Ident, TokenStream, TokenTree};
use std::iter::Peekable;

use crate::Error;

#[derive(Debug, Clone)]
pub struct Field {
    pub name: Ident,
    pub ty: Ident,
}

#[derive(Debug)]
pub struct Table {
    pub name: Ident,
    pub fields: Vec<Field>,
}

#[derive(Debug)]
pub struct Index {
    pub name: Ident,
    pub fields: Vec<Field>,
}

#[derive(Debug)]
pub struct Query {
    pub fn_name: Ident,
    pub sql: String,
}

#[derive(Debug)]
pub enum SchemaPart {
    Table(Table),
    Index(Index),
    Query(Query),
}

#[derive(Debug)]
pub struct DatabaseSchema {
    pub parts: Vec<SchemaPart>,
}

pub struct Parser<I: Iterator<Item = TokenTree>> {
    tokens: Peekable<I>,
}

impl Parser<proc_macro::token_stream::IntoIter> {
    pub fn new(input: TokenStream) -> Self {
        Parser {
            tokens: input.into_iter().peekable(),
        }
    }

    fn expect_ident(&mut self) -> Result<Ident, Error> {
        match self.tokens.next() {
            Some(TokenTree::Ident(ident)) => Ok(ident),
            Some(other) => Err(Error::Parse(format!(
                "Expected an identifier, but got: {}",
                other
            ))),
            None => Err(Error::Parse(
                "Expected an identifier, but found end of stream.".to_string(),
            )),
        }
    }

    fn expect_punct(&mut self, expected: char) -> Result<(), Error> {
        match self.tokens.next() {
            Some(TokenTree::Punct(punct)) if punct.as_char() == expected => Ok(()),
            Some(other) => Err(Error::Parse(format!(
                "Expected punctuation '{}', but got: {}",
                expected, other
            ))),
            None => Err(Error::Parse(format!(
                "Expected punctuation '{}', but found end of stream.",
                expected
            ))),
        }
    }

    fn parse_table(&mut self) -> Result<Table, Error> {
        let name = self.expect_ident()?;
        let fields = self.parse_braced_fields()?;
        Ok(Table { name, fields })
    }

    fn parse_index(&mut self) -> Result<Index, Error> {
        let name = self.expect_ident()?;
        let fields = self.parse_braced_fields()?;
        Ok(Index { name, fields })
    }

    fn parse_query(&mut self) -> Result<Query, Error> {
        let fn_name = self.expect_ident()?;
        match self.tokens.next() {
            Some(TokenTree::Literal(lit)) => {
                let sql = lit.to_string().trim_matches('"').to_string();
                Ok(Query { fn_name, sql })
            }
            _ => Err(Error::Parse(
                "Expected a string literal for the SQL query inside the select parentheses."
                    .to_string(),
            )),
        }
    }

    fn parse_braced_fields(&mut self) -> Result<Vec<Field>, Error> {
        match self.tokens.next() {
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
                let mut content_parser = Parser::new(group.stream());
                content_parser.parse_fields()
            }
            _other => Err(Error::Parse(
                "Expected a braced block `{ ... }`".to_string(),
            )),
        }
    }

    fn parse_fields(&mut self) -> Result<Vec<Field>, Error> {
        let mut fields = Vec::new();
        while self.tokens.peek().is_some() {
            let name = self.expect_ident()?;
            self.expect_punct(':')?;
            let ty = self.expect_ident()?;
            fields.push(Field { name, ty });

            if let Some(TokenTree::Punct(p)) = self.tokens.peek() {
                if p.as_char() == ',' {
                    self.tokens.next();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(fields)
    }
}

pub fn parse(input: TokenStream) -> Result<DatabaseSchema, Error> {
    let mut parser = Parser::new(input);
    let mut parts = Vec::new();
    while parser.tokens.peek().is_some() {
        let keyword = parser.expect_ident()?;
        match keyword.to_string().as_str() {
            "table" => parts.push(SchemaPart::Table(parser.parse_table()?)),
            "index" => parts.push(SchemaPart::Index(parser.parse_index()?)),
            "query" => parts.push(SchemaPart::Query(parser.parse_query()?)),
            _ => {
                return Err(Error::Parse(format!(
                    "Unexpected keyword: {}. Expected 'table', 'index', or 'query'.",
                    keyword
                )));
            }
        }
    }
    Ok(DatabaseSchema { parts })
}
