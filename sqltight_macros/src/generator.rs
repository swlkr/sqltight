use crate::{
    Error,
    parser::{DatabaseSchema, Field, Index, ReturnTy, SchemaPart, Select, Table},
};
use proc_macro::{Diagnostic, Ident, Level, Span, TokenStream, quote};

pub fn generate(schema: &DatabaseSchema) -> Result<TokenStream, Error> {
    let db = sqltight_core::Sqlite::open(":memory:").unwrap();
    let migrations = schema.parts.iter().flat_map(migration).collect::<Vec<_>>();
    let _result = db.migrate(&migrations)?;
    let tokens = schema
        .parts
        .iter()
        .map(|part| generate_part(&db, part))
        .collect::<Result<TokenStream, Error>>()?;
    let migration_tokens = migrations
        .iter()
        .map(|mig| quote! { $mig, })
        .collect::<TokenStream>();

    Ok(quote! {
        impl sqltight::Opener for sqltight::Database {
            fn open(path: &str) -> sqltight::Result<sqltight::Database> {
                let conn = sqltight::Sqlite::open(path)?;
                let _result = conn.execute(
                    "PRAGMA journal_mode = WAL;
                    PRAGMA busy_timeout = 5000;
                    PRAGMA synchronous = NORMAL;
                    PRAGMA cache_size = 1000000000;
                    PRAGMA foreign_keys = true;
                    PRAGMA temp_store = memory;",
                )?;
                let _result = conn.migrate(&[$migration_tokens])?;
                Ok(sqltight::Database(conn))
            }
        }

        $tokens
    })
}

fn migration(part: &SchemaPart) -> Vec<String> {
    match part {
        SchemaPart::Table(table) => table_migrations(table),
        SchemaPart::Index(index) => index_migrations(index),
        SchemaPart::Select(_select) => vec![],
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

fn generate_part(db: &sqltight_core::Sqlite, part: &SchemaPart) -> Result<TokenStream, Error> {
    match part {
        SchemaPart::Table(table) => Ok(generate_table(table)),
        SchemaPart::Index(_index) => Ok(TokenStream::new()),
        SchemaPart::Select(select) => generate_select(db, select),
    }
}

fn generate_table(table: &Table) -> TokenStream {
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

    quote! {
        #[derive(Default)]
        pub struct $name {
            id: sqltight::Int,
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
                let params = vec![sqltight::Value::Integer(self.id)];
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
                    id: row.get("id").unwrap().into(),
                    $from_row_fields
                }
            }
        }
    }
}

fn generate_select(db: &sqltight_core::Sqlite, select: &Select) -> Result<TokenStream, Error> {
    let Select {
        fn_name,
        return_ty,
        sql,
    } = select;
    let return_val = match return_ty {
        ReturnTy::Vec(_) => quote!(Ok(rows)),
        ReturnTy::Ident(_) => quote!(rows.into_iter().nth(0).ok_or(sqltight::Error::RowNotFound)),
    };
    let table_name = return_ty.ident();
    let table_name_str = table_name.to_string();
    let sql = format!("select {table_name_str}.* from {table_name_str} {sql}");
    let stmt = match db.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(err) => match err {
            sqltight_core::Error::Sqlite { text, .. } => {
                Diagnostic::spanned(fn_name.span(), Level::Error, &text).emit();
                return Err(Error::Generate {
                    text,
                    span: fn_name.span(),
                });
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
    let return_ty_tokens = match return_ty {
        ReturnTy::Vec(ident) => quote! { Vec<$ident> },
        ReturnTy::Ident(ident) => quote! { $ident },
    };
    Ok(quote!(
        impl sqltight::Database {
            pub fn $fn_name(&self, $fn_args) -> sqltight::Result<$return_ty_tokens> {
                let rows = self.0
                    .prepare($sql)?
                    .bind($params)?
                    .rows()?
                    .iter()
                    .map($table_name::from_row)
                    .collect::<Vec<$table_name>>();
                $return_val
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
            sqltight_core::Error::Sqlite { text, .. } => Self::Generate {
                text,
                span: Span::call_site(),
            },
            _ => todo!(),
        }
    }
}
