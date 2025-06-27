extern crate self as sqltight;
pub use sqltight_core::{Error, Result, Sqlite, Stmt, Transaction, Tx, Value};
pub use sqltight_macros::schema;
use std::{
    collections::BTreeMap,
    sync::{LazyLock, Mutex},
};

pub type Text = Option<String>;
pub type Int = Option<i64>;
pub type Blob = Option<Vec<u8>>;
pub type Real = Option<f64>;

#[allow(non_upper_case_globals)]
pub const is: &str = "is";

#[allow(non_upper_case_globals)]
pub const is_not: &str = "is not";

#[allow(non_upper_case_globals)]
pub const gte: &str = ">";

pub fn text(s: impl std::fmt::Display) -> Text {
    Some(s.to_string())
}

pub fn int(value: i64) -> Int {
    Some(value)
}

pub fn real(value: f64) -> Real {
    Some(value)
}

pub fn blob(value: Vec<u8>) -> Blob {
    Some(value)
}

pub trait FromRow {
    fn from_row(row: BTreeMap<String, Value>) -> Self;
}

fn open() -> Result<Sqlite> {
    #[cfg(test)]
    let path = ":memory:";
    #[cfg(not(test))]
    let path = "db.sqlite3";
    let db = Sqlite::open(path)?;
    let _result = db.execute(
        "PRAGMA journal_mode = WAL;
        PRAGMA busy_timeout = 5000;
        PRAGMA synchronous = NORMAL;
        PRAGMA cache_size = 1000000000;
        PRAGMA foreign_keys = true;
        PRAGMA temp_store = memory;",
    )?;
    Ok(db)
}

pub fn one<T>(sql: String, params: Vec<Value>) -> Result<T>
where
    T: FromRow,
{
    let row = DATABASE
        .lock()
        .map_err(|_| Error::MutexLockFailed)?
        .prepare(&sql, &params)?
        .rows()?
        .into_iter()
        .nth(0)
        .ok_or(Error::RowNotFound)?;
    Ok(T::from_row(row))
}

pub fn many<T>(sql: String, params: Vec<Value>) -> Result<Vec<T>>
where
    T: FromRow,
{
    let rows = DATABASE
        .lock()
        .map_err(|_| Error::MutexLockFailed)?
        .prepare(&sql, &params)?
        .rows()?
        .into_iter()
        .map(T::from_row)
        .collect::<Vec<T>>();
    Ok(rows)
}

static DATABASE: LazyLock<Mutex<Sqlite>> = LazyLock::new(|| Mutex::new(open().unwrap()));

#[macro_export]
macro_rules! save {
    ($struct_name:ident { $($field:tt : $value:expr),* $(,)? }) => {
        $struct_name {
            $($field : $value),*
            ,..Default::default()
        }.save()
    }
}

#[derive(Debug)]
pub struct Query<T>
where
    T: FromRow,
{
    sql: String,
    params: Vec<Value>,
    ty: std::marker::PhantomData<T>,
}

impl<T> Query<T>
where
    T: FromRow,
{
    pub fn select_where(
        table_name: &'static str,
        left: &'static str,
        op: &'static str,
        right: impl Into<Value>,
    ) -> Self {
        Self {
            sql: format!(
                "select {}.* from {} where {} {} ?",
                table_name, table_name, left, op
            ),
            params: vec![right.into()],
            ty: std::marker::PhantomData,
        }
    }

    pub fn and_where(
        mut self,
        left: &'static str,
        op: &'static str,
        right: impl Into<Value>,
    ) -> Self {
        self.sql.push_str(&format!(" and {} {} ?", left, op));
        self.params.push(right.into());
        self
    }

    pub fn or_where(
        mut self,
        left: &'static str,
        op: &'static str,
        right: impl Into<Value>,
    ) -> Self {
        self.sql.push_str(&format!(" or {} {} ?", left, op));
        self.params.push(right.into());
        self
    }

    pub fn with_sql(table_name: &'static str, sql: &'static str) -> Self {
        Self {
            sql: format!("select {}.* from {} {}", table_name, table_name, sql),
            params: vec![],
            ty: std::marker::PhantomData,
        }
    }

    pub fn params(mut self, params: Vec<Value>) -> Self {
        self.params.extend(params);
        self
    }

    pub fn rows(self) -> Result<Vec<T>> {
        Ok(DATABASE
            .lock()
            .map_err(|_| Error::MutexLockFailed)?
            .prepare(&self.sql, &self.params)?
            .rows()?
            .into_iter()
            .map(T::from_row)
            .collect::<Vec<T>>())
    }

    pub fn first(self) -> Result<T> {
        self.rows()?.into_iter().nth(0).ok_or(Error::RowNotFound)
    }

    pub fn order_desc(mut self, col: &'static str) -> Self {
        self.sql.push_str(&format!(" order by {} desc", col));
        self
    }

    pub fn order_asc(mut self, col: &'static str) -> Self {
        self.sql.push_str(&format!(" order by {} asc", col));
        self
    }

    pub fn limit(mut self, value: u16) -> Self {
        self.sql.push_str(&format!(" limit {}", value));
        self
    }
}

pub trait IntoQuery {
    fn name(&self) -> &'static str;
}

#[macro_export]
macro_rules! op {
    ($tt:tt) => {
        stringify!($tt)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    schema! {
        table User {
            id: Int,
            email: Text
        }

        table Post {
            id: Int,
            user_id: Int,
            content: Text,
            created_at: Int,
            updated_at: Int,
        }

        index User {
            email: Unique
        }

        // query user_posts {
        //   select!(Vec<Post> "where user_id = ?" user_id)
        // }
        // ->
        // pub fn user_posts(user_id: Int) -> Result<Vec<Post>> {
        //   select!(Vec<Post> "where user_id = ?", user_id)
        // }
    }

    #[test]
    fn it_works() -> sqltight::Result<()> {
        let (users, posts) = schema()?;
        let user = save!(User {
            email: text("email"),
        })?;
        assert_eq!(user.id, int(1));
        assert_eq!(user.email, text("email"));
        let mut post = save!(Post {
            content: text("content"),
            user_id: user.id,
        })?;
        assert_eq!(post.id, int(1));
        post.content = text("content 2");
        let post = post.save()?;
        assert_eq!(post.content, text("content 2"));
        let post2 = save!(Post {
            content: text("content"),
            user_id: int(1),
        })?;
        assert_eq!(post2.id, int(2));
        assert_eq!(post2.user_id, int(1));
        let post_rows = posts.select_where(posts.user_id, is, user.id).rows()?;
        let user = users.select_where(users.id, is, user.id).first()?;
        assert_eq!(post_rows.len(), 2);
        assert_eq!(user.id, int(1));
        let post_rows = posts
            .select_where(posts.content, is, "content")
            .or_where(posts.content, is, "content 2")
            .order_desc(posts.created_at)
            .limit(2)
            .rows()?;
        assert_eq!(post_rows.len(), 2);
        Ok(())
    }
}
