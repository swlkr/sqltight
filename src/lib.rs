extern crate self as sqltight;
pub use sqltight_core::{
    Blob, Crud, Error, FromRow, Int, Real, Result, Sqlite, Stmt, Text, Tx, Value, blob, int, real,
    text,
};
pub use sqltight_macros::db;

#[cfg(test)]
mod tests {
    use super::*;

    db! {
        table User {
            id: Int,
            email: Text,
            created_at: Int,
            updated_at: Int
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

        query posts_by_user_id "
            select post.id
            from post
            where post.user_id = :user_id
            order by post.created_at desc
            limit 2
        "

        query user_by_id "select user.id from user where id = :id limit 1"

        query posts_by_contents "
            select id, content, user_id, created_at
            from post
            where content = :content
            or content = :content_1
            order by created_at desc
            limit 2
        "

        query count_posts_by_user "
            select count(post.id) as post_count, user.id, user.email
            from post
            join user on user.id = post.user_id
            group by post.user_id
            order by post_count desc
            limit 1
        "
    }

    #[test]
    fn it_works() -> sqltight::Result<()> {
        let db = Database::open(":memory:")?;
        let user = User::new("email");
        let user = db.save(user)?;
        assert_eq!(user.id, int(1));
        assert_eq!(user.email, text("email"));
        let mut post = db.save(Post::new(user.id, "content"))?;
        assert_eq!(post.id, int(1));
        post.content = text("content 2");
        let post = db.save(post)?;
        assert_eq!(post.content, text("content 2"));
        let post2 = db.save(Post::new(user.id, "content"))?;
        assert_eq!(post2.id, int(2));
        assert_eq!(post2.user_id, int(1));
        let posts = db.posts_by_user_id(user.id)?;
        let user = db.user_by_id(user.id)?;
        assert_eq!(posts.len(), 2);
        assert_eq!(user.id, int(1));
        let posts = db.posts_by_contents("content", "content 2")?;
        assert_eq!(posts.len(), 2);
        let row = db.count_posts_by_user()?;
        assert_eq!(row.post_count, int(2));
        assert_eq!(row.id, user.id);
        Ok(())
    }

    #[test]
    fn readme() -> sqltight::Result<()> {
        let db = Database::open(":memory:")?;
        let user = User::new("email");
        let user = db.save(user)?;

        let user1 = User::new("email2");
        let mut user1 = db.save(user1)?;

        // sqlite types are explicit there is no implicit mapping between them
        user1.email = text("email3");

        let user1 = db.save(user1)?;
        let _user1 = db.delete(user1)?;

        let post = Post::new(user.id, "content");
        let post1 = Post::new(user.id, "content1");

        {
            let tx = db.transaction()?;
            let _post = tx.save(post)?;
            let _post1 = tx.save(post1)?;
        }

        // queries are defined and prepared into statements
        // at startup in the db! macro
        let posts = db.posts_by_user_id(user.id)?;
        let found_user = db.user_by_id(user.id)?;
        assert_eq!(posts.len(), 2);
        assert_eq!(found_user.id, user.id);
        Ok(())
    }
}

pub struct Transaction<'a>(pub sqltight_core::Transaction<'a>);

impl<'a> Transaction<'a> {
    pub fn save<T: sqltight::Crud>(&self, row: T) -> Result<T> {
        row.save(&self.0)
    }

    pub fn delete<T: sqltight::Crud>(&self, row: T) -> Result<T> {
        row.delete(&self.0)
    }
}
