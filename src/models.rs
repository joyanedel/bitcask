use std::{
    collections::HashMap,
    ffi::OsString,
    fs::OpenOptions,
    io::{Read, Seek, Write},
    marker::PhantomData,
    os::linux::fs::MetadataExt,
    path::{Path, PathBuf},
    process::ExitStatus,
    time::{SystemTime, UNIX_EPOCH},
};

use bytes::{Buf, BufMut};

type BaseSize = u64;

pub struct BitCaskHandlerOpen;
pub struct BitCaskHandlerClosed;

pub struct BitCaskHandler<State = BitCaskHandlerClosed> {
    hashmap: HashMap<Box<[u8]>, BitCaskInMemoryValue>,
    directory: PathBuf,
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

#[derive(Debug)]
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
        let data_entries = get_all_data_entries_in_dir(&directory_path);
        let active_file_path = match data_entries.as_slice() {
            [.., target_file] if is_file_active(target_file, &opts) => target_file.clone(),
            _ => get_default_data_entry(&directory_path),
        };

        let hashmap = populate_in_memory_table_data(data_entries.as_slice());

        // create if does not exist
        let mut open_opts = std::fs::OpenOptions::new();
        let f = open_opts
            .read(true)
            .append(true)
            .create(true)
            .open(active_file_path.clone())
            .unwrap();
        let active_filename = active_file_path.canonicalize().unwrap().into_os_string();
        let active_file_size_in_bytes = f.metadata().unwrap().len();

        let handler: BitCaskHandler<BitCaskHandlerOpen> = BitCaskHandler {
            hashmap,
            directory: directory_path.clone(),
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
        self.sync()?;
        Ok(BitCaskHandler {
            hashmap: self.hashmap,
            directory: self.directory,
            active_file: self.active_file,
            active_filename: self.active_filename,
            current_active_file_size: self.current_active_file_size,
            state: PhantomData,
        })
    }

    pub fn get(&self, key: &[u8]) -> Option<bytes::Bytes> {
        let in_memory_ref = self.hashmap.get(key)?;
        let mut f = OpenOptions::new()
            .read(true)
            .create(false)
            .open(in_memory_ref.file_id.clone())
            .expect("referenced file does not exist");
        let _ = f.seek(std::io::SeekFrom::Start(in_memory_ref.value_position));
        let mut buffer = vec![0; in_memory_ref.value_size as usize];
        let r = f.read_exact(&mut buffer);
        Some(buffer.into())
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), std::io::Error> {
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
        payload.put_slice(key);
        payload.put_slice(value);

        let crc = calculate_crc32(payload.into());

        let disk_value_row = BitCaskDiskRow {
            crc,
            timestamp,
            key_size,
            value_size,
            key: bytes::Bytes::copy_from_slice(key),
            value: bytes::Bytes::copy_from_slice(value),
        };

        let value_position = self.write_row_on_disk(&disk_value_row).unwrap();
        let _ = self.write_value_in_memory(key, value.len() as BaseSize, value_position, timestamp);

        Ok(())
    }

    pub fn delete(&mut self, key: bytes::Bytes) -> Result<(), std::io::Error> {
        todo!("implement delete method")
    }

    pub fn list_keys(&self) -> Vec<bytes::Bytes> {
        self.hashmap
            .keys()
            .map(|x| bytes::Bytes::copy_from_slice(x))
            .collect()
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
        Ok(self.current_active_file_size as BaseSize - value.value_size)
    }

    fn write_value_in_memory(
        &mut self,
        key: &[u8],
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

        self.hashmap.insert(Box::from(key), in_memory_value);

        Ok(())
    }
}

impl TryFrom<&mut bytes::BytesMut> for BitCaskDiskRow {
    type Error = ();

    fn try_from(value: &mut bytes::BytesMut) -> Result<Self, Self::Error> {
        let crc = value.try_get_u32().unwrap();
        let timestamp = value.try_get_u128().unwrap();
        let key_size = value.try_get_u64().unwrap();
        let value_size = value.try_get_u64().unwrap();
        let key = value.copy_to_bytes(key_size as usize);
        let value_bytes = value.copy_to_bytes(value_size as usize);

        Ok(Self {
            crc,
            timestamp,
            key_size,
            value_size,
            key,
            value: value_bytes,
        })
    }
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

/// Get all entries in a directory that are data files (e.g ends with .bc.data) sorted by creation time
fn get_all_data_entries_in_dir(directory: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(directory).expect("couldn't read directory");
    let mut entries: Vec<_> = entries
        .filter_map(|x| x.ok())
        .filter(|x| {
            x.path().is_file() && x.path().to_str().is_some_and(|x| x.ends_with(".bc.data"))
        })
        .map(|x| x.path())
        .collect();

    entries.sort_by(|a, b| {
        a.metadata()
            .unwrap()
            .created()
            .unwrap()
            .cmp(&b.metadata().unwrap().created().unwrap())
    });
    entries
}

/// Returns a pathbuf with default filename (current system time in milliseconds)
fn get_default_data_entry(directory: &Path) -> PathBuf {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let mut path = directory.to_path_buf();
    path.push(format!("{}.bc.data", current_time));
    path
}

/// Determines if file is considered active
fn is_file_active(entry: &Path, bitcask_open_opts: &BitCaskHandlerOpenOpts) -> bool {
    let metadata = entry
        .metadata()
        .expect("couldn't read last valid active file metadata");

    metadata.st_size() <= bitcask_open_opts.max_file_size_in_bytes
}

/// Populates an in-memory table with bitcask data value hints
fn populate_in_memory_table_data(
    data_entries: &[PathBuf],
) -> HashMap<Box<[u8]>, BitCaskInMemoryValue> {
    let mut hash_map = HashMap::new();

    for data_entry in data_entries {
        populate_in_memory_hash_map_with_file_data(&mut hash_map, data_entry);
    }

    hash_map
}

/// Populate hash map with data in entry
fn populate_in_memory_hash_map_with_file_data(
    hash_map: &mut HashMap<Box<[u8]>, BitCaskInMemoryValue>,
    file_path: &Path,
) {
    let mut current_read_position = 0;
    let mut buffer =
        bytes::Bytes::from(std::fs::read(file_path).expect("couldn't read immutable data file"))
            .try_into_mut()
            .unwrap_or_default();
    while !buffer.is_empty() {
        let previous_buffer_size = buffer.len();
        let disk_row =
            BitCaskDiskRow::try_from(&mut buffer).expect("couldn't read data from file. corrupted");
        let current_buffer_size = buffer.len();
        current_read_position += previous_buffer_size - current_buffer_size;

        let value_offset = current_read_position - disk_row.value_size as usize;
        let in_memory_val = BitCaskInMemoryValue {
            file_id: file_path.canonicalize().unwrap().as_os_str().to_os_string(),
            value_size: disk_row.value_size,
            value_position: value_offset as u64,
            timestamp: disk_row.timestamp,
        };
        hash_map.insert(disk_row.key.to_vec().into_boxed_slice(), in_memory_val);
    }
}
