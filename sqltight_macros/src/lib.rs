#![feature(proc_macro_quote, proc_macro_totokens)]

mod generator;
mod parser;

use generator::generate;
use parser::parse;
use proc_macro::{TokenStream, quote};

#[proc_macro]
pub fn db(input: TokenStream) -> TokenStream {
    match db_macro(input) {
        Ok(tokens) => tokens,
        Err(err) => to_compile_error(err),
    }
}

fn db_macro(input: TokenStream) -> Result<TokenStream, Error> {
    let schema = parse(input)?;
    let tokens = generate(&schema);
    Ok(tokens)
}

#[derive(Debug)]
enum Error {
    String(String),
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

fn to_compile_error(error: Error) -> TokenStream {
    let err = format!("{:?}", error);
    quote!(compile_error!($err))
}
