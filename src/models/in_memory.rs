use std::{ffi::OsString, path::Path};

use crate::models::disk_row::BitCaskDiskRow;

#[derive(Debug, Clone)]
pub struct BitCaskInMemoryValue {
    pub(crate) file_id: OsString,
    pub(crate) value_size: u64,
    pub(crate) value_offset: u64,
    #[allow(dead_code)]
    pub(crate) timestamp: u128,
}

impl BitCaskInMemoryValue {
    pub(crate) fn from_disk_entry(
        disk_entry: &BitCaskDiskRow,
        file_path: &Path,
        value_offset: u64,
    ) -> Self {
        Self {
            file_id: file_path.canonicalize().unwrap().into_os_string(),
            value_size: disk_entry.value_size,
            value_offset,
            timestamp: disk_entry.timestamp,
        }
    }
}
