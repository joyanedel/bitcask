use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Buf;

pub struct BitCaskDiskRow {
    pub(crate) crc: u32,
    pub(crate) timestamp: u128,
    pub(crate) key_size: u64,
    pub(crate) value_size: u64,
    pub(crate) key: bytes::Bytes,
    pub(crate) value: bytes::Bytes,
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
