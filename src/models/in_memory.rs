use std::ffi::OsString;

#[derive(Debug)]
pub struct BitCaskInMemoryValue {
    pub(crate) file_id: OsString,
    pub(crate) value_size: u64,
    pub(crate) value_offset: u64,
    #[allow(dead_code)]
    pub(crate) timestamp: u128,
}
