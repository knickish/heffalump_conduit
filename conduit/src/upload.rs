use log::{error, info};
use megalodon::{entities::Status, error::Error, megalodon::PostStatusInputOptions, Megalodon};
use palmrs::database::record::pdb_record::RecordAttributes;

use crate::heffalump_hh_types::{OnDevice, TootWrite};

pub(crate) fn parse_writes(
    raw_device_data: Vec<(Vec<u8>, RecordAttributes, u32)>,
) -> std::io::Result<Vec<TootWrite>> {
    raw_device_data
        .into_iter()
        .map(|(operation, _, _)| TootWrite::from_hh_bytes(&operation))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| {
            error!("{}", e);
            e
        })
}

pub(crate) async fn execute_writes(
    client: &(dyn Megalodon + Send + Sync),
    writes: Vec<TootWrite>,
    source: Vec<Status>,
) -> Result<(), Error> {
    for write in writes {
        execute_single_write(client, write, &source).await?;
    }
    Ok(())
}

async fn execute_single_write(
    client: &(dyn Megalodon + Send + Sync),
    write: TootWrite,
    source: &[Status],
) -> Result<(), Error> {
    match write {
        TootWrite::Favorite(fav) => {
            let status = source.get(fav as usize).ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Cache seems to have incorrect data",
            ))?;
            client.favourite_status(status.id.clone()).await?;
        }
        TootWrite::Follow(_) => (),
        TootWrite::Reblog(reblog) => {
            let status = source.get(reblog as usize).ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Cache seems to have incorrect data",
            ))?;
            client.reblog_status(status.id.clone()).await?;
        }
        TootWrite::Toot(toot) => {
            let options = match toot.is_reply_to {
                0 => None,
                // have to emulate an option type because C.
                //  See TootContentConstuctor in heffalump
                val @ 1.. => {
                    let mut options = PostStatusInputOptions::default();
                    let status_id = source.get((val - 1) as usize).ok_or(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Cache seems to have incorrect data",
                    ))?;
                    options.in_reply_to_id = Some(status_id.id.clone());
                    Some(options)
                }
            };

            use encoding::all::ISO_8859_1;
            use encoding::{DecoderTrap, Encoding};
            let content = match ISO_8859_1.decode(&toot.contents, DecoderTrap::Strict) {
                Ok(c) => c,
                Err(e) => {
                    error!("Error decoding text from handheld: {e}");
                    return Err(Error::StandardError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string().as_str(),
                    )));
                }
            };

            match options
                .as_ref()
                .map(|x| x.in_reply_to_id.as_ref())
                .flatten()
            {
                Some(reply_id) => info!("Posting in reply to {}: {}", reply_id, &content),
                None => info!("Posting: {}", &content),
            };
            if let Err(e) = client.post_status(content, options.as_ref()).await {
                error!("{}", e);
                return Err(e);
            };
        }
    }
    Ok(())
}
