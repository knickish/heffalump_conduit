use std::{
    ffi::{c_long, c_uchar, c_void, CString},
    path::Path,
};

use hotsync_conduit_rs::{CSyncProperties, ConduitBuilder, ConduitDBSource};
use log::{error, info, trace};
use megalodon::Megalodon;
use palmrs::database::{record::pdb_record::RecordAttributes, PalmDatabase, PdbDatabase};
use simplelog::*;

mod download;
mod heffalump_hh_types;
mod upload;

use download::{feed, get_client};
use heffalump_hh_types::{Record, TootAuthor, TootContent};
use upload::*;

const CREATOR: [c_uchar; 4] = [b'H', b'E', b'F', b'f'];
const AUTHOR_DB: &[u8] = include_bytes!("../include/HeffalumpAuthorDB.pdb");
const CONTENT_DB: &[u8] = include_bytes!("../include/HeffalumpContentDB.pdb");
const MASTADON_INST: &'static str = env!("HEFFALUMP_MASTADON_INST");
const MASTADON_ACCESS: &'static str = env!("HEFFALUMP_ACCESS_TOKEN");
const MASTODON_CACHE_OLD: &'static str = "heffalump_mastodon_timeline_old.json";
const MASTODON_CACHE_NEW: &'static str = "heffalump_mastodon_timeline.json";

#[no_mangle]
pub extern "cdecl" fn OpenConduit(_: *const c_void, sync_props: *const CSyncProperties) -> c_long {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let client = get_client(MASTODON_INST.to_owned(), MASTODON_ACCESS.to_owned());

    // SAFETY this is initialized by HS Manager
    // this is the only way to retrieve the path for the application's
    // HotSync-created directory
    let path = unsafe { sync_props.as_ref().unwrap().get_dir_path().unwrap() };
    initialize_logger(&path);

    let Ok((author_db, content_db)) = runtime.block_on(create_dbs(&client, Some(&path))) else {
        return -1;
    };

    let conduit =
        ConduitBuilder::new_with_name_creator(CString::new("heffalump_conduit").unwrap(), CREATOR)
            .download_db_and(
                CString::new("HeffalumpWritesDB").unwrap(),
                hotsync_conduit_rs::ConduitDBSink::Dynamic(Box::new(move |from_hh| {
                    let parsed = match parse_writes(from_hh) {
                        Ok(v) => v,
                        Err(e) => return Err(Box::new(e)),
                    };
                    trace!("parsed writes");
                    let mut path = path.clone();
                    path.push(MASTODON_CACHE_OLD);
                    let source_file = match std::fs::File::open(&path) {
                        Ok(f) => f,
                        Err(e) => {
                            error!("Failed to open cache: {}", e);
                            return Err(Box::new(e));
                        }
                    };
                    trace!("found cache");
                    let source =
                        serde_json::from_reader(&source_file).expect("Cannot deserialize cache");
                    trace!("deserialized cache");
                    if let Err(e) = runtime.block_on(execute_writes(&client, parsed, source)) {
                        error!("Failed to write with result: {}", e);
                        return Err(Box::new(e));
                    }
                    trace!("executed writes");
                    Ok(())
                })),
            )
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

fn initialize_logger(at: &Path) {
    let mut log_path = at.to_owned();
    log_path.push("heffalump.log");
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Debug,
        Config::default(),
        std::fs::File::create(log_path).unwrap(),
    )])
    .unwrap();

    info!("Logger Initialized");
}

async fn create_dbs(
    client: &Box<dyn Megalodon + Send + Sync>,
    write_to_path: Option<&Path>,
) -> Result<(PalmDatabase<PdbDatabase>, PalmDatabase<PdbDatabase>), ()> {
    let mut base_author =
        PalmDatabase::<PdbDatabase>::from_bytes(&AUTHOR_DB).map_err(|e| error!("{}", e))?;
    let mut base_content =
        PalmDatabase::<PdbDatabase>::from_bytes(&CONTENT_DB).map_err(|e| error!("{}", e))?;
    let (contents, raw) = feed(&client, 1000).await.map_err(|e| error!("{}", e))?;
    for (idx, (author, content)) in contents.into_iter().enumerate() {
        let author = TootAuthor {
            author_name: to_ascii(author),
        }
        .to_hh_bytes()
        .map_err(|e| error!("{}", e))?;
        let content = TootContent {
            author: idx as u16,
            is_reply_to: 0,
            contents: to_ascii(content),
        }
        .to_hh_bytes()
        .map_err(|e| error!("{}", e))?;
        base_author.insert_record(RecordAttributes::default(), &author);
        base_content.insert_record(RecordAttributes::default(), &content);
    }
    if let Some(path) = write_to_path {
        let mut owned = path.clone().to_owned();
        owned.push(MASTODON_CACHE_NEW);
        if let Ok(_) = std::fs::metadata(&owned) {
            // a previous version exists, move it to a different path
            // (overwrites the previous _old file)
            let mut prev = path.to_owned();
            prev.push(MASTODON_CACHE_OLD);
            std::fs::rename(&owned, &prev).map_err(|e| error!("{}", e))?;
        }
        let file = std::fs::File::create(&owned).map_err(|e| error!("{}", e))?;
        serde_json::to_writer(&file, &raw).map_err(|e| error!("{}", e))?;
        file.sync_all().map_err(|e| error!("{}", e))?;
    }
    Ok((base_author, base_content))
}

#[cfg(test)]
mod test {
    use crate::{
        create_dbs, download::get_client, MASTODON_ACCESS, MASTODON_CACHE_NEW, MASTODON_INST,
    };

    #[tokio::test]
    async fn test_create() {
        let mut path = std::env::temp_dir();
        let client = get_client(MASTODON_INST.to_owned(), MASTODON_ACCESS.to_owned());
        let (auth, cont) = create_dbs(&client, Some(&path)).await.unwrap();
        dbg!(auth.to_bytes().unwrap().len());
        dbg!(cont.to_bytes().unwrap().len());
        path.push(MASTODON_CACHE_NEW);
        assert!(std::fs::metadata(&path).is_ok())
    }
}
