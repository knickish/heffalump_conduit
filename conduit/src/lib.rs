use std::{
    ffi::{c_long, c_uchar, c_void, CString},
    fmt::Display,
    path::{Path, PathBuf},
};

use hotsync_conduit_rs::{CSyncProperties, ConduitBuilder, ConduitDBSource};
use log::{error, info, trace};
use megalodon::Megalodon;
use palmrs::database::{record::pdb_record::RecordAttributes, PalmDatabase, PdbDatabase};
use simplelog::*;

mod config;
mod download;
mod heffalump_hh_types;
mod upload;

use download::{feed, get_client};
use heffalump_hh_types::{Record, TootAuthor, TootContent};
use upload::*;

const CREATOR: [c_uchar; 4] = [b'H', b'E', b'F', b'f'];
const MASTODON_APP_NAME: &str = "Heffalump 0.2 (PalmOS)";
const AUTHOR_DB: &[u8] = include_bytes!("../include/HeffalumpAuthorDB.pdb");
const CONTENT_DB: &[u8] = include_bytes!("../include/HeffalumpContentDB.pdb");
const MASTODON_CACHE_OLD: &str = "heffalump_mastodon_timeline_old.json";
const MASTODON_CACHE_NEW: &str = "heffalump_mastodon_timeline.json";
const CONFIG_FILE: &str = "heffalump_config.json";

#[no_mangle]
/// # Safety
///
/// these pointers are initialized by HS Manager, and this function should only ever be called by it
pub unsafe extern "cdecl" fn OpenConduit(
    _: *const c_void,
    sync_props: *const CSyncProperties,
) -> c_long {
    let Some(path) = (unsafe { path_from_sync_props(sync_props) }) else {
        return -1;
    };
    initialize_logger(&path);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let config_path = {
        let mut owned = path.clone().to_owned();
        owned.push(CONFIG_FILE);
        owned
    };

    if let Err(std::io::ErrorKind::NotFound) = std::fs::metadata(&config_path).map_err(|e| e.kind())
    {
        if runtime
            .block_on(config::configure(&config_path))
            .map_err(log_err)
            .is_err()
        {
            return -1;
        }
    }

    let Ok(config) = std::fs::read_to_string(config_path).map_err(log_err) else {
        return -1;
    };

    let Ok((mastodon_inst, mastodon_access)) = serde_json::from_str(&config).map_err(log_err)
    else {
        return -1;
    };

    let client = get_client(mastodon_inst, mastodon_access);
    let Ok((author_db, content_db)) = runtime.block_on(create_dbs(client.as_ref(), Some(&path)))
    else {
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
                    let source_file = match (std::fs::File::open(&path), parsed.len()) {
                        (Ok(f), _) => f,
                        (Err(_), 0) => {
                            // We can't find the cache, but don't have anything to write anyway
                            return Ok(());
                        }
                        (Err(e), _) => {
                            error!("Failed to open cache: {}", e);
                            return Err(Box::new(e));
                        }
                    };
                    trace!("found cache");
                    let source =
                        serde_json::from_reader(&source_file).expect("Cannot deserialize cache");
                    trace!("deserialized cache");
                    if let Err(e) =
                        runtime.block_on(execute_writes(client.as_ref(), parsed, source))
                    {
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

unsafe fn path_from_sync_props(props: *const CSyncProperties) -> Option<PathBuf> {
    // SAFETY this is initialized by HS Manager
    // this is the only way to retrieve the path for the application's
    // HotSync-created directory
    match unsafe { props.as_ref() } {
        Some(sync_props) => sync_props.get_dir_path(),
        None => None,
    }
}

fn to_latin_1(arg: String) -> Vec<u8> {
    use encoding::{
        all::ISO_8859_1,
        {EncoderTrap, Encoding},
    };

    ISO_8859_1
        .encode(arg.as_str(), EncoderTrap::Ignore)
        .expect("Ignoring non-encodable chars, this shouldn't be reachable")
}

fn initialize_logger(at: &Path) {
    let mut log_path = at.to_owned();
    log_path.push("heffalump.log");
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Info,
        Config::default(),
        std::fs::File::create(log_path).unwrap(),
    )])
    .unwrap();

    info!("Logger Initialized");
}

fn log_err<E: Display>(error: E) -> E {
    error!("{error}");
    error
}

async fn create_dbs(
    client: &(dyn Megalodon + Send + Sync),
    write_to_path: Option<&Path>,
) -> Result<(PalmDatabase<PdbDatabase>, PalmDatabase<PdbDatabase>), ()> {
    let mut base_author =
        PalmDatabase::<PdbDatabase>::from_bytes(AUTHOR_DB).map_err(|e| error!("{}", e))?;
    let mut base_content =
        PalmDatabase::<PdbDatabase>::from_bytes(CONTENT_DB).map_err(|e| error!("{}", e))?;
    let (contents, raw) = feed(client, 1000).await.map_err(|e| error!("{}", e))?;
    for (idx, (author, content)) in contents.into_iter().enumerate() {
        let author = TootAuthor {
            author_name: to_latin_1(author),
        }
        .to_hh_bytes()
        .map_err(|e| error!("{}", e))?;
        let content = TootContent {
            author: idx as u16,
            is_reply_to: 0,
            contents: to_latin_1(content),
        }
        .to_hh_bytes()
        .map_err(|e| error!("{}", e))?;
        base_author.insert_record(RecordAttributes::default(), &author);
        base_content.insert_record(RecordAttributes::default(), &content);
    }
    if let Some(path) = write_to_path {
        let mut owned = path.to_owned();
        owned.push(MASTODON_CACHE_NEW);
        if std::fs::metadata(&owned).is_ok() {
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
