use sqltight_ffi::{
    SQLITE_DONE, SQLITE_OK, SQLITE_ROW, sqlite3, sqlite3_bind_blob, sqlite3_bind_double,
    sqlite3_bind_int64, sqlite3_bind_null, sqlite3_bind_parameter_count,
    sqlite3_bind_parameter_name, sqlite3_bind_text, sqlite3_changes, sqlite3_close,
    sqlite3_column_bytes, sqlite3_column_count, sqlite3_column_decltype, sqlite3_column_double,
    sqlite3_column_int64, sqlite3_column_name, sqlite3_column_text, sqlite3_column_type,
    sqlite3_errmsg, sqlite3_exec, sqlite3_finalize, sqlite3_open, sqlite3_prepare_v2, sqlite3_step,
    sqlite3_stmt,
};

use std::{
    collections::BTreeMap,
    ffi::{CStr, CString, NulError, c_char, c_int},
    num::TryFromIntError,
    ops::Deref,
    str::Utf8Error,
};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Null(NulError),
    TryFromInt(TryFromIntError),
    Sqlite { text: String, code: i32 },
    FailedToPrepare,
    UniqueConstraint(String),
    ConnectionClosed,
    RowNotFound,
    Utf8Error(Utf8Error),
    DuplicateColumnName(String),
    MutexLockFailed,
}

pub type Result<T> = std::result::Result<T, Error>;
type Row = BTreeMap<String, Value>;

#[derive(Debug, Clone)]
pub struct Sqlite {
    db: *mut sqlite3,
}

impl Sqlite {
    pub fn open(path: &str) -> Result<Self> {
        let c_path = CString::new(path)?;
        let mut db: *mut sqlite3 = core::ptr::null_mut();
        let result = unsafe { sqlite3_open(c_path.as_ptr(), &mut db) };
        match result {
            SQLITE_OK => Ok(Self { db }),
            code => Err(sqlite_err(code, db)),
        }
    }

    pub fn prepare(&self, sql: &str) -> Result<Stmt> {
        let stmt = Stmt::prepare(self.db, sql, core::ptr::null_mut())?;
        Ok(stmt)
    }

    pub fn execute(&self, sql: &str) -> Result<i32> {
        let c_sql = CString::new(sql)?;
        let result = unsafe {
            sqlite3_exec(
                self.db,
                c_sql.as_ptr(),
                None,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        match result {
            SQLITE_OK => Ok(0),
            code => Err(sqlite_err(code, self.db)),
        }
    }

    pub fn transaction(&self) -> Result<Transaction<'_>> {
        Transaction::new(self, Tx::Immediate)
    }

    pub fn migrate(&self, migrations: &[impl ToString]) -> Result<()> {
        let tx = self.transaction()?;
        let _result =
            tx.execute("create table if not exists migrations (sql text unique not null) strict")?;
        for sql in migrations {
            let result = tx.execute(&sql.to_string());
            let _result = match result {
                Ok(result) => result,
                Err(Error::DuplicateColumnName(_)) => 0,
                Err(err) => return Err(err),
            };
            let text = Value::Text(Some(sql.to_string()));
            let _result = tx
                .prepare("insert into migrations (sql) values (:sql) on conflict (sql) do update set sql = excluded.sql")?
                .bind(&[text])?
                .changes()?;
        }

        Ok(())
    }
}

impl Drop for Sqlite {
    fn drop(&mut self) {
        unsafe {
            sqlite3_close(self.db);
        }
    }
}

#[derive(Clone, Copy)]
pub struct Stmt {
    stmt: *mut sqlite3_stmt,
    db: *mut sqlite3,
}

impl Stmt {
    pub(crate) fn prepare(
        db: *mut sqlite3,
        sql: &str,
        mut stmt: *mut sqlite3_stmt,
    ) -> Result<Self> {
        let c_sql = CString::new(sql)?;
        let result =
            unsafe { sqlite3_prepare_v2(db, c_sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut()) };
        match result {
            SQLITE_OK | SQLITE_ROW | SQLITE_DONE => Ok(Self { db, stmt }),
            code => Err(sqlite_err(code, db)),
        }
    }

