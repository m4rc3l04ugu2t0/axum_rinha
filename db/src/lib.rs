#![allow(unused)]
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, Write},
    iter,
    marker::PhantomData,
    path::Path,
};

use serde::{de::DeserializeOwned, Serialize};

type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Serialize(Box<dyn std::error::Error>),
    Io(io::Error),
    DataSize,
}

const PAGE_SIZE: usize = 4096;

pub struct Page<const ROW_SIZE: usize = 64> {
    pub data: Vec<u8>,
}

impl<const ROW_SIZE: usize> Page<ROW_SIZE> {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        if data.len() != PAGE_SIZE {
            return Err(Error::DataSize);
        }
        Ok(Self { data })
    }

    pub fn insert<S: Serialize>(&mut self, row: S) -> Result<()> {
        let serialized = bitcode::serialize(&row).map_err(|e| Error::Serialize(Box::new(e)))?;

        let size = serialized.len() as u64;
        let size = size.to_be_bytes();

        self.data.write(&size).map_err(Error::Io)?;
        self.data.write(&serialized).map_err(Error::Io)?;
        self.data
            .write(&vec![0; ROW_SIZE - (serialized.len() + size.len())])
            .map_err(Error::Io)?;

        Ok(())
    }

    pub fn rows(&self) -> impl Iterator<Item = &[u8]> + '_ {
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
            Some(&row[8..8 + size])
        })
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    pub fn available_rows(&self) -> usize {
        (PAGE_SIZE - self.data.len()) / ROW_SIZE
    }
}

impl<const ROW_SIZE: usize> AsRef<[u8]> for Page<ROW_SIZE> {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}

struct Db<T, const ROW_SIZE: usize = 64> {
    current_page: Page,
    writer: File,
    reader: File,
    data: PhantomData<T>,
}

impl<const ROW_SIZE: usize, T: Serialize + DeserializeOwned> Db<T, ROW_SIZE> {
    pub fn from_path(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&path)?;
        Ok(Self {
            current_page: Page::new(),
            reader: File::open(&path)?,
            writer: file,
            data: PhantomData,
        })
    }

    pub fn insert(&mut self, row: T) -> Result<()> {
        self.current_page.insert(row);
        self.writer.write_all(self.current_page.as_ref());
        self.writer
            .write_all(&vec![0; PAGE_SIZE - self.current_page.len()]);

        if self.current_page.available_rows() == 0 {
            self.current_page = Page::new();
        } else {
            self.writer.seek(io::SeekFrom::End(-(PAGE_SIZE as i64)));
        }
        Ok(())
    }

    fn pages(&mut self) -> impl Iterator<Item = Page> + '_ {
        let mut cursor = 0;
        iter::from_fn(move || {
            let offset = (cursor * PAGE_SIZE) as u64;
            if self.reader.seek(io::SeekFrom::Start(offset)).is_err() {
                return None;
            }

            let mut buf = vec![0; PAGE_SIZE];
            cursor += 1;
            match self.reader.read_exact(&mut buf) {
                Ok(()) => Some(Page::from_bytes(buf).unwrap()),
                Err(_) => None,
            }
        })
    }

    pub fn rows(&mut self) -> impl Iterator<Item = T> + '_ {
        self.pages().flat_map(|p| {
            p.rows()
                .filter_map(|r| bitcode::deserialize(r).ok())
                .collect::<Vec<_>>()
        })
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn it_works() {
        let mut page = Page::<1024>::new();
        assert_eq!(4, page.available_rows());
        page.insert(String::from("sla1"));
        assert_eq!(3, page.available_rows());
        page.insert(String::from("sla2"));
        assert_eq!(2, page.available_rows());
        page.insert(String::from("sla3"));
        assert_eq!(1, page.available_rows());
        page.insert(9090 as u64);
        assert_eq!(0, page.available_rows());

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

    fn test_insert_into_db() {
        let tmp = tempdir().unwrap();
        let mut db = Db::<(i32, String)>::from_path(tmp.path().join("test.db")).unwrap();
        db.insert((50, String::from("value")));
        db.insert((-50, String::from("sla")));

        let mut rows = db.rows();

        assert_eq!((50, String::from("value")), rows.next().unwrap());
        assert_eq!((-50, String::from("sla")), rows.next().unwrap());
        assert!(rows.next().is_none());
    }
}
