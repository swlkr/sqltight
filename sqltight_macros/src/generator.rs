use crate::{
    Error,
    parser::{DatabaseSchema, Field, Index, Query, SchemaPart, Table},
};
use proc_macro::{Diagnostic, Ident, Level, Span, TokenStream, quote};

pub fn generate(schema: &DatabaseSchema) -> Result<TokenStream, Error> {
    let db = sqltight_core::Sqlite::open(":memory:").unwrap();
    let migrations = schema.parts.iter().flat_map(migration).collect::<Vec<_>>();
    let _result = db.migrate(&migrations)?;
    let table_tokens = schema
        .parts
        .iter()
        .filter_map(|part| match part {
            SchemaPart::Table(table) => Some(generate_table(table)),
            SchemaPart::Index(_index) => None,
            SchemaPart::Query(_select) => None,
        })
        .collect::<Result<TokenStream, Error>>()?;
    let select_tokens = schema
        .parts
        .iter()
        .filter_map(|part| match part {
            SchemaPart::Table(_table) => None,
            SchemaPart::Index(_index) => None,
            SchemaPart::Query(select) => Some(generate_select(&db, select)),
        })
        .collect::<Result<TokenStream, Error>>()?;
    let select_struct_tokens = schema
        .parts
        .iter()
        .filter_map(|part| match part {
            SchemaPart::Table(_table) => None,
            SchemaPart::Index(_index) => None,
            SchemaPart::Query(select) => Some(generate_select_struct(&db, select)),
        })
        .collect::<Result<TokenStream, Error>>()?;
    let migration_tokens = migrations
        .iter()
        .map(|mig| quote! { $mig, })
        .collect::<TokenStream>();
    let statements = schema
        .parts
        .iter()
        .map(statement_from_part)
        .collect::<TokenStream>();
    // HACK: call_site spans for each ident
    let database = Ident::new("Database", Span::call_site());
    let open_fn = Ident::new("open", Span::call_site());
    let transaction = Ident::new("transaction", Span::call_site());
    let execute = Ident::new("execute", Span::call_site());
    let save = Ident::new("save", Span::call_site());
    let delete = Ident::new("delete", Span::call_site());

    Ok(quote! {
        #[allow(unused)]
        pub struct $database {
            pub connection: sqltight::Sqlite,
            pub statements: std::collections::HashMap<&'static str, sqltight::Stmt>,
        }

        impl $database {
            pub fn $transaction<'a>(&'a self) -> sqltight::Result<sqltight::Transaction<'a>> {
                let tx = self.connection.transaction()?;
                Ok(sqltight::Transaction(tx))
            }

            pub fn $execute(&self, sql: &str) -> sqltight::Result<i32> {
                self.connection.execute(sql)
            }

            pub fn $save<T: sqltight::Crud>(&self, row: T) -> sqltight::Result<T> {
                row.save(&self.connection)
            }

            pub fn $delete<T: sqltight::Crud>(&self, row: T) -> sqltight::Result<T> {
                row.delete(&self.connection)
            }

            pub fn $open_fn(path: &str) -> sqltight::Result<Self> {
                let connection = sqltight::Sqlite::open(path)?;
                let _result = connection.execute(
                    "PRAGMA journal_mode = WAL;
                    PRAGMA busy_timeout = 5000;
                    PRAGMA synchronous = NORMAL;
                    PRAGMA cache_size = 1000000000;
                    PRAGMA foreign_keys = true;
                    PRAGMA temp_store = memory;",
                )?;
                let _result = connection.migrate(&[$migration_tokens])?;
                let statements: std::collections::HashMap<&'static str, sqltight::Stmt> = vec![$statements].into_iter().collect();
                Ok(Self { connection, statements })
            }

            $select_tokens
        }

        $table_tokens
        $select_struct_tokens
    })
}

fn migration(part: &SchemaPart) -> Vec<String> {
    match part {
        SchemaPart::Table(table) => table_migrations(table),
        SchemaPart::Index(index) => index_migrations(index),
        SchemaPart::Query(_select) => vec![],
    }
}

