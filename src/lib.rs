use std::ffi::{c_long, c_uchar, c_void, CString};

use download::feed;
use heffalump_hh_types::{Record, TootAuthor, TootContent};
use hotsync_conduit_rs::{ConduitBuilder, ConduitDBSource};
use palmrs::database::{record::pdb_record::RecordAttributes, PalmDatabase, PdbDatabase};

mod download;
mod heffalump_hh_types;

const CREATOR: [c_uchar; 4] = [b'H', b'E', b'F', b'f'];
const AUTHOR_DB: &[u8] = include_bytes!("..\\include\\HeffalumpAuthorDB.pdb");
const CONTENT_DB: &[u8] = include_bytes!("..\\include\\HeffalumpContentDB.pdb");
const MASTADON_INST: &'static str = env!("HEFFALUMP_MASTADON_INST");
const MASTADON_ACCESS: &'static str = env!("HEFFALUMP_ACCESS_TOKEN");

#[no_mangle]
pub extern "cdecl" fn OpenConduit(_: *const c_void, _: *const c_void) -> c_long {
    let Ok((author_db, content_db)) = create_dbs() else {
        return -1;
    };

    let conduit =
        ConduitBuilder::new_with_name_creator(CString::new("heffalump_conduit").unwrap(), CREATOR)
            .overwrite_db(ConduitDBSource::Static(
                CString::new("HeffalumpAuthorDB").unwrap(),
                [b'A', b'u', b't', b'h'],
                author_db,
            ))
            .overwrite_db(ConduitDBSource::Static(
                CString::new("HeffalumpContentDB").unwrap(),
                [b'T', b'o', b'o', b't'],
                content_db,
            ))
            .build();

    match conduit.sync() {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

fn to_ascii(arg: String) -> Vec<u8> {
    arg.chars()
        .filter(|c| char::is_ascii(&c))
        .collect::<String>()
        .into_bytes()
}

fn create_dbs() -> Result<(PalmDatabase<PdbDatabase>, PalmDatabase<PdbDatabase>), ()> {
    let mut base_author = PalmDatabase::<PdbDatabase>::from_bytes(&AUTHOR_DB).map_err(|e| {
        dbg!(e);
        ()
    })?;
    let mut base_content = PalmDatabase::<PdbDatabase>::from_bytes(&CONTENT_DB).map_err(|e| {
        dbg!(e);
        ()
    })?;
    let contents = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(feed(
            MASTADON_INST.to_string(),
            MASTADON_ACCESS.to_string(),
            1000,
        ))
        .map_err(|e| {
            dbg!(e);
            ()
        })?;
    for (idx, (author, content)) in contents.into_iter().enumerate() {
        let author = TootAuthor {
            author_name: to_ascii(author),
        }
        .to_hh_bytes()
        .map_err(|e| {
            dbg!(e);
            ()
        })?;
        let content = TootContent {
            author: idx as u16,
            is_reply_to: 0,
            contents: to_ascii(content),
        }
        .to_hh_bytes()
        .map_err(|e| {
            dbg!(e);
            ()
        })?;
        base_author.insert_record(RecordAttributes::default(), &author);
        base_content.insert_record(RecordAttributes::default(), &content);
    }
    Ok((base_author, base_content))
}

#[cfg(test)]
mod test {
    use crate::create_dbs;

    #[test]
    fn test_create() {
        let (auth, cont) = create_dbs().unwrap();
        dbg!(auth.to_bytes().unwrap().len());
        dbg!(cont.to_bytes().unwrap().len());
    }
}
