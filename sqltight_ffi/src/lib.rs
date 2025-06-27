#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

pub fn sqlite_version() -> String {
    unsafe {
        let version = sqlite3_libversion();
        std::ffi::CStr::from_ptr(version)
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        let version = sqlite_version();
        println!("SQLite version: {}", version);
        assert!(!version.is_empty());
    }
}
