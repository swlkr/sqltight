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
  // type is created from the fn name at compile time
  query posts_by_user_id "
    select id
    from post
    where user_id = :user_id
    order by created_at desc
    limit 2
  "

  query user_by_id "
    select id, email, created_at
    from user
    where id = :user_id
  "
}

fn main() -> Result<()> {
  let db = Database::open(":memory:")?;

  // upsert (save) and delete are the only write functions
  let user = User::new("email");
  let user = db.save(user)?;

  let user1 = User::new("email1");
  let mut user1 = db.save(user1)?;

  // sqlite types are explicit there is no implicit mapping between them
  user1.email = text("email2");

  let user1 = db.save(user1)?;
  let user1 = db.delete(user1)?;

  let post = Post::new(user.id, "content");
  let post1 = Post::new(user.id, "content1");
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

# Tree Sitter Injection for SQL syntax highlighting

```scm
((macro_invocation
   macro:
     [
       (scoped_identifier
         name: (_) @_macro_name)
       (identifier) @_macro_name
     ]
   (token_tree
     (identifier)
     (string_literal
       (string_content) @injection.content)))
 (#eq? @_macro_name "db")
 (#set! injection.language "sql")
 (#set! injection.include-children))
```

Happy hacking!