    fn step(&self) -> Result<i32> {
        let result = unsafe { sqlite3_step(self.stmt) };
        match result {
            SQLITE_OK => Ok(SQLITE_OK),
            SQLITE_ROW => Ok(SQLITE_ROW),
            SQLITE_DONE => Ok(SQLITE_DONE),
            code => Err(sqlite_err(code, self.db)),
        }
    }

    fn finalize(&self) -> Result<()> {
        let result = unsafe { sqlite3_finalize(self.stmt) };
        match result {
            SQLITE_OK | SQLITE_ROW | SQLITE_DONE => Ok(()),
            code => Err(sqlite_err(code, self.db)),
        }
    }

    pub fn bind(self, params: &[Value]) -> Result<Self> {
        params
            .iter()
            .enumerate()
            .for_each(|(ix, param)| match param {
                Value::Text(Some(val)) => unsafe {
                    sqlite3_bind_text(
                        self.stmt,
                        (ix + 1) as i32,
                        val.as_ptr() as *const _,
                        val.len() as c_int,
                        None,
                    );
                },
                Value::Integer(Some(n)) => unsafe {
                    sqlite3_bind_int64(self.stmt, (ix + 1) as i32, *n);
                },
                Value::Real(Some(f)) => unsafe {
                    sqlite3_bind_double(self.stmt, (ix + 1) as i32, *f);
                },
                Value::Blob(Some(b)) => {
                    unsafe {
                        sqlite3_bind_blob(
                            self.stmt,
                            (ix + 1) as i32,
                            b.as_ptr() as *const _,
                            b.len() as c_int,
                            None,
                        )
                    };
                }
                Value::Text(None)
                | Value::Integer(None)
                | Value::Real(None)
                | Value::Blob(None)
                | Value::Null => {
                    unsafe { sqlite3_bind_null(self.stmt, (ix + 1) as i32) };
                }
            });

        Ok(self)
    }

    fn column_count(&self) -> i32 {
        unsafe { sqlite3_column_count(self.stmt) }
    }

    fn column_name(&self, i: i32) -> String {
        let result = unsafe { CStr::from_ptr(sqlite3_column_name(self.stmt, i)) };
        result.to_string_lossy().into_owned()
    }

    fn column_value(&self, i: i32) -> Value {
        let result = unsafe { sqlite3_column_type(self.stmt, i) };
        match result {
            1 => Value::Integer(Some(unsafe { sqlite3_column_int64(self.stmt, i) })),
            2 => Value::Real(Some(unsafe { sqlite3_column_double(self.stmt, i) })),
            3 => {
                let result =
                    unsafe { CStr::from_ptr(sqlite3_column_text(self.stmt, i) as *const c_char) };
                let text = result.to_string_lossy().into_owned();
                Value::Text(Some(text))
            }
            4 => {
                let slice = unsafe {
                    let len = sqlite3_column_bytes(self.stmt, i) as usize;
                    let ptr = sqlite3_column_text(self.stmt, i);
                    std::slice::from_raw_parts(ptr, len)
                };
                Value::Blob(Some(slice.to_vec()))
            }
            _ => Value::Null,
        }
    }

    pub fn rows(&self) -> Result<Vec<Row>> {
        let mut rows = Vec::new();
        while let Ok(sqlite_row) = self.step()
            && sqlite_row == SQLITE_ROW
        {
            let column_count = self.column_count();
            let mut values: BTreeMap<String, Value> = BTreeMap::new();
            for i in 0..column_count {
                let name = self.column_name(i);
                let value = self.column_value(i);
                values.insert(name, value);
            }
            rows.push(values);
        }
        let _result = self.finalize()?;
        Ok(rows)
    }

    pub fn changes(&self) -> Result<i32> {
        while let Ok(result) = self.step()
            && (result != SQLITE_ROW || result != SQLITE_DONE)
        {}
        self.finalize()?;
        let changes = unsafe { sqlite3_changes(self.db) };
        Ok(changes)
    }

    pub fn parameter_names(&self) -> Vec<String> {
        let mut names = vec![];
        let parameter_count = unsafe { sqlite3_bind_parameter_count(self.stmt) };
        for i in 1..=parameter_count {
            let name = unsafe { CStr::from_ptr(sqlite3_bind_parameter_name(self.stmt, i)) };
            let name = name.to_string_lossy().to_string();
            names.push(name);
        }
        names
    }

