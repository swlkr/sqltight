mod schema;
use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use schema::{Schema, schema_macro};
use syn::parse_macro_input;

#[proc_macro]
pub fn schema(input: TokenStream) -> TokenStream {
    let schema = parse_macro_input!(input as Schema);
    match schema_macro(&schema) {
        Ok(s) => TokenStream::from(s).into(),
        Err(e) => {
            let err = format!("{}", e);
            quote! { compile_error!(#err) }.into_token_stream().into()
        }
    }
}
