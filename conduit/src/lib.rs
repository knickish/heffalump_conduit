use std::{
    collections::BTreeMap,
    ffi::{c_long, c_uchar, c_void, CString},
    fmt::Display,
    path::{Path, PathBuf},
};

use hotsync_conduit_rs::{CSyncProperties, ConduitBuilder, ConduitDBSource, PreferenceType};
use log::{error, info, trace};
use megalodon::Megalodon;
use palmrs::database::{record::pdb_record::RecordAttributes, PalmDatabase, PdbDatabase};
use simplelog::*;

mod config;
mod download;
mod heffalump_hh_types;
mod upload;

use download::{feed, get_client, replies, self_posts};
use heffalump_hh_types::{HeffalumpPrefs, OnDevice, TootAuthor, TootContent};
use tokio::try_join;
use upload::*;

const CREATOR: [c_uchar; 4] = [b'H', b'E', b'F', b'f'];
const MASTODON_APP_NAME: &str = "Heffalump 0.2 (PalmOS)";
const AUTHOR_DB: &[u8] = include_bytes!("../include/HeffalumpAuthorDB.pdb");
const CONTENT_DB: &[u8] = include_bytes!("../include/HeffalumpContentDB.pdb");
const MASTODON_CACHE_OLD: &str = "heffalump_mastodon_timeline_old.json";
const MASTODON_CACHE_NEW: &str = "heffalump_mastodon_timeline.json";
const CONFIG_FILE: &str = "heffalump_config.json";

const DB_NAME_CONTENT: &str = "HeffalumpContentDB";
const DB_NAME_AUTHOR: &str = "HeffalumpAuthorDB";
const DB_NAME_WRITES: &str = "HeffalumpWritesDB";

#[no_mangle]
/// # Safety
///
/// these pointers are initialized by HS Manager, and this function should only ever be called by it
pub unsafe extern "cdecl" fn OpenConduit(
    _: *const c_void,
    sync_props: *const CSyncProperties,
) -> c_long {
    match std::panic::catch_unwind(|| conduit(sync_props)) {
        Ok(res) => res,
        Err(_) => {
            error!(
                "Caught Panic: {}",
                std::backtrace::Backtrace::force_capture()
            );
            -1
        }
    }
}

unsafe fn conduit(sync_props: *const CSyncProperties) -> c_long {
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
    let Ok((author_db, content_db, prefs)) =
        runtime.block_on(create_dbs(client.as_ref(), Some(&path)))
    else {
        return -1;
    };
    info!("{:?}", &prefs);

    let conduit = ConduitBuilder::<HeffalumpPrefs>::new_with_name_creator(
        CString::new("heffalump_conduit").unwrap(),
        CREATOR,
    )
    .download_db_and(
        CString::new(DB_NAME_WRITES).unwrap(),
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
            let (prefs, source) = match serde_json::from_reader(&source_file) {
                Ok(ok) => ok,
                Err(e) => {
                    error!("Failed to deserialize cache with error: {}", e);
                    return Err(Box::new(e));
                }
            };
            trace!("deserialized cache");

            if let Err(e) = runtime.block_on(execute_writes(client.as_ref(), parsed, source, prefs))
            {
                error!("Failed to write with result: {}", e);
                return Err(Box::new(e));
            }
            trace!("executed writes");
            Ok(())
        })),
    )
    .overwrite_db(ConduitDBSource::Static(
        CString::new(DB_NAME_AUTHOR).unwrap(),
        [b'A', b'u', b't', b'h'],
        author_db,
    ))
    .overwrite_db(ConduitDBSource::Static(
        CString::new(DB_NAME_CONTENT).unwrap(),
        [b'T', b'o', b'o', b't'],
        content_db,
    ))
    .set_preferences(PreferenceType::Static(0, prefs))
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

