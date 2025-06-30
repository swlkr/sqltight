mod db;
use db::{Database, db_macro};
use proc_macro::TokenStream;
use syn::parse_macro_input;

#[proc_macro]
pub fn db(input: TokenStream) -> TokenStream {
    let db = parse_macro_input!(input as Database);
    db_macro(&db)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
