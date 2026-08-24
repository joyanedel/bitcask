use std::{
    fs::File,
    io::Write,
    os::linux::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub(crate) struct DataFile {
    pub(crate) file_path: PathBuf,
}

impl AsRef<Path> for DataFile {
    fn as_ref(&self) -> &Path {
        &self.file_path
    }
}

impl From<PathBuf> for DataFile {
    fn from(value: PathBuf) -> Self {
        Self { file_path: value }
    }
}

#[derive(Debug)]
pub struct ActiveDataFile {
    pub(crate) file_path: PathBuf,
    pub(crate) current_file_size: u64,
    pub(crate) file_descriptor: File,
}

impl ActiveDataFile {
    pub(crate) fn new(directory: &Path) -> std::io::Result<Self> {
        let active_filepath = _get_default_new_data_entry(directory);
        Ok(Self {
            file_path: active_filepath.clone(),
            current_file_size: 0,
            file_descriptor: _get_active_file_fd(&active_filepath)?,
        })
    }

    pub(crate) fn write_and_flush(&mut self, data: &[u8]) -> std::io::Result<u64> {
        let written_bytes = self
            .file_descriptor
            .write(data)
            .map(|x| u64::try_from(x).expect("couldn't parse usize as u64"))?;
        self.file_descriptor.flush()?;
        self.current_file_size += written_bytes;
        Ok(written_bytes)
    }
}

pub(crate) fn get_all_segments(directory: &Path) -> std::io::Result<Vec<DataFile>> {
    let data_entries = _get_all_data_entries_in_dir(directory)?;
    let inactive_data_files = data_entries
        .iter()
        .map(PathBuf::to_owned)
        .map(DataFile::from)
        .collect();
    Ok(inactive_data_files)
}

fn _get_active_file_fd(active_file_path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::options()
        .read(true)
        .append(true)
        .create(true)
        .open(active_file_path)
}

/// Get all entries in a directory that are data files (e.g ends with .bc.data) sorted by creation time
fn _get_all_data_entries_in_dir(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
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
fn _get_default_new_data_entry(directory: &Path) -> PathBuf {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut path = directory.to_path_buf();
    path.push(format!("{current_time}.bc.data"));
    path
}

/// Determines if file is considered active
fn _is_file_active(entry: &Path, max_file_size_in_bytes: u64) -> bool {
    let metadata = entry
        .metadata()
        .expect("couldn't read last valid active file metadata");

    metadata.st_size() <= max_file_size_in_bytes
}
