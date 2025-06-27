use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::quote;

// pub fn select_macro(select: &Select) -> syn::Result<TokenStream2> {
//     // let (table_name, return_ty) = select_table_and_return_type(&select)?;
//     let params = select
//         .params
//         .iter()
//         .map(|x| quote! { Value::from(#x) })
//         .collect::<Vec<_>>();
//     let table_ident = &select.table;
//     let parts = &select
//         .sql
//         .value()
//         .split_whitespace()
//         .into_iter()
//         .map(|part| match part {
//             "=" => Ok(quote! { sqltight::is }),
//             part if part.contains(".") => {
//                 let expr = syn::parse_str::<syn::Expr>(part)?;
//                 Ok(quote! { #expr })
//             }
//             part => Ok(quote! { #part }),
//         })
//         .collect::<syn::Result<Vec<_>>>()?;
//     // let sql = format!("select {}.* from {} {}", table_name, input_sql.value());
//     // let sql = quote! { format!(#sql) };
//     // let function = match return_ty {
//     //     ReturnTy::One => quote! { sqltight::one },
//     //     ReturnTy::Many => quote! { sqltight::many },
//     // };
//     let tokens = quote! {
//         #table_ident.with_sql(#(#parts)).params(vec![#(#params),*])
//         // #function::<#table_name>(#sql, vec![#(#params),*])
//     };
//     Ok(tokens)
// }

enum ReturnTy {
    One,
    Many,
}

// fn select_table_and_return_type<'a>(select: &'a Select) -> Result<(&'a Ident, ReturnTy)> {
//     match select.table {
//         syn::Type::Path(syn::TypePath { ref path, .. }) => match path.segments.last() {
//             Some(syn::PathSegment {
//                 ident,
//                 arguments: syn::PathArguments::AngleBracketed(args),
//                 ..
//             }) if ident == "Vec" => {
//                 let ty = match args.args.last() {
//                     Some(syn::GenericArgument::Type(syn::Type::Path(syn::TypePath {
//                         path,
//                         ..
//                     }))) => path.get_ident(),
//                     _ => return Err(Error::UnsupportedType),
//                 }
//                 .ok_or(Error::UnsupportedType)?;
//                 return Ok((ty, ReturnTy::Many));
//             }
//             Some(syn::PathSegment { ident, .. }) if ident != "Vec" => {
//                 return Ok((ident, ReturnTy::One));
//             }
//             _ => return Err(Error::UnsupportedType),
//         },
//         _ => return Err(Error::UnsupportedType),
//     }
// }

#[derive(Debug)]
enum Ty {
    Int,
    Text,
    Real,
    Blob,
    Any,
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Ty::Int => "integer",
            Ty::Text => "text",
            Ty::Real => "real",
            Ty::Blob => "blob",
            Ty::Any => "any",
        })
    }
}

impl TryFrom<&syn::Type> for Ty {
    type Error = Error;

    fn try_from(value: &syn::Type) -> Result<Self> {
        match value {
            syn::Type::Path(type_path) => {
                match type_path
                    .path
                    .get_ident()
                    .ok_or(Error::UnsupportedType)?
                    .to_string()
                    .as_str()
                {
                    "Int" => Ok(Ty::Int),
                    "Text" => Ok(Ty::Text),
                    "Real" => Ok(Ty::Real),
                    "Blob" => Ok(Ty::Blob),
                    _ => Ok(Ty::Any),
                }
            }
            _ => Err(Error::UnsupportedType),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Sql(sqltight_core::Error),
    UnsupportedFields,
    UnsupportedType,
    MissingIdField,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::MissingIdField => f.write_str("Missing field id: Int. Add it"),
            error => f.write_fmt(format_args!("{:?}", error)),
        }
    }
}

impl From<sqltight_core::Error> for Error {
    fn from(value: sqltight_core::Error) -> Self {
        Self::Sql(value)
    }
}

type Result<T> = std::result::Result<T, Error>;

pub struct Select {
    table: syn::Ident,
    sql: syn::LitStr,
    params: Vec<syn::Expr>,
}

impl syn::parse::Parse for Select {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let table: syn::Ident = input.parse()?;
        let sql: syn::LitStr = input.parse()?;
        let mut params: Vec<syn::Expr> = vec![];
        while let Ok(part) = input.parse() {
            params.push(part);
        }

        Ok(Self { table, sql, params })
    }
}
