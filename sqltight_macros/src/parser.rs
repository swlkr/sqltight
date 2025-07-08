use proc_macro::{Delimiter, Ident, TokenStream, TokenTree};
use std::iter::Peekable;

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
pub struct Select {
    pub fn_name: Ident,
    pub return_ty: ReturnTy,
    pub sql: String,
}

#[derive(Debug)]
pub enum SchemaPart {
    Table(Table),
    Index(Index),
    Select(Select),
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

    fn expect_ident(&mut self) -> Result<Ident, String> {
        match self.tokens.next() {
            Some(TokenTree::Ident(ident)) => Ok(ident),
            Some(other) => Err(format!("Expected an identifier, but got: {}", other)),
            None => Err("Expected an identifier, but found end of stream.".to_string()),
        }
    }

    fn expect_punct(&mut self, expected: char) -> Result<(), String> {
        match self.tokens.next() {
            Some(TokenTree::Punct(punct)) if punct.as_char() == expected => Ok(()),
            Some(other) => Err(format!(
                "Expected punctuation '{}', but got: {}",
                expected, other
            )),
            None => Err(format!(
                "Expected punctuation '{}', but found end of stream.",
                expected
            )),
        }
    }

    fn parse_table(&mut self) -> Result<Table, String> {
        let name = self.expect_ident()?;
        let fields = self.parse_braced_fields()?;
        Ok(Table { name, fields })
    }

    fn parse_index(&mut self) -> Result<Index, String> {
        let name = self.expect_ident()?;
        let fields = self.parse_braced_fields()?;
        Ok(Index { name, fields })
    }

    fn parse_select(&mut self) -> Result<Select, String> {
        let fn_name = self.expect_ident()?;

        match self.tokens.next() {
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
                let mut content_parser = Parser::new(group.stream());
                let return_ty = content_parser.parse_return_ty()?;
                let next_token = content_parser.tokens.next();
                match next_token {
                     Some(TokenTree::Literal(lit)) => {
                         let sql = lit.to_string().trim_matches('"').to_string();
                         Ok(Select { fn_name, return_ty, sql })
                     },
                     _ => Err("Expected a string literal for the SQL query inside the select parentheses.".to_string())
                 }
            }
            _ => {
                Err("Expected a parenthesized group `(...)` for the select statement.".to_string())
            }
        }
    }

    fn parse_braced_fields(&mut self) -> Result<Vec<Field>, String> {
        match self.tokens.next() {
            Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
                let mut content_parser = Parser::new(group.stream());
                content_parser.parse_fields()
            }
            _ => Err("Expected a braced block `{ ... }`".to_string()),
        }
    }

    fn parse_fields(&mut self) -> Result<Vec<Field>, String> {
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

    fn parse_return_ty(&mut self) -> Result<ReturnTy, String> {
        if let Some(TokenTree::Ident(ident)) = self.tokens.next() {
            if ident.to_string() != "Vec" {
                return Ok(ReturnTy::Ident(ident));
            }
        } else {
            return Err("Expected Vec<T> or T".into());
        };

        if let Some(TokenTree::Punct(punct)) = self.tokens.peek() {
            if punct.as_char() == '<' {
                self.tokens.next();
            }
        } else {
            return Err("Expected Vec<T>".into());
        };

        let return_ty = if let Some(TokenTree::Ident(ident)) = self.tokens.next() {
            ReturnTy::Vec(ident.clone())
        } else {
            return Err("Expected Vec<T>".into());
        };

        self.tokens.next(); // get that last >

        Ok(return_ty)
    }
}

pub fn parse(input: TokenStream) -> Result<DatabaseSchema, String> {
    let mut parser = Parser::new(input);
    let mut parts = Vec::new();
    while parser.tokens.peek().is_some() {
        let keyword = parser.expect_ident()?;
        match keyword.to_string().as_str() {
            "table" => parts.push(SchemaPart::Table(parser.parse_table()?)),
            "index" => parts.push(SchemaPart::Index(parser.parse_index()?)),
            "select" => parts.push(SchemaPart::Select(parser.parse_select()?)),
            _ => {
                return Err(format!(
                    "Unexpected keyword: {}. Expected 'table', 'index', or 'select'.",
                    keyword
                ));
            }
        }
    }
    Ok(DatabaseSchema { parts })
}

#[derive(Debug)]
pub enum ReturnTy {
    Vec(Ident),
    Ident(Ident),
}

impl ReturnTy {
    pub fn ident(&self) -> &Ident {
        match self {
            ReturnTy::Vec(ident) => ident,
            ReturnTy::Ident(ident) => ident,
        }
    }
}
