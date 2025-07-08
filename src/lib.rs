extern crate self as sqltight;
pub use sqltight_core::{Error, Result, Sqlite, Stmt, Tx, Value};
pub use sqltight_macros::db;
use std::collections::BTreeMap;

#[cfg(test)]
mod tests {
    use super::*;

    db! {
        table User {
            id: Int,
            email: Text,
            created_at: Int,
            updated_at: Int,
        }

        index User {
            email: Unique
        }

        table Post {
            id: Int,
            user_id: Int,
            content: Text,
            created_at: Int,
            updated_at: Int,
        }

        select find_posts (
            Vec<Post>
            "where Post.user_id = :user_id
            order by created_at desc
            limit 2"
        )

        select find_user (User "where id = :id")

        select find_posts_by_contents (
            Vec<Post>
            "where Post.content = :content
            or Post.content = :content_1
            order by Post.created_at desc
            limit 2"
        )
    }

    #[test]
    fn it_works() -> sqltight::Result<()> {
        let db = Database::open(":memory:")?;
        let user = db.save(User {
            email: text("email"),
            ..Default::default()
        })?;
        assert_eq!(user.id, int(1));
        assert_eq!(user.email, text("email"));
        let mut post = db.save(Post {
            content: text("content"),
            user_id: user.id,
            ..Default::default()
        })?;
        assert_eq!(post.id, int(1));
        post.content = text("content 2");
        let post = db.save(post)?;
        assert_eq!(post.content, text("content 2"));
        let post2 = db.save(Post {
            content: text("content"),
            user_id: int(1),
            ..Default::default()
        })?;
        assert_eq!(post2.id, int(2));
        assert_eq!(post2.user_id, int(1));
        let posts = db.find_posts(user.id)?;
        let user = db.find_user(user.id)?;
        assert_eq!(posts.len(), 2);
        assert_eq!(user.id, int(1));
        let posts = db.find_posts_by_contents(text("content"), text("content 2"))?;
        assert_eq!(posts.len(), 2);
        Ok(())
    }

    #[test]
    fn readme() -> sqltight::Result<()> {
        let db = Database::open(":memory:")?;
        let user = User {
            email: text("email"),
            ..Default::default()
        };
        let user = db.save(user)?;

        let user1 = User {
            email: text("email2"),
            ..Default::default()
        };
        let mut user1 = db.save(user1)?;
        // sqlite types are explicit there is no implicit mapping between them
        user1.email = text("email3");
        let user1 = db.save(user1)?;
        let _user1 = db.delete(user1)?;

        let post = Post {
            content: text("content"),
            user_id: user.id,
            ..Default::default()
        };
        let post1 = Post {
            content: text("content1"),
            user_id: user.id,
            ..Default::default()
        };
        {
            let tx = db.transaction()?;
            let _post = tx.save(post)?;
            let _post1 = tx.save(post1)?;
        }

        // queries are defined and prepared into statements
        // ahead of time in the db! macro
        let posts = db.find_posts(user.id)?;
        let found_user = db.find_user(user.id)?;
        assert_eq!(posts.len(), 2);
        assert_eq!(found_user.id, user.id);
        Ok(())
    }
}

pub type Text = Option<String>;
pub type Int = Option<i64>;
pub type Blob = Option<Vec<u8>>;
pub type Real = Option<f64>;

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
    fn from_row(row: &BTreeMap<String, Value>) -> Self;
}

pub trait Crud {
    fn save(self, db: &Sqlite) -> sqltight::Result<Self>
    where
        Self: Sized;

    fn delete(self, db: &Sqlite) -> sqltight::Result<Self>
    where
        Self: Sized;
}

#[allow(unused)]
pub struct Database {
    pub connection: sqltight::Sqlite,
    pub statements: std::collections::HashMap<&'static str, sqltight::Stmt>,
}

impl Database {
    pub fn transaction<'a>(&'a self) -> Result<Transaction<'a>> {
        let tx = self.connection.transaction()?;
        Ok(Transaction(tx))
    }

    pub fn execute(&self, sql: &str) -> Result<i32> {
        self.connection.execute(sql)
    }

    pub fn save<T: sqltight::Crud>(&self, row: T) -> Result<T> {
        row.save(&self.connection)
    }

    pub fn delete<T: sqltight::Crud>(&self, row: T) -> Result<T> {
        row.delete(&self.connection)
    }
}

pub struct Transaction<'a>(sqltight_core::Transaction<'a>);

impl<'a> Transaction<'a> {
    pub fn save<T: sqltight::Crud>(&self, row: T) -> Result<T> {
        row.save(&self.0)
    }

    pub fn delete<T: sqltight::Crud>(&self, row: T) -> Result<T> {
        row.delete(&self.0)
    }
}

pub trait Opener {
    fn open(path: &str) -> Result<Database>;
}