fn table_migrations(table: &Table) -> Vec<String> {
    let table_name = table.name.to_string();
    let columns = table
        .fields
        .iter()
        .filter(|field| field.name.to_string() != "id");
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

fn generate_table(table: &Table) -> Result<TokenStream, Error> {
    let name = &table.name;
    let fields = table
        .fields
        .iter()
        .map(|Field { name, ty }| quote! { pub $name: $ty, })
        .collect::<TokenStream>();
    let (upsert_sql, upsert_params) = upsert_sql(table);
    let delete_sql = format!("delete from {name} where id = :id returning *");
    let from_row_fields = table
        .fields
        .iter()
        .map(|field| {
            let field_name = &field.name;
            let key = field.name.to_string();
            quote!($field_name: match row.get($key) { Some(val) => val.into(), None => None },)
        })
        .collect::<TokenStream>();
    let id = match table
        .fields
        .iter()
        .find(|field| field.name.to_string() == "id")
        .map(|field| &field.name)
    {
        Some(id) => id,
        None => {
            Diagnostic::spanned(
                table.name.span(),
                Level::Error,
                "Missing required column: id",
            )
            .emit();
            return Err(Error::Generate("Missing required column: id".to_string()));
        }
    };

    Ok(quote! {
        #[derive(Default)]
        pub struct $name {
            $fields
        }
        impl sqltight::Crud for $name {
            fn save(self, db: &sqltight::Sqlite) -> sqltight::Result<Self> {
                let sql = $upsert_sql;
                let params = vec![$upsert_params];
                let row = db.prepare(&sql)?
                    .bind(&params)?
                    .rows()?
                    .into_iter()
                    .nth(0)
                    .ok_or(sqltight::Error::RowNotFound)?;
                Ok(Self::from_row(&row))
            }

            fn delete(self, db: &sqltight::Sqlite) -> sqltight::Result<Self> {
                let sql = $delete_sql;
                let params = vec![sqltight::Value::Integer(self.$id)];
                let row = db
                    .prepare(&sql)?
                    .bind(&params)?
                    .rows()?
                    .into_iter()
                    .nth(0)
                    .ok_or(sqltight::Error::RowNotFound)?;
                Ok(Self::from_row(&row))
            }
        }

        impl sqltight::FromRow for $name {
            fn from_row(row: &std::collections::BTreeMap<String, sqltight::Value>) -> Self {
                Self {
                    $from_row_fields
                }
            }
        }
    })
}

fn pascal_case(name: &str) -> String {
    name.split("_")
        .map(|x| {
            format!(
                "{}{}",
                x.chars().nth(0).unwrap().to_uppercase(),
                x.chars().skip(1).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn generate_select(db: &sqltight_core::Sqlite, select: &Query) -> Result<TokenStream, Error> {
    let sql = &select.sql;
    let fn_name = &select.fn_name;
    let return_ident = Ident::new(&pascal_case(&fn_name.to_string()), fn_name.span());
    let (return_ty, return_val) = match sql.contains("limit 1") {
        false => (quote!(Vec<$return_ident>), quote!(Ok(rows))),
        true => (
            quote!($return_ident),
            quote!(rows.into_iter().nth(0).ok_or(sqltight::Error::RowNotFound)),
        ),
    };
    let stmt = match db.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(err) => match err {
            sqltight_core::Error::Sqlite { text, .. } => {
                Diagnostic::spanned(fn_name.span(), Level::Error, &text).emit();
                return Err(Error::Generate(text));
            }
            _ => todo!(),
        },
    };
    let param_names = stmt.parameter_names();
    let param_names = param_names
        .iter()
        .map(|x| x.trim_start_matches(":"))
        .collect::<Vec<_>>();
    let param_idents = param_names
        .iter()
        .map(|name| Ident::new(name, fn_name.span()))
        .collect::<Vec<_>>();
    let fn_args = param_idents
        .iter()
        .map(|arg| quote!($arg: impl Into<sqltight::Value>,))
        .collect::<TokenStream>();
    let params = param_idents
        .iter()
        .map(|arg| quote!($arg.into(),))
        .collect::<TokenStream>();
    let params = quote!(&[$params]);
    let fn_name_str = fn_name.to_string();
    Ok(quote!(
        #[doc = $sql]
        pub fn $fn_name(&self, $fn_args) -> sqltight::Result<$return_ty> {
            let rows = self.statements.get($fn_name_str).unwrap()
                .bind($params)?
                .rows()?
                .iter()
                .map($return_ident::from_row)
                .collect::<Vec<$return_ident>>();
            $return_val
        }
    ))
}

fn generate_select_struct(
    db: &sqltight_core::Sqlite,
    select: &Query,
) -> Result<TokenStream, Error> {
    let sql = &select.sql;
    let fn_name = &select.fn_name;
    let struct_ident = Ident::new(&pascal_case(&fn_name.to_string()), fn_name.span());
    let stmt = match db.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(err) => match err {
            sqltight_core::Error::Sqlite { text, .. } => {
                Diagnostic::spanned(fn_name.span(), Level::Error, &text).emit();
                return Err(Error::Generate(text));
            }
            _ => todo!(),
        },
    };
    let column_names = stmt.select_column_names();
    let column_types = stmt.select_column_types();
    let columns = column_names
        .into_iter()
        .zip(column_types)
        .collect::<Vec<_>>();
    let fields = columns
        .iter()
        .map(|(name, ty)| {
            let name = Ident::new(name, fn_name.span());
            let ty = match ty.as_str() {
                "INTEGER" | "INT" => "Int",
                "TEXT" => "Text",
                "BLOB" => "Blob",
                "REAL" => "Real",
                _ => "Blob",
            };
            let ty = Ident::new(ty, fn_name.span());
            quote! { pub $name: $ty, }
        })
        .collect::<TokenStream>();
    let from_row_fields = columns
        .iter()
        .map(|(name, ..)| {
            let ident = Ident::new(name, fn_name.span());
            quote!($ident: match row.get($name) { Some(val) => val.into(), None => None },)
        })
        .collect::<TokenStream>();

    Ok(quote!(
        pub struct $struct_ident {
            $fields
        }

        impl sqltight::FromRow for $struct_ident {
            fn from_row(row: &std::collections::BTreeMap<String, sqltight::Value>) -> Self {
                Self {
                    $from_row_fields
                }
            }
        }
    ))
}

fn upsert_sql(table: &Table) -> (String, TokenStream) {
    let columns: Vec<_> = table.fields.iter().map(|f| f.name.to_string()).collect();
    let column_names = columns.join(",");
    let placeholders = columns
        .iter()
        .map(|c| format!(":{c}"))
        .collect::<Vec<_>>()
        .join(",");
    let set_clause = columns
        .iter()
        .map(|c| format!("{c} = excluded.{c}"))
        .collect::<Vec<_>>()
        .join(",");

    let sql = format!(
        "insert into {} ({}) values ({}) on conflict (id) do update set {} returning *",
        table.name, column_names, placeholders, set_clause
    );

    let params = table
        .fields
        .iter()
        .map(|Field { name, .. }| quote!(sqltight::Value::from(self.$name),))
        .collect::<TokenStream>();

    (sql, params)
}

impl From<sqltight_core::Error> for Error {
    fn from(value: sqltight_core::Error) -> Self {
        match value {
            sqltight_core::Error::Sqlite { text, .. } => Self::Generate(text),
            _ => todo!(),
        }
    }
}

fn statement_from_part(part: &SchemaPart) -> TokenStream {
    match part {
        SchemaPart::Table(_table) => TokenStream::new(),
        SchemaPart::Index(_index) => TokenStream::new(),
        SchemaPart::Query(select) => statement_from_select(select),
    }
}

fn statement_from_select(select: &Query) -> TokenStream {
    let key = select.fn_name.to_string();
    let sql = &select.sql;
    quote! {
        ($key, connection.prepare($sql)?),
    }
}
