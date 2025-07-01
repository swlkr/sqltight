use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    Result, Token, braced,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
};

pub fn db_macro(database: &Database) -> Result<TokenStream2> {
    let Database { parts } = database;
    let migrations = parts.iter().flat_map(migrations).collect::<Vec<_>>();
    let db = sqltight_core::Sqlite::open(":memory:").unwrap();
    let tx = db.transaction().unwrap();
    for migration in &migrations {
        tx.execute(&migration).unwrap();
    }
    let tables = parts
        .into_iter()
        .filter_map(|part| match part {
            SchemaType::Table(table) => Some(table),
            _ => None,
        })
        .collect::<Vec<_>>();
    let table_tokens = tables.iter().map(|table| table_tokens(table));
    let select_tokens = parts
        .into_iter()
        .filter_map(|part| match part {
            SchemaType::Select(select) => Some(select),
            _ => None,
        })
        .map(|select| select_statement(&db, select))
        .collect::<Result<Vec<_>>>()?;
    let tokens = quote! {
        #(#table_tokens)*
        impl Database {
            #(#select_tokens)*
        }
        pub fn db() -> sqltight::Result<Database> {
            #[cfg(test)]
            let path = ":memory:";
            #[cfg(not(test))]
            let path = "db.sqlite3";
            let db = sqltight::Sqlite::open(path)?;
            let _result = db.execute(
                "PRAGMA journal_mode = WAL;
                PRAGMA busy_timeout = 5000;
                PRAGMA synchronous = NORMAL;
                PRAGMA cache_size = 1000000000;
                PRAGMA foreign_keys = true;
                PRAGMA temp_store = memory;",
            )?;
            let _result = db.migrate(&[#(#migrations),*])?;
            Ok(Database(db))
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
        impl sqltight::Crud for #name {
            fn save(self, db: &sqltight::Sqlite) -> sqltight::Result<Self> {
                let sql = #upsert_sql;
                let params = vec![#(#upsert_params),*];
                let row = db
                    .prepare(&sql, &params)?
                    .rows()?
                    .into_iter()
                    .nth(0)
                    .ok_or(sqltight::Error::RowNotFound)?;
                Ok(Self::from_row(&row))
            }
            fn delete(self, db: &sqltight::Sqlite) -> sqltight::Result<Self> {
                let sql = #delete_sql;
                let params = vec![sqltight::Value::Integer(self.id)];
                let row = db
                    .prepare(&sql, &params)?
                    .rows()?
                    .into_iter()
                    .nth(0)
                    .ok_or(sqltight::Error::RowNotFound)?;
                Ok(Self::from_row(&row))
            }
        }
        impl sqltight::FromRow for #name {
            fn from_row(row: &std::collections::BTreeMap<String, sqltight::Value>) -> Self {
                Self {
                    #(#from_row_fields),*
                }
            }
        }
    }
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
        SchemaType::Select(_select) => vec![],
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

fn select_statement(db: &sqltight_core::Sqlite, select: &Select) -> Result<TokenStream2> {
    let Select {
        fn_name,
        return_ty,
        sql,
    } = select;
    let return_val = match is_vec(&return_ty) {
        true => quote! { Ok(rows) },
        false => quote! { rows.into_iter().nth(0).ok_or(sqltight::Error::RowNotFound) },
    };
    let table_name = ident_from_type_path(&return_ty).ok_or(syn::Error::new(
        select.return_ty.span(),
        "Return type expected",
    ))?;
    let sql = format!(
        "select {}.* from {} {}",
        table_name,
        table_name,
        sql.value()
    );
    let param_names = db
        .prepare(&sql, &[])
        .map_err(|e| syn::Error::new(fn_name.span(), format!("{:?}", e)))?
        .parameter_names();
    let param_names = param_names
        .iter()
        .map(|x| x.trim_start_matches(":"))
        .collect::<Vec<_>>();
    let param_idents = param_names
        .iter()
        .map(|name| Ident::new(name, fn_name.span()))
        .collect::<Vec<_>>();
    let args = param_idents
        .iter()
        .filter_map(|name| Some(quote! { #name: impl Into<sqltight::Value> }));
    let params = param_idents
        .iter()
        .map(|arg| quote! { #arg.into() })
        .collect::<Vec<_>>();
    let params = match params.is_empty() {
        true => quote! { &[] },
        false => quote! { &[#(#params,)*] },
    };
    let tokens = quote! {
        pub fn #fn_name(&self, #(#args,)*) -> sqltight::Result<#return_ty> {
            let rows = self.0
                .prepare(#sql, #params)?
                .rows()?
                .iter()
                .map(#table_name::from_row)
                .collect::<Vec<#table_name>>();

            #return_val
        }
    };

    Ok(tokens)
}

mod keyword {
    syn::custom_keyword!(table);
    syn::custom_keyword!(index);
    syn::custom_keyword!(select);
}

enum SchemaType {
    Table(Table),
    Index(Index),
    Select(Select),
}

pub struct Database {
    parts: Vec<SchemaType>,
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

pub struct Select {
    pub fn_name: Ident,
    pub return_ty: syn::TypePath,
    pub sql: syn::LitStr,
}

impl Parse for Select {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<keyword::select>()?;
        let fn_name: Ident = input.parse()?;
        let return_ty: syn::TypePath = input.parse()?;
        let content;
        braced!(content in input);
        let sql = content.parse()?;
        Ok(Select {
            fn_name,
            return_ty,
            sql,
        })
    }
}

impl Parse for Database {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut parts = Vec::new();

        while !input.is_empty() {
            let lookahead = input.lookahead1();
            if lookahead.peek(keyword::table) {
                parts.push(SchemaType::Table(input.parse()?));
            } else if lookahead.peek(keyword::index) {
                parts.push(SchemaType::Index(input.parse()?));
            } else if lookahead.peek(keyword::select) {
                parts.push(SchemaType::Select(input.parse()?));
            } else {
                return Err(lookahead.error());
            }
        }

        Ok(Database { parts })
    }
}

fn ident_from_type_path(ty: &syn::TypePath) -> Option<Ident> {
    match ty.path.segments.last() {
        Some(syn::PathSegment {
            ident,
            arguments: syn::PathArguments::AngleBracketed(args),
            ..
        }) if ident == "Vec" => match args.args.last() {
            Some(syn::GenericArgument::Type(syn::Type::Path(syn::TypePath { path, .. }))) => {
                path.get_ident().cloned()
            }
            _ => return None,
        },
        Some(syn::PathSegment { ident, .. }) if ident != "Vec" => Some(ident.clone()),
        _ => None,
    }
}

fn is_vec(ty: &syn::TypePath) -> bool {
    match ty.path.segments.last() {
        Some(syn::PathSegment {
            ident,
            arguments: syn::PathArguments::AngleBracketed(_args),
            ..
        }) if ident == "Vec" => true,
        _ => false,
    }
}