fn to_latin_1(arg: impl AsRef<str>, cutoff: Option<usize>, add_null: bool) -> Vec<u8> {
    use encoding::{
        all::ISO_8859_1,
        {EncoderTrap, Encoding},
    };

    let mut ret = ISO_8859_1
        .encode(arg.as_ref(), EncoderTrap::Ignore)
        .expect("Ignoring non-encodable chars, this shouldn't be reachable");

    if let Some(cutoff) = cutoff {
        ret.truncate(cutoff);
    }

    if add_null {
        ret.push(0);
    }

    ret
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
) -> Result<
    (
        PalmDatabase<PdbDatabase>,
        PalmDatabase<PdbDatabase>,
        HeffalumpPrefs,
    ),
    (),
> {
    let mut base_author =
        PalmDatabase::<PdbDatabase>::from_bytes(AUTHOR_DB).map_err(|e| error!("{}", e))?;
    let mut base_content =
        PalmDatabase::<PdbDatabase>::from_bytes(CONTENT_DB).map_err(|e| error!("{}", e))?;
    let mut prefs = HeffalumpPrefs::default();

    let ((feed_contents, mut feed_raw), (self_contents, self_raw)) =
        try_join!(feed(client, 100), self_posts(client, 40)).map_err(|e| error!("{}", e))?;
    let replies = replies(client, feed_raw.iter().chain(self_raw.iter()), 10)
        .await
        .map_err(|e| error!("{}", e))?;

    prefs.home_timeline_len = feed_contents.len() as u16;
    prefs.self_timeline_len = self_contents.len() as u16;
    prefs.reply_content_len = replies.len() as u16;

    feed_raw.extend(self_raw);

    let authors = self_contents
        .iter()
        .chain(&feed_contents)
        .chain(replies.iter().flat_map(|t| &t.0))
        .map(|(author, _)| (author.to_string(), to_latin_1(author, Some(39), true)))
        .collect::<BTreeMap<_, _>>();

    let mut start = feed_contents.len() + self_contents.len();
    for ((author, content), replies) in feed_contents
        .into_iter()
        .chain(self_contents)
        .zip(replies.iter().map(|t| t.0.len()))
    {
        let (idx, _) = authors
            .iter()
            .enumerate()
            .find(|(_idx, (k, _v))| k == &&author)
            .to_owned()
            .unwrap();
        let content = match replies == 0 {
            true => TootContent {
                author: idx as u16,
                is_reply_to: 0,
                replies_start: 0,
                contents: to_latin_1(content, None, false),
            },
            false => {
                let ret = TootContent {
                    author: idx as u16,
                    is_reply_to: 0,
                    replies_start: start as u16,
                    contents: to_latin_1(content, None, false),
                };
                start += replies;
                ret
            }
        };
        let content = content.to_hh_bytes().map_err(|e| error!("{}", e))?;
        base_content.insert_record(RecordAttributes::default(), &content);
    }

    for (index, contents) in replies.iter().map(|t| &t.0).enumerate() {
        for (author, content) in contents.iter() {
            let (author_idx, _) = authors
                .iter()
                .enumerate()
                .find(|(_idx, (k, _v))| k == &author)
                .to_owned()
                .unwrap();
            let content = TootContent {
                author: author_idx as u16,
                is_reply_to: index as u16,
                replies_start: 0,
                contents: to_latin_1(content, None, false),
            };
            let content = content.to_hh_bytes().map_err(|e| error!("{}", e))?;
            base_content.insert_record(RecordAttributes::default(), &content);
        }
    }

    feed_raw.extend(replies.into_iter().flat_map(|t| t.1));

    for author_name in authors.into_values() {
        let author = TootAuthor { author_name }
            .to_hh_bytes()
            .map_err(|e| error!("{}", e))?;
        base_author.insert_record(RecordAttributes::default(), &author);
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
        serde_json::to_writer(&file, &(&prefs, feed_raw)).map_err(|e| error!("{}", e))?;
        file.sync_all().map_err(|e| error!("{}", e))?;
    }
    Ok((base_author, base_content, prefs))
}
