use std::path::PathBuf;

use bitcask::models::{BitCaskHandler, BitCaskHandlerOpenOpts};

fn main() {
    let handler = BitCaskHandler::open(
        PathBuf::from("test_bitcask_dir"),
        BitCaskHandlerOpenOpts {
            max_file_size_in_bytes: 1000,
            mode: bitcask::models::BitCaskHandlerOpenMode::Write,
        },
    );
}
