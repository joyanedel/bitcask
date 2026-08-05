use std::{
    collections::HashMap,
    ffi::OsString,
    marker::PhantomData,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct BitCaskHandlerOpen;
pub struct BitCaskHandlerClosed;

pub struct BitCaskHandler<State = BitCaskHandlerClosed> {
    hashmap: HashMap<bytes::Bytes, BitCaskInMemoryValue>,
    active_file: std::fs::File,
    current_active_file_size: u64,
    state: PhantomData<State>,
}

pub enum BitCaskHandlerOpenMode {
    Read = 0,
    Write = 1,
}

pub struct BitCaskHandlerOpenOpts {
    pub max_file_size_in_bytes: usize,
    pub mode: BitCaskHandlerOpenMode,
}

pub struct BitCaskInMemoryValue {
    file_id: OsString,
    value_size: u32,
    value_position: u32,
    timestamp: u32,
}

pub struct BitCaskDiskRow {
    crc: u32,
    timestamp: u32,
    key_size: u32,
    value_size: u32,
    key: bytes::Bytes,
    value: bytes::Bytes,
}

#[derive(Debug, thiserror::Error)]
pub enum BitCaskHandlerOpenError {
    #[error("BitCask directory not found")]
    DirectoryNotFound,
    #[error("Directory/File with insufficient permissions")]
    PermissionError,
}

impl BitCaskHandler<BitCaskHandlerClosed> {
    pub fn open(
        directory_path: PathBuf,
        opts: BitCaskHandlerOpenOpts,
    ) -> Result<BitCaskHandler<BitCaskHandlerOpen>, BitCaskHandlerOpenError> {
        let active_file_descriptor = get_active_file_from_directory(&directory_path)?;
        let active_file_size_in_bytes = active_file_descriptor.metadata().unwrap().len();
        let hashmap = HashMap::new();
        let handler: BitCaskHandler<BitCaskHandlerOpen> = BitCaskHandler {
            hashmap,
            active_file: active_file_descriptor,
            current_active_file_size: active_file_size_in_bytes,
            state: PhantomData,
        };
        Ok(handler)
    }
}

impl BitCaskHandler<BitCaskHandlerOpen> {
    pub fn close(self) -> Result<BitCaskHandler<BitCaskHandlerClosed>, std::io::Error> {
        todo!("implement close method")
    }

    pub fn get(&self, key: bytes::Bytes) -> Option<bytes::Bytes> {
        todo!("implement get method")
    }

    pub fn put(&mut self, key: bytes::Bytes, value: bytes::Bytes) -> Result<(), std::io::Error> {
        todo!("implement put method")
    }

    pub fn delete(&mut self, key: bytes::Bytes) -> Result<(), std::io::Error> {
        todo!("implement delete method")
    }

    pub fn list_keys(&self) -> Vec<bytes::Bytes> {
        todo!("implement list keys method")
    }

    pub fn sync(&self) -> Result<(), std::io::Error> {
        todo!("implement sync method")
    }
}

fn get_active_file_from_directory(
    directory: &Path,
) -> Result<std::fs::File, BitCaskHandlerOpenError> {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let dir_entries = directory.read_dir().unwrap();
    let active_file_path = dir_entries
        .filter_map(|v| {
            v.as_ref()
                .is_ok_and(|d| d.file_name().into_string().unwrap().ends_with(".bc"))
                .then(|| v.unwrap().path())
        })
        .next()
        .unwrap_or_else(|| {
            let mut dir = directory.to_path_buf();
            dir.push(format!("{}.bc", current_time));
            dir
        });

    // create if does not exist
    let mut open_opts = std::fs::OpenOptions::new();
    let f = open_opts
        .read(true)
        .append(true)
        .create(true)
        .open(active_file_path)
        .unwrap();
    Ok(f)
}
