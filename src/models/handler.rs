use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::{Read, Seek, Write},
    ops::BitOr,
    os::linux::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bytes::BufMut;

use crate::models::{
    disk_row::BitCaskDiskRow, in_memory::BitCaskInMemoryValue, key_dir::KeyDirectory,
};

pub struct BitCaskHandlerOpen;
pub struct BitCaskHandlerClosed;

pub struct BitCaskHandler<State = BitCaskHandlerClosed> {
    key_dir: KeyDirectory,
    directory: PathBuf,
    active_file: std::fs::File,
    active_filename: OsString,
    current_active_file_size: u64,
    _state: std::marker::PhantomData<State>,
    opts: BitCaskHandlerOpenOpts,
}

#[repr(transparent)]
#[derive(Debug, Clone)]
pub struct BitCaskHandlerOpenMode(u8);

impl BitOr for BitCaskHandlerOpenMode {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}
impl BitCaskHandlerOpenMode {
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);

    pub fn contains(&self, mode: Self) -> bool {
        (self.0 & mode.0) == mode.0
    }
}

pub struct BitCaskHandlerOpenOpts {
    pub max_file_size_in_bytes: u64,
    pub mode: BitCaskHandlerOpenMode,
    /// Indicates if at start time, system must check CRC of entries
    pub hintfile_checksum_strict: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum BitCaskHandlerOpenError {
    #[error("I/O error: {0}")]
    IOError(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum BitCaskHandlerPutError {
    #[error("I/O error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Write not allowed in mode: {0:?}")]
    MissingWritePermission(BitCaskHandlerOpenMode),
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
        let f = get_active_file_fd(&active_file_path)?;
        let active_filename = active_file_path.canonicalize()?.into_os_string();
        let active_file_size_in_bytes = f.metadata()?.len();

        let handler: BitCaskHandler<BitCaskHandlerOpen> = BitCaskHandler {
            key_dir: hashmap,
            directory: directory_path.clone(),
            active_file: f,
            active_filename,
            current_active_file_size: active_file_size_in_bytes as u64,
            _state: std::marker::PhantomData,
            opts,
        };
        Ok(handler)
    }
}

impl BitCaskHandler<BitCaskHandlerOpen> {
    pub fn close(self) -> Result<BitCaskHandler<BitCaskHandlerClosed>, std::io::Error> {
        if self.opts.mode.contains(BitCaskHandlerOpenMode::WRITE) {
            self.sync()?;
        }

        Ok(BitCaskHandler {
            key_dir: self.key_dir,
            directory: self.directory,
            active_file: self.active_file,
            active_filename: self.active_filename,
            current_active_file_size: self.current_active_file_size,
            _state: std::marker::PhantomData,
            opts: self.opts,
        })
    }

    #[must_use]
    pub fn get(&self, key: &[u8]) -> Option<bytes::Bytes> {
        let in_memory_ref = self.key_dir.get(key)?;
        let mut f = OpenOptions::new()
            .read(true)
            .create(false)
            .open(in_memory_ref.file_id.clone())
            .expect("referenced file does not exist");
        let _ = f.seek(std::io::SeekFrom::Start(in_memory_ref.value_offset));
        let mut buffer = vec![
            0;
            usize::try_from(in_memory_ref.value_size)
                .expect("couldn't parse value as usize")
        ];
        let _ = f.read_exact(&mut buffer);
        Some(buffer.into())
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), BitCaskHandlerPutError> {
        if !self.opts.mode.contains(BitCaskHandlerOpenMode::WRITE) {
            return Err(BitCaskHandlerPutError::MissingWritePermission(
                self.opts.mode.clone(),
            ));
        }
        let disk_value_row = BitCaskDiskRow::new(key, value);

        let value_offset = self.write_row_on_disk(&disk_value_row)?;
        self.write_value_in_memory(
            key,
            value.len() as u64,
            value_offset,
            disk_value_row.timestamp,
        );

        Ok(())
    }

    pub fn delete(&mut self, key: &[u8]) -> Result<(), std::io::Error> {
        if self.key_dir.delete(key).is_none() {
            return Ok(());
        }
        let mut disk_row_value = BitCaskDiskRow::new(key, b"");
        disk_row_value.timestamp = 0;
        let _ = self.write_row_on_disk(&disk_row_value);
        Ok(())
    }

    pub fn list_keys(&self) -> Vec<bytes::Bytes> {
        self.key_dir
            .keys()
            .iter()
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

        // to be written buffer exceeds file size?
        if self.current_active_file_size + buffer.len() as u64 > self.opts.max_file_size_in_bytes {
            // set new active file
            let active_filepath = get_default_data_entry(&self.directory);
            self.active_file = get_active_file_fd(&active_filepath)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            self.active_filename = active_filepath.canonicalize()?.into_os_string();
            self.current_active_file_size = 0;
        }

        let written_bytes = self.active_file.write(&buffer)?;
        self.active_file.flush()?;

        self.current_active_file_size += written_bytes as u64;
        Ok(self.current_active_file_size - value.value_size)
    }

    fn write_value_in_memory(
        &mut self,
        key: &[u8],
        value_length: u64,
        value_offset: u64,
        timestamp: u128,
    ) {
        let file_id = self.active_filename.clone();
        let in_memory_value = BitCaskInMemoryValue {
            file_id,
            value_size: value_length,
            value_offset,
            timestamp,
        };

        let _ = self.key_dir.put(key, in_memory_value);
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
        .as_nanos();
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
fn populate_in_memory_table_data(data_entries: &[PathBuf]) -> KeyDirectory {
    // let mut hash_map = HashMap::new();
    let mut key_dir = KeyDirectory::default();

    for data_entry in data_entries {
        populate_in_memory_hash_map_with_file_data(&mut key_dir, data_entry);
    }

    key_dir
}

/// Populate hash map with data in entry
fn populate_in_memory_hash_map_with_file_data(key_dir: &mut KeyDirectory, file_path: &Path) {
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

        if disk_row.timestamp == 0 {
            key_dir.delete(&disk_row.key);
            continue;
        }

        let value_offset = current_read_position
            - usize::try_from(disk_row.value_size)
                .expect("couldn't obtain offset due to value size being truncated");
        let in_memory_entry = BitCaskInMemoryValue::from_disk_entry(
            &disk_row,
            file_path,
            u64::try_from(value_offset).unwrap(),
        );
        let _ = key_dir.put(&disk_row.key, in_memory_entry);
    }
}

fn get_active_file_fd(active_file_path: &Path) -> Result<std::fs::File, BitCaskHandlerOpenError> {
    std::fs::File::options()
        .read(true)
        .append(true)
        .create(true)
        .open(active_file_path)
        .map_err(BitCaskHandlerOpenError::IOError)
}
