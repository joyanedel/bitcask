use std::collections::{HashMap, hash_map::IterMut};

use crate::models::in_memory::BitCaskInMemoryValue;

#[derive(Debug, Default)]
pub(crate) struct KeyDirectory {
    map: HashMap<Box<[u8]>, BitCaskInMemoryValue>,
}

impl KeyDirectory {
    pub(crate) fn get(&self, key: &[u8]) -> Option<&BitCaskInMemoryValue> {
        self.map.get(key)
    }

    pub(crate) fn iter_mut_entries(&mut self) -> IterMut<'_, Box<[u8]>, BitCaskInMemoryValue> {
        self.map.iter_mut()
    }

    pub(crate) fn put(&mut self, key: &[u8], value: BitCaskInMemoryValue) -> std::io::Result<()> {
        self.map.insert(key.to_vec().into_boxed_slice(), value);
        Ok(())
    }

    pub(crate) fn delete(&mut self, key: &[u8]) -> Option<BitCaskInMemoryValue> {
        self.map.remove(key)
    }

    pub(crate) fn keys(&self) -> Vec<&[u8]> {
        self.map.keys().map(|x| x.as_ref()).collect()
    }
}
