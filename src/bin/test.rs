use std::path::PathBuf;

use bitcask::models::handler::{BitCaskHandler, BitCaskHandlerOpenMode, BitCaskHandlerOpenOpts};

fn main() {
    #[allow(unused_mut)]
    let mut handler = BitCaskHandler::open(
        PathBuf::from("test_dir"),
        BitCaskHandlerOpenOpts {
            max_file_size_in_bytes: 1000,
            mode: BitCaskHandlerOpenMode::Write,
        },
    )
    .unwrap();

    // let _ = handler.put(b"hola", b"mundo");
    // let _ = handler.put(b"hello", b"world");
    // let val = handler.get(b"hola");
    // println!("{val:?}");
    // let keys = handler.list_keys();
    // keys.iter().inspect(|x| println!("-> {x:?}")).count();
    // let r = handler.delete(b"hola");
    // if let Err(e) = r {
    //     eprintln!("{e}");
    // }
    // let keys = handler.list_keys();
    // println!("--------------");
    // keys.iter().inspect(|x| println!("-> {x:?}")).count();
    for _ in 0..1000000 {
        let v = handler.get(b"dummy key with long string length");
        let k = handler.list_keys();

        std::hint::black_box(v);
        std::hint::black_box(k);
    }
}
