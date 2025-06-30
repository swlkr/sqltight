extern crate self as sqltight;
pub use sqltight_core::{Error, Result, Sqlite, Stmt, Transaction, Tx, Value};
pub use sqltight_macros::db;
use std::collections::BTreeMap;

pub type Text = Option<String>;
pub type Int = Option<i64>;
pub type Blob = Option<Vec<u8>>;
pub type Real = Option<f64>;

#[allow(non_upper_case_globals)]
pub const is: &str = "is";

#[allow(non_upper_case_globals)]
pub const is_not: &str = "is not";

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
    fn save(self, db: &Database) -> sqltight::Result<Self>
    where
        Self: Sized;
    fn delete(self, db: &Database) -> sqltight::Result<Self>
    where
        Self: Sized;
}

#[allow(unused)]
pub struct Database(sqltight::Sqlite);

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

        select get_posts Vec<Post> {
            "where Post.user_id = :user_id
            order by Post.created_at
            limit 2"
        }

        select get_user User {
            "where User.id = :id
             limit 1"
        }

        select get_posts_by_contents Vec<Post> {
            "where Post.content = :content
            or Post.content = :content_1
            order by Post.created_at desc
            limit 2"
        }
    }

    #[test]
    fn it_works() -> sqltight::Result<()> {
        let db = db()?;
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
        let posts = db.get_posts(user.id)?;
        let user = db.get_user(user.id)?;
        assert_eq!(posts.len(), 2);
        assert_eq!(user.id, int(1));
        let posts = db.get_posts_by_contents(text("content"), text("content 2"))?;
        assert_eq!(posts.len(), 2);
        Ok(())
    }
}
