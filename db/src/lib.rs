#![allow(unused)]
use std::{
    io::{self, Write},
    iter,
};

use serde::Serialize;

type Result<T> = core::result::Result<T, Error>;

enum Error {
    Serialize(Box<dyn std::error::Error>),
    Io(io::Error),
}

const PAGE_SIZE: usize = 4096;
const ROW_SIZE: usize = 256;

struct Page {
    data: Vec<u8>,
}

impl Page {
    pub fn new() -> Self {
        Self {
            data: Vec::with_capacity(PAGE_SIZE),
        }
    }
    pub fn insert<S: Serialize>(&mut self, row: S) -> Result<()> {
        let serialized = bitcode::serialize(&row).map_err(|e| Error::Serialize(Box::new(e)))?;

        let size = serialized.len() as u64;
        let size = size.to_be_bytes();

        self.data.write(&size).map_err(|e| Error::Io(e))?;
        self.data.write(&serialized).map_err(|e| Error::Io(e))?;
        self.data
            .write(&vec![0; ROW_SIZE - (serialized.len() + size.len())])
            .map_err(|e| Error::Io(e))?;

        Ok(())
    }

    pub fn rows(&self) -> impl Iterator<Item = Vec<u8>> + '_ {
        let mut cursor = 0;
        iter::from_fn(move || {
            let offset = ROW_SIZE * cursor;
            if offset + ROW_SIZE > self.data.len() {
                return None;
            }

            let row = &self.data[offset..offset + ROW_SIZE];
            let size = {
                let mut buf = [0; 8];
                buf.copy_from_slice(&row[0..8]);
                u64::from_be_bytes(buf) as usize
            };

            cursor += 1;
            Some(row[8..8 + size].to_vec())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let mut page = Page::new();
        page.insert(String::from("sla1"));
        page.insert(String::from("sla2"));
        page.insert(String::from("sla3"));
        page.insert(9090 as u64);

        let mut rows = page.rows();
        assert_eq!(
            "sla1",
            bitcode::deserialize::<String>(&rows.next().unwrap()).unwrap()
        );
        assert_eq!(
            "sla2",
            bitcode::deserialize::<String>(&rows.next().unwrap()).unwrap()
        );
        assert_eq!(
            "sla3",
            bitcode::deserialize::<String>(&rows.next().unwrap()).unwrap()
        );
        assert_eq!(
            9090,
            bitcode::deserialize::<u64>(&rows.next().unwrap()).unwrap()
        );
        assert!(rows.next().is_none());
    }
}
