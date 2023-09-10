// typedef struct TootContent_s {
//     UInt16  author;
//     UInt16  is_reply_to;
//     UInt16  content_len;
//     char    toot_content[];
// } TootContent;

// typedef struct TootAuthor_s {
//     UInt8 author_name_len;
//     char  author_name[];
// } TootAuthor;

use byteorder::{BigEndian, WriteBytesExt};
use std::io::{Cursor, Write};

pub trait Record {
    fn to_hh_bytes(&self) -> std::io::Result<Vec<u8>>;
}

pub(crate) struct TootContent {
    pub(crate) author: u16,
    pub(crate) is_reply_to: u16,
    // pub(crate) content_len: u16, not used in rust, needed in c
    pub(crate) contents: Vec<u8>,
}

pub(crate) struct TootAuthor {
    // pub(crate) author_name_len: u16, not used in rust, needed in c
    pub(crate) author_name: Vec<u8>,
}

impl Record for TootContent {
    fn to_hh_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::new());
        cursor.write_u16::<BigEndian>(self.author)?;
        cursor.write_u16::<BigEndian>(self.is_reply_to)?;
        cursor.write_u16::<BigEndian>(self.contents.len() as u16)?;
        cursor.write_all(&self.contents)?;

        Ok(cursor.into_inner())
    }
}

impl Record for TootAuthor {
    fn to_hh_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::new());
        cursor.write_u8(self.author_name.len() as u8)?;
        cursor.write_all(&self.author_name)?;

        Ok(cursor.into_inner())
    }
}
