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

// enum TootWriteType {
//     Favorite = 0,
//     Follow = 1,
//     Reblog = 2,
//     Toot = 3,
//     // to ensure the values chosen ar
//     DoNotUse = 0xFF
// }

// union TootWriteContent {
//     UInt16 favorite;
//     UInt16 reblog;
//     UInt16 follow;
//     TootContent toot;
// } ;

// struct TootWrite {
//     UInt8 type;
//     TootWriteContent content;
// }

// typedef struct HeffalumpPrefs_s {
//     UInt16          timeline_content_start;
//     UInt16          self_content_start;
//     UInt16          reply_content_start;
//     UInt16          reply_content_end;
// } HeffalumpPrefs;

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use log::debug;
use std::{ffi::CString, io::{Cursor, Read, Write}};

#[derive(Debug, Clone)]
pub(crate) struct HeffalumpPrefs {
    pub(crate) self_content_start: u16,
    pub(crate) test: CString  
    // reply_content_start: u16,
    // reply_content_end: u16,
}

pub trait OnDevice: Sized {
    fn to_hh_bytes(&self) -> std::io::Result<Vec<u8>>;
    fn from_hh_bytes(bytes: &[u8]) -> std::io::Result<Self>;
}

#[derive(Debug, Clone)]
pub(crate) struct TootContent {
    pub(crate) author: u16,
    pub(crate) is_reply_to: u16,
    // pub(crate) content_len: u16, not used in rust, needed in c
    pub(crate) contents: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct TootAuthor {
    // pub(crate) author_name_len: u16, not used in rust, needed in c
    pub(crate) author_name: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) enum TootWrite {
    Favorite(u16),
    Follow(u16),
    Reblog(u16),
    Toot(TootContent),
}

impl TootWrite {
    fn c_enum_val(&self) -> u16 {
        match self {
            TootWrite::Favorite(_) => 0,
            TootWrite::Follow(_) => 1,
            TootWrite::Reblog(_) => 2,
            TootWrite::Toot(_) => 3,
        }
    }
}

impl OnDevice for TootContent {
    fn to_hh_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::new());
        cursor.write_u16::<BigEndian>(self.author)?;
        cursor.write_u16::<BigEndian>(self.is_reply_to)?;
        cursor.write_u16::<BigEndian>(self.contents.len() as u16)?;
        cursor.write_all(&self.contents)?;

        Ok(cursor.into_inner())
    }

    fn from_hh_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut cursor = Cursor::new(bytes);
        let author = cursor.read_u16::<BigEndian>()?;
        let is_reply_to = cursor.read_u16::<BigEndian>()?;
        let content_len = cursor.read_u16::<BigEndian>()? as usize;

        if content_len == 0 {
            debug!("Size of content is: {} bytes", content_len);
            debug!("content is: \n{:?} ", bytes);
        }
        let mut content_buf = vec![0_u8; content_len];
        cursor.read_exact(&mut content_buf)?;
        Ok(Self {
            author,
            is_reply_to,
            contents: content_buf,
        })
    }
}

impl OnDevice for TootAuthor {
    fn to_hh_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::new());
        cursor.write_u8(self.author_name.len() as u8)?;
        cursor.write_all(&self.author_name)?;

        Ok(cursor.into_inner())
    }

    fn from_hh_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut cursor = Cursor::new(bytes);
        let mut content_buf = vec![0_u8; cursor.read_u16::<BigEndian>()? as usize];
        cursor.read_exact(&mut content_buf)?;
        Ok(Self {
            author_name: content_buf,
        })
    }
}

impl OnDevice for TootWrite {
    fn to_hh_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::new());
        cursor.write_u16::<BigEndian>(self.c_enum_val())?;
        match self {
            TootWrite::Favorite(val) => cursor.write_u16::<BigEndian>(*val)?,
            TootWrite::Follow(val) => cursor.write_u16::<BigEndian>(*val)?,
            TootWrite::Reblog(val) => cursor.write_u16::<BigEndian>(*val)?,
            TootWrite::Toot(toot) => cursor.write_all(toot.clone().to_hh_bytes()?.as_ref())?,
        }

        Ok(cursor.into_inner())
    }

    fn from_hh_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut cursor = Cursor::new(bytes);
        let discrim = cursor.read_u16::<BigEndian>()?;
        match discrim {
            0 => Ok(Self::Favorite(cursor.read_u16::<BigEndian>()?)),
            1 => Ok(Self::Follow(cursor.read_u16::<BigEndian>()?)),
            2 => Ok(Self::Reblog(cursor.read_u16::<BigEndian>()?)),
            3 => Ok(Self::Toot(TootContent::from_hh_bytes(
                cursor.get_ref()[(cursor.position() as usize)..].as_ref(),
            )?)),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid discriminant",
            )),
        }
    }
}

impl OnDevice for HeffalumpPrefs {
    fn to_hh_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::new());
        cursor.write_u16::<BigEndian>(self.self_content_start)?;
        cursor.write_all(self.test.as_bytes_with_nul())?;
        Ok(cursor.into_inner())
    }

    fn from_hh_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut cursor = Cursor::new(bytes);
        let num = cursor.read_u16::<BigEndian>()?;
        let str = CString::from_vec_with_nul(cursor.get_ref()[(cursor.position() as usize)..].to_owned())
            .map_err(|_| std::io::Error::new(
                std::io::ErrorKind::InvalidData, 
                "Could not read preferences from device"
            ))?;

        Ok(HeffalumpPrefs { self_content_start: num, test: str })
    }
}

impl TryFrom<Vec<u8>> for HeffalumpPrefs {
    type Error = std::io::Error;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        HeffalumpPrefs::from_hh_bytes(&value)
    }
}

impl TryInto<Vec<u8>> for HeffalumpPrefs {
    type Error = std::io::Error;

    fn try_into(self) -> Result<Vec<u8>, Self::Error> {
        self.to_hh_bytes()
    }
}