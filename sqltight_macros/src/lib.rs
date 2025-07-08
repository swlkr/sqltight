#![feature(proc_macro_quote, proc_macro_totokens, proc_macro_diagnostic)]

mod generator;
mod parser;

use generator::generate;
use parser::parse;
use proc_macro::{Span, TokenStream, quote};

#[proc_macro]
pub fn db(input: TokenStream) -> TokenStream {
    match db_macro(input) {
        Ok(tokens) => tokens,
        Err(err) => to_compile_error(err),
    }
}

fn db_macro(input: TokenStream) -> Result<TokenStream, Error> {
    let schema = parse(input)?;
    let tokens = generate(&schema)?;
    Ok(tokens)
}

enum Error {
    Generate { text: String, span: Span },
    Parse { text: String, span: Span },
}

impl Error {
    pub fn msg(&self) -> &str {
        match self {
            Error::Generate { text, .. } => text,
            Error::Parse { text, .. } => text,
        }
    }
}

fn to_compile_error(err: Error) -> TokenStream {
    quote! {
        compile_error!("Failed to build macro")
    }
}
