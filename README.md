# sqltight

Zero dependency sqlite library for *nightly* rust

# Quickstart

```rust
use sqltight::{db, Result};

// schema is additive only, no drops
// create table is done with a required "id integer primary key" column
// after that it is only alter table, no migrations necessary
// all columns do not have defaults and are optional except the id, updated_at and created_at columns
db! {
  table User {
    id: Int,
    email: Text,
    created_at: Int
  }

  index User {
    email: Unique
  }

  table Post {
    id: Int,
    user_id: Int,
    content: Text,
    created_at: Int
  }

  // select statements are named and the return
  // type determines the columns selected
  select posts_by_user_id (
    Vec<Post>
    "where Post.user_id = :user_id
     order by created_at desc
     limit 2"
   )

  select user_by_id (
    User
    "where User.id = :user_id"
  )
}

fn main() -> Result<()> {
  let db = db();

  // upsert and delete are the only write functions
  let user = User::new("email");
  let user = db.save(user)?;

  let user1 = User { email: text("email2"), ..Default::default() };
  let mut user1 = db.save(user1)?;
  // sqlite types are explicit there is no implicit mapping between them
  user1.email = text("email3");
  let user1 = db.save(user1)?;
  let user1 = db.delete(user1)?;

  let post = Post { content: text("content"), user_id: user.id, ..Default::default() };
  let post1 = Post { content: text("content1"), user_id: user.id, ..Default::default() };
  {
    let tx = db.transaction()?;
    let post = tx.save(post)?;
    let post1 = tx.save(post1)?;
  }

  // queries are defined and prepared into statements
  // ahead of time in the db! macro
  let posts = db.posts_by_user_id(user.id)?;
  let user = db.user_by_id(user.id)?;

  Ok(())
}
```
# Use

```sh
cargo add --git https://github.com/swlkr/sqltight
```

Happy hacking!

