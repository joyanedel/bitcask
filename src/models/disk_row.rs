use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use bytes::{Buf, BufMut, BytesMut};

use crate::models::in_memory::BitCaskInMemoryValue;

pub struct BitCaskDiskRow {
    pub(crate) crc: u32,
    pub(crate) timestamp: u128,
    pub(crate) key_size: u64,
    pub(crate) value_size: u64,
    pub(crate) key: bytes::Bytes,
    pub(crate) value: bytes::Bytes,
}

#[derive(Debug)]
pub(crate) struct DataHintEntry {
    timestamp: u128,
    key_size: u64,
    value_size: u64,
    value_offset: u64,
    pub(crate) key: bytes::Bytes,
}

impl BitCaskDiskRow {
    #[must_use]
    pub(crate) fn new(key: &[u8], value: &[u8]) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let key_size = key.len() as u64;
        let value_size = value.len() as u64;

        let crc_payload = [
            timestamp.to_be_bytes().as_slice(),
            key_size.to_be_bytes().as_slice(),
            value_size.to_be_bytes().as_slice(),
            key,
            value,
        ]
        .concat();
        let crc = calculate_crc32(&crc_payload.into());
        Self {
            crc,
            timestamp,
            key: bytes::Bytes::copy_from_slice(key),
            key_size,
            value: bytes::Bytes::copy_from_slice(value),
            value_size,
        }
    }

    #[must_use]
    /// Serialize struct into bytes
    pub(crate) fn to_bytes(&self) -> bytes::Bytes {
        let mut buf = BytesMut::with_capacity(
            size_of::<u32>()
                + size_of::<u128>()
                + 2 * size_of::<u64>()
                + self.key_size as usize
                + self.value_size as usize,
        );
        buf.put_u32(self.crc);
        buf.put_u128(self.timestamp);
        buf.put_u64(self.key_size);
        buf.put_u64(self.value_size);
        buf.put_slice(&self.key);
        buf.put_slice(&self.value);
        buf.freeze()
    }
}

impl TryFrom<&mut bytes::BytesMut> for BitCaskDiskRow {
    type Error = ();

    fn try_from(value: &mut bytes::BytesMut) -> Result<Self, Self::Error> {
        let crc = value.try_get_u32().unwrap();
        let timestamp = value.try_get_u128().unwrap();
        let key_size = value.try_get_u64().unwrap();
        let value_size = value.try_get_u64().unwrap();
        let key = value
            .copy_to_bytes(usize::try_from(key_size).expect("couldn't parse key size as usize"));
        let value_bytes = value.copy_to_bytes(
            usize::try_from(value_size).expect("couldn't parse value size as usize"),
        );

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

impl DataHintEntry {
    pub(crate) fn from_disk_entry(disk_entry: &BitCaskDiskRow, value_offset: u64) -> Self {
        Self {
            timestamp: disk_entry.timestamp,
            key_size: disk_entry.key_size,
            value_size: disk_entry.value_size,
            value_offset,
            key: disk_entry.key.clone(),
        }
    }

    pub(crate) fn to_bytes(&self) -> bytes::Bytes {
        let mut buf = bytes::BytesMut::with_capacity(
            size_of::<u128>() + 3 * size_of::<u64>() + self.key_size as usize,
        );
        buf.put_u128(self.timestamp);
        buf.put_u64(self.key_size);
        buf.put_u64(self.value_size);
        buf.put_u64(self.value_offset);
        buf.put_slice(&self.key);
        buf.freeze()
    }

    pub(crate) fn to_in_memory_entry(&self, file_id: &Path) -> BitCaskInMemoryValue {
        BitCaskInMemoryValue {
            file_id: file_id.canonicalize().unwrap().into_os_string(),
            value_size: self.value_size,
            value_offset: self.value_offset,
            timestamp: self.timestamp,
        }
    }
}

impl TryFrom<&mut bytes::BytesMut> for DataHintEntry {
    type Error = ();
    fn try_from(value: &mut bytes::BytesMut) -> Result<Self, Self::Error> {
        let timestamp = value.try_get_u128().unwrap();
        let key_size = value.try_get_u64().unwrap();
        let value_size = value.try_get_u64().unwrap();
        let value_offset = value.try_get_u64().unwrap();
        let key =
            value.copy_to_bytes(usize::try_from(key_size).expect("couldn't parse usize as u64"));

        Ok(Self {
            timestamp,
            key_size,
            value_size,
            value_offset,
            key,
        })
    }
}

fn calculate_crc32(data: &bytes::Bytes) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;

    for i in 0..data.len() {
        crc ^= u32::from(unsafe { *data.get_unchecked(i) });

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
