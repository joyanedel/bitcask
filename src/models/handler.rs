use std::{
    collections::HashMap,
    ffi::OsString,
    fs::OpenOptions,
    io::{Read, Seek, Write},
    os::linux::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bytes::BufMut;

use crate::models::{disk_row::BitCaskDiskRow, in_memory::BitCaskInMemoryValue};

pub struct BitCaskHandlerOpen;
pub struct BitCaskHandlerClosed;

pub struct BitCaskHandler<State = BitCaskHandlerClosed> {
    key_dir: HashMap<Box<[u8]>, BitCaskInMemoryValue>,
    directory: PathBuf,
    active_file: std::fs::File,
    active_filename: OsString,
    current_active_file_size: u64,
    _state: std::marker::PhantomData<State>,
}

pub enum BitCaskHandlerOpenMode {
    Read = 0,
    Write = 1,
}

pub struct BitCaskHandlerOpenOpts {
    pub max_file_size_in_bytes: u64,
    pub mode: BitCaskHandlerOpenMode,
}

#[derive(Debug, thiserror::Error)]
pub enum BitCaskHandlerOpenError {
    #[error("I/O error: {0}")]
    IOError(#[from] std::io::Error),
}

impl BitCaskHandler<BitCaskHandlerClosed> {
    pub fn open(
        directory_path: PathBuf,
        opts: BitCaskHandlerOpenOpts,
    ) -> Result<BitCaskHandler<BitCaskHandlerOpen>, BitCaskHandlerOpenError> {
        let data_entries = get_all_data_entries_in_dir(&directory_path)?;
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
            .open(active_file_path.clone())?;
        let active_filename = active_file_path.canonicalize()?.into_os_string();
        let active_file_size_in_bytes = f.metadata()?.len();

        let handler: BitCaskHandler<BitCaskHandlerOpen> = BitCaskHandler {
            key_dir: hashmap,
            directory: directory_path.clone(),
            active_file: f,
            active_filename,
            current_active_file_size: active_file_size_in_bytes as u64,
            _state: std::marker::PhantomData,
        };
        Ok(handler)
    }
}

impl BitCaskHandler<BitCaskHandlerOpen> {
    pub fn close(self) -> Result<BitCaskHandler<BitCaskHandlerClosed>, std::io::Error> {
        self.sync()?;
        Ok(BitCaskHandler {
            key_dir: self.key_dir,
            directory: self.directory,
            active_file: self.active_file,
            active_filename: self.active_filename,
            current_active_file_size: self.current_active_file_size,
            _state: std::marker::PhantomData,
        })
    }

    pub fn get(&self, key: &[u8]) -> Option<bytes::Bytes> {
        let in_memory_ref = self.key_dir.get(key)?;
        let mut f = OpenOptions::new()
            .read(true)
            .create(false)
            .open(in_memory_ref.file_id.clone())
            .expect("referenced file does not exist");
        let _ = f.seek(std::io::SeekFrom::Start(in_memory_ref.value_offset));
        let mut buffer = vec![0; in_memory_ref.value_size as usize];
        let _ = f.read_exact(&mut buffer);
        Some(buffer.into())
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), std::io::Error> {
        let disk_value_row = BitCaskDiskRow::new(key, value);

        let value_offset = self.write_row_on_disk(&disk_value_row).unwrap();
        let _ = self.write_value_in_memory(
            key,
            value.len() as u64,
            value_offset,
            disk_value_row.timestamp,
        );

        Ok(())
    }

    pub fn delete(&mut self, key: &[u8]) -> Result<(), std::io::Error> {
        if self
            .key_dir
            .remove(&key.to_vec().into_boxed_slice())
            .is_none()
        {
            return Ok(());
        }
        let disk_row_value = BitCaskDiskRow::new(key, b"\0");
        let _ = self.write_row_on_disk(&disk_row_value);
        Ok(())
    }

    pub fn list_keys(&self) -> Vec<bytes::Bytes> {
        self.key_dir
            .keys()
            .map(|x| bytes::Bytes::copy_from_slice(x))
            .collect()
    }

    pub fn sync(&self) -> Result<(), std::io::Error> {
        todo!("implement sync method")
    }
}

impl BitCaskHandler<BitCaskHandlerOpen> {
    fn write_row_on_disk(&mut self, value: &BitCaskDiskRow) -> Result<u64, std::io::Error> {
        let mut buffer = bytes::BytesMut::new();
        buffer.put_u32(value.crc);
        buffer.put_u128(value.timestamp);
        buffer.put_u64(value.key_size);
        buffer.put_u64(value.value_size);
        buffer.put_slice(&value.key);
        buffer.put_slice(&value.value);
        let written_bytes = self.active_file.write(&buffer).unwrap();
        self.active_file.flush().unwrap();

        self.current_active_file_size += written_bytes as u64;
        Ok(self.current_active_file_size - value.value_size)
    }

    fn write_value_in_memory(
        &mut self,
        key: &[u8],
        value_length: u64,
        value_offset: u64,
        timestamp: u128,
    ) -> Result<(), std::io::Error> {
        let file_id = self.active_filename.clone();
        let in_memory_value = BitCaskInMemoryValue {
            file_id,
            value_size: value_length,
            value_offset,
            timestamp,
        };

        self.key_dir.insert(Box::from(key), in_memory_value);

        Ok(())
    }
}

/// Get all entries in a directory that are data files (e.g ends with .bc.data) sorted by creation time
fn get_all_data_entries_in_dir(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(directory)?;
    let mut entries: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|x| {
            x.path().is_file() && x.path().to_str().is_some_and(|x| x.ends_with(".bc.data"))
        })
        .map(|x| x.path())
        .collect();

    entries
        .sort_by_key(|x| unsafe { x.metadata().unwrap_unchecked().created().unwrap_unchecked() });
    Ok(entries)
}

/// Returns a pathbuf with default filename (current system time in milliseconds)
fn get_default_data_entry(directory: &Path) -> PathBuf {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let mut path = directory.to_path_buf();
    path.push(format!("{current_time}.bc.data"));
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

        if disk_row.value == "\0" {
            hash_map.remove(&disk_row.key.to_vec().into_boxed_slice());
            continue;
        }

        let value_offset = current_read_position
            - usize::try_from(disk_row.value_size)
                .expect("couldn't obtain offset due to value size being truncated");
        let in_memory_val = BitCaskInMemoryValue {
            file_id: file_path.canonicalize().unwrap().as_os_str().to_os_string(),
            value_size: disk_row.value_size,
            value_offset: value_offset as u64,
            timestamp: disk_row.timestamp,
        };
        hash_map.insert(disk_row.key.to_vec().into_boxed_slice(), in_memory_val);
    }
}
