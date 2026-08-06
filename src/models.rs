use std::{
    collections::HashMap,
    ffi::OsString,
    io::Write,
    marker::PhantomData,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bytes::BufMut;

type BaseSize = u64;

pub struct BitCaskHandlerOpen;
pub struct BitCaskHandlerClosed;

pub struct BitCaskHandler<State = BitCaskHandlerClosed> {
    hashmap: HashMap<bytes::Bytes, BitCaskInMemoryValue>,
    active_file: std::fs::File,
    active_filename: OsString,
    current_active_file_size: BaseSize,
    state: PhantomData<State>,
}

pub enum BitCaskHandlerOpenMode {
    Read = 0,
    Write = 1,
}

pub struct BitCaskHandlerOpenOpts {
    pub max_file_size_in_bytes: BaseSize,
    pub mode: BitCaskHandlerOpenMode,
}

pub struct BitCaskInMemoryValue {
    file_id: OsString,
    value_size: BaseSize,
    value_position: BaseSize,
    timestamp: u128,
}

pub struct BitCaskDiskRow {
    crc: u32,
    timestamp: u128,
    key_size: BaseSize,
    value_size: BaseSize,
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
        let active_file_path = get_active_file_from_directory(&directory_path)?;

        // create if does not exist
        let mut open_opts = std::fs::OpenOptions::new();
        let f = open_opts
            .read(true)
            .append(true)
            .create(true)
            .open(active_file_path.clone())
            .unwrap();
        let active_filename = active_file_path.file_name().unwrap().to_os_string();
        let active_file_size_in_bytes = f.metadata().unwrap().len();

        let hashmap = HashMap::new();
        let handler: BitCaskHandler<BitCaskHandlerOpen> = BitCaskHandler {
            hashmap,
            active_file: f,
            active_filename,
            current_active_file_size: active_file_size_in_bytes as BaseSize,
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
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let key_size = key.len() as BaseSize;
        let value_size = value.len() as BaseSize;

        let mut payload = bytes::BytesMut::new();
        payload.put_u128(timestamp);
        payload.put_u64(key_size);
        payload.put_u64(value_size);
        payload.put_slice(&key);
        payload.put_slice(&value);

        let crc = calculate_crc32(payload.into());

        let disk_value_row = BitCaskDiskRow {
            crc,
            timestamp,
            key_size,
            value_size,
            key: key.clone(),
            value: value.clone(),
        };

        let value_position = self.write_row_on_disk(&disk_value_row).unwrap();
        let _ = self.write_value_in_memory(key, value.len() as BaseSize, value_position, timestamp);

        Ok(())
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

impl BitCaskHandler<BitCaskHandlerOpen> {
    fn write_row_on_disk(&mut self, value: &BitCaskDiskRow) -> Result<BaseSize, std::io::Error> {
        let mut buffer = bytes::BytesMut::new();
        buffer.put_u32(value.crc);
        buffer.put_u128(value.timestamp);
        buffer.put_u64(value.key_size);
        buffer.put_u64(value.value_size);
        buffer.put_slice(&value.key);
        buffer.put_slice(&value.value);
        let written_bytes = self.active_file.write(&buffer).unwrap();
        self.active_file.flush().unwrap();

        self.current_active_file_size += written_bytes as BaseSize;
        Ok(self.current_active_file_size as BaseSize)
    }

    fn write_value_in_memory(
        &mut self,
        key: bytes::Bytes,
        value_length: BaseSize,
        value_position: BaseSize,
        timestamp: u128,
    ) -> Result<(), std::io::Error> {
        let file_id = self.active_filename.clone();
        let in_memory_value = BitCaskInMemoryValue {
            file_id,
            value_size: value_length,
            value_position,
            timestamp,
        };

        self.hashmap.insert(key, in_memory_value);

        Ok(())
    }
}

fn get_active_file_from_directory(directory: &Path) -> Result<PathBuf, BitCaskHandlerOpenError> {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
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
    Ok(active_file_path)
}

fn calculate_crc32(data: bytes::Bytes) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;

    for i in 0..data.len() {
        crc ^= unsafe { *data.get_unchecked(i) } as u32;

        for _ in 0..8 {
            crc = if crc ^ 1 == 1 {
                (crc >> 1) ^ 0xEDB88320
            } else {
                crc >> 1
            }
        }
    }

    !crc
}
