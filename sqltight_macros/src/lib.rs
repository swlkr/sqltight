mod db;
use db::{Database, db_macro};
use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::parse_macro_input;

#[proc_macro]
pub fn db(input: TokenStream) -> TokenStream {
    let db = parse_macro_input!(input as Database);
    match db_macro(&db) {
        Ok(s) => TokenStream::from(s).into(),
        Err(e) => {
            let err = format!("{}", e);
            quote! { compile_error!(#err) }.into_token_stream().into()
        }
    }
}
