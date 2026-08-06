use std::{io::Read, path::PathBuf};

use bitcask::models::{BitCaskHandler, BitCaskHandlerOpenOpts};

fn main() {
    let mut handler = BitCaskHandler::open(
        PathBuf::from("test_bitcask_dir"),
        BitCaskHandlerOpenOpts {
            max_file_size_in_bytes: 1000,
            mode: bitcask::models::BitCaskHandlerOpenMode::Write,
        },
    )
    .unwrap();

    let key = bytes::Bytes::from_static(b"hola");
    let value = bytes::Bytes::from_static(b"mundo");
    let _ = handler.put(key, value);
}
