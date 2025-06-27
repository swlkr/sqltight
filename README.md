# sqltight

Yet another sqlite library for rust

# Quickstart

```rust
use sqltight::{db, Result};

schema! {
  table User {
    id: Int,
    email: Text
  }

  index User {
    email: Unique
  }

  table Post {
    id: Int,
    user_id: Int,
    content: Text
  }
}

fn main() -> Result<()> {
  let (users, posts) = schema();
  let user = save!(User { email: "email".into() })?;
  let mut user1 = save!(User { email: "email2".into() })?;
  user1.email = "email3".into();
  let user1 = user1.save()?;
  let user1 = user1.delete()?;
  let post = save!(Post { content: "content".into() })?;
  let post1 = save!(Post { content: "content1".into() })?;
  let post_rows = posts.select_where(posts.user_id, is, user.id).rows()?;
  let user = users.select_where(users.id, is, user.id).rows()?;
  Ok(())
}
```
# Use

```sh
cargo add --git https://github.com/swlkr/sqltight
```

Happy hacking!

