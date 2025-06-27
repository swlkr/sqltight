use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::{ToTokens, quote};
use sqltight_core::Sqlite;
use syn::{
    Result, Token, braced,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

pub fn schema_macro(schema: &Schema) -> Result<TokenStream2> {
    let Schema { parts } = schema;
    let mut migrations = parts.iter().flat_map(migrations).collect::<Vec<_>>();
    let migrations = migrations.as_slice();
    let db = Sqlite::open(":memory:").unwrap();
    db.migrate(&migrations).unwrap();
    let tokens = parts
        .into_iter()
        .filter_map(|part| match part {
            SchemaType::Table(table) => Some(table.clone()),
            SchemaType::Index(_index) => None,
        })
        .map(|table| {
            (
                table_tokens(table),
                schema_struct_tokens(table),
                tuple_tokens(table).to_token_stream(),
                return_val_tokens(table),
            )
        })
        .collect::<Vec<(TokenStream2, TokenStream2, TokenStream2, TokenStream2)>>();
    let tables = tokens.iter().map(|x| &x.0);
    let schema_structs = tokens.iter().map(|x| &x.1);
    let schema_return_ty = tokens.iter().map(|x| &x.2);
    let schema_return_val = tokens.iter().map(|x| &x.3);
    let tokens = quote! {
        #(#tables)*

        #(#schema_structs)*

        pub fn schema() -> sqltight::Result<(#(#schema_return_ty),*)> {
            sqltight::DATABASE
                .lock()
                .map_err(|_| Error::MutexLockFailed)?
                .migrate(&[#(#migrations),*])?;
            Ok((#(#schema_return_val),*))
        }
    };
    Ok(tokens)
}

fn table_tokens(table: &Table) -> TokenStream2 {
    let Table { name, fields } = table;
    let field_tokens: Vec<_> = fields
        .iter()
        .map(|Field { name, ty }| quote! { pub #name: #ty })
        .collect();
    match fields
        .iter()
        .any(|field| field.name == "id" && field.ty == "Int")
    {
        true => {}
        false => {
            let err = format!("Missing id field on {}", name);
            return quote! {
                pub struct #name {
                    #(#field_tokens,)*
                }
                compile_error(#err)
            };
        }
    }
    let (upsert_sql, upsert_params) = upsert_sql_vec(&table);
    let delete_sql = format!("delete from {} where id = :id returning *", table.name);
    let from_row_fields = table.fields.iter().map(|field| {
        let field_name = &field.name;
        let key = field.name.to_string();
        quote! {
            #field_name: match row.get(#key) {
                Some(val) => val.into(),
                None => None
            }
        }
    });
    quote! {
        #[derive(Default)]
        pub struct #name {
            #(#field_tokens,)*
        }
        impl #name {
            pub fn save(self) -> Result<Self> {
                let sql = #upsert_sql;
                let params = vec![#(#upsert_params),*];
                let row = DATABASE
                    .lock()
                    .map_err(|_| Error::MutexLockFailed)?
                    .prepare(&sql, &params)?
                    .rows()?
                    .into_iter()
                    .nth(0)
                    .ok_or(Error::RowNotFound)?;
                Ok(Self::from_row(row))
            }
            pub fn delete(self) -> Result<Self> {
                let sql = #delete_sql;
                let params = vec![Value::Integer(self.id)];
                let row = DATABASE
                    .lock()
                    .map_err(|_| Error::MutexLockFailed)?
                    .prepare(&sql, &params)?
                    .rows()?
                    .into_iter()
                    .nth(0)
                    .ok_or(Error::RowNotFound)?;
                Ok(Self::from_row(row))
            }
        }
        impl sqltight::FromRow for #name {
            fn from_row(row: std::collections::BTreeMap<String, sqltight::Value>) -> Self {
                Self {
                    #(#from_row_fields),*
                }
            }
        }
    }
}

fn schema_struct_tokens(table: &Table) -> TokenStream2 {
    let Table { name, fields } = table;
    let field_tokens: Vec<_> = fields
        .iter()
        .map(|Field { name, .. }| quote! { pub #name: &'static str })
        .collect();
    let new_tokens: Vec<_> = fields
        .iter()
        .map(|Field { name, .. }| {
            let name_str = name.to_string();
            quote! { #name: #name_str }
        })
        .collect();
    let ident = name;
    let table_name = ident.to_string();
    let name = Ident::new(&format!("{}Schema", ident), table.name.span());
    quote! {
        pub struct #name {
            #(#field_tokens,)*
        }
        impl #name {
            pub fn new() -> Self {
                Self { #(#new_tokens,)* }
            }

            pub fn select_where(&self, left: &'static str, op: &'static str, right: impl Into<sqltight::Value>) -> sqltight::Query<#ident> {
                sqltight::Query::select_where(#table_name, left, op, right)
            }

            pub fn with_sql(&self, sql: &'static str) -> sqltight::Query<#ident> {
                sqltight::Query::with_sql(#table_name, sql)
            }
        }

        impl IntoQuery for #name {
            fn name(&self) -> &'static str {
                #table_name
            }
        }
    }
}

fn tuple_tokens(table: &Table) -> Ident {
    Ident::new(&format!("{}Schema", table.name), table.name.span())
}

fn return_val_tokens(table: &Table) -> TokenStream2 {
    let ident = Ident::new(&format!("{}Schema", table.name), table.name.span());
    quote! { #ident::new() }
}

fn upsert_sql_vec(table: &Table) -> (String, Vec<TokenStream2>) {
    let params: Vec<_> = table
        .fields
        .iter()
        .map(|field| {
            let ident = &field.name;
            quote! { Value::from(self.#ident) }
        })
        .collect();
    let columns = &table.fields;
    let column_names = columns
        .iter()
        .map(|field| field.name.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let placeholders = columns
        .iter()
        .map(|field| format!(":{}", field.name))
        .collect::<Vec<_>>()
        .join(",");
    let set_clause = columns
        .iter()
        .map(|field| format!("{} = excluded.{}", field.name, field.name))
        .collect::<Vec<_>>()
        .join(",");
    (
        format!(
            "insert into {} ({}) values ({}) on conflict (id) do update set {} returning *",
            table.name, column_names, placeholders, set_clause
        ),
        params,
    )
}

fn migrations(part: &SchemaType) -> Vec<String> {
    match part {
        SchemaType::Table(table) => table_migrations(table),
        SchemaType::Index(index) => index_migrations(index),
    }
}

fn table_migrations(table: &Table) -> Vec<String> {
    let table_name = table.name.to_string();
    let columns = table.fields.iter().filter(|field| field.name != "id");
    let mut migrations = vec![format!(
        "create table if not exists {table_name} ( id integer primary key ) strict"
    )];
    migrations.extend(columns.map(|Field { name, ty }| {
        format!("alter table {} add column {} {}", table_name, name, ty)
    }));
    migrations
}

fn index_migrations(index: &Index) -> Vec<String> {
    index
        .fields
        .iter()
        .map(|field| {
            format!(
                "create {} index if not exists {}_{}_ix on {} ({})",
                match field.ty.to_string().as_str() {
                    "Unique" => "unique",
                    _ => "",
                },
                index.name,
                field.name,
                index.name,
                field.name
            )
        })
        .collect()
}

mod keyword {
    syn::custom_keyword!(table);
    syn::custom_keyword!(index);
}

enum SchemaType {
    Table(Table),
    Index(Index),
}

pub struct Schema {
    pub parts: Vec<SchemaType>,
}

pub struct Table {
    pub name: Ident,
    pub fields: Punctuated<Field, Token![,]>,
}

impl Parse for Table {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        input.parse::<keyword::table>()?;
        let name: Ident = input.parse()?;
        let content;
        braced!(content in input);
        let fields = content.parse_terminated(Field::parse, Token![,])?;
        Ok(Table { name, fields })
    }
}

pub struct Index {
    pub name: Ident,
    pub fields: Punctuated<Field, Token![,]>,
}

impl Parse for Index {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<keyword::index>()?;
        let name: Ident = input.parse()?;
        let content;
        braced!(content in input);
        let fields = content.parse_terminated(Field::parse, Token![,])?;
        Ok(Index { name, fields })
    }
}

pub struct Field {
    pub name: Ident,
    pub ty: Ident,
}

impl Parse for Field {
    fn parse(input: ParseStream) -> Result<Self> {
        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty: Ident = input.parse()?;
        Ok(Field { name, ty })
    }
}

impl Parse for Schema {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut parts = Vec::new();

        while !input.is_empty() {
            let lookahead = input.lookahead1();
            if lookahead.peek(keyword::table) {
                parts.push(SchemaType::Table(input.parse()?));
            } else if lookahead.peek(keyword::index) {
                parts.push(SchemaType::Index(input.parse()?));
            } else {
                return Err(lookahead.error());
            }
        }

        Ok(Schema { parts })
    }
}