    pub fn select_column_names(&self) -> Vec<String> {
        let mut names = vec![];
        let column_count = unsafe { sqlite3_column_count(self.stmt) };
        for i in 0..column_count {
            let name = unsafe { CStr::from_ptr(sqlite3_column_name(self.stmt, i)) };
            let name = name.to_string_lossy().to_string();
            names.push(name);
        }
        names
    }

    pub fn select_column_types(&self) -> Vec<String> {
        let mut types = vec![];
        let column_count = unsafe { sqlite3_column_count(self.stmt) };
        for i in 0..column_count {
            let datatype = unsafe { CStr::from_ptr(sqlite3_column_decltype(self.stmt, i)) };
            let datatype = datatype.to_string_lossy().into_owned();
            types.push(datatype);
        }
        types
    }
}

#[derive(Debug)]
pub struct Transaction<'a> {
    sqlite: &'a Sqlite,
}

#[derive(Default)]
pub enum Tx {
    #[default]
    Deferred,
    Immediate,
    Exclusive,
}

impl<'a> Transaction<'a> {
    pub fn new(sqlite: &'a Sqlite, tx: Tx) -> Result<Transaction<'a>> {
        let sql = match tx {
            Tx::Deferred => "begin deferred transaction",
            Tx::Immediate => "begin immediate transaction",
            Tx::Exclusive => "begin exclusive transaction",
        };
        let _stmt = sqlite.execute(&sql)?;
        Ok(Self { sqlite })
    }

    pub fn end(&self) -> Result<i32> {
        self.execute("end transaction")
    }

    pub fn rollback(&self) -> Result<i32> {
        self.execute("rollback transaction")
    }
}

impl<'a> Deref for Transaction<'a> {
    type Target = Sqlite;

    fn deref(&self) -> &Self::Target {
        self.sqlite
    }
}

impl<'a> Drop for Transaction<'a> {
    fn drop(&mut self) {
        match self.end() {
            Ok(_) => {}
            Err(_err) => {
                self.rollback().expect("Rollback failed");
            }
        }
    }
}

fn sqlite_err(code: i32, db: *mut sqlite3) -> Error {
    match db.is_null() {
        true => Error::Sqlite {
            text: "The sqlite db pointer is null".into(),
            code: -1,
        },
        false => {
            let text = unsafe { CStr::from_ptr(sqlite3_errmsg(db)) }
                .to_string_lossy()
                .into_owned();
            if text.starts_with("UNIQUE constraint failed: ") {
                return Error::UniqueConstraint(text.replace("UNIQUE constraint failed: ", ""));
            } else if text.starts_with("duplicate column name: ") {
                return Error::DuplicateColumnName(text.replace("duplicate column name: ", ""));
            } else {
                return Error::Sqlite { text, code };
            }
        }
    }
}

impl From<NulError> for Error {
    fn from(value: NulError) -> Self {
        Self::Null(value)
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Text(Option<String>),
    Integer(Option<i64>),
    Real(Option<f64>),
    Blob(Option<Vec<u8>>),
    Null,
}

impl From<Option<String>> for Value {
    fn from(value: Option<String>) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Text(Some(value.to_string()))
    }
}

impl From<Option<i64>> for Value {
    fn from(value: Option<i64>) -> Self {
        Self::Integer(value)
    }
}

impl From<Option<Vec<u8>>> for Value {
    fn from(value: Option<Vec<u8>>) -> Self {
        Self::Blob(value)
    }
}

impl From<Option<f64>> for Value {
    fn from(value: Option<f64>) -> Self {
        Self::Real(value)
    }
}

impl From<&Value> for Option<i64> {
    fn from(value: &Value) -> Self {
        match value {
            Value::Text(_) => todo!(),
            Value::Integer(val) => val.clone(),
            Value::Real(_) => todo!(),
            Value::Blob(_items) => todo!(),
            Value::Null => None,
        }
    }
}

impl From<&Value> for Option<String> {
    fn from(value: &Value) -> Self {
        match value {
            Value::Text(val) => val.clone(),
            Value::Integer(_) => todo!(),
            Value::Real(_) => todo!(),
            Value::Blob(_items) => todo!(),
            Value::Null => None,
        }
    }
}
