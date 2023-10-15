use log::{error, info};
use megalodon::{entities::Status, megalodon::PostStatusInputOptions, Megalodon};
use palmrs::database::record::pdb_record::RecordAttributes;

use crate::heffalump_hh_types::{Record, TootWrite};

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
) -> Result<(), megalodon::error::Error> {
    for write in writes {
        execute_single_write(client, write, &source).await?;
    }
    Ok(())
}

async fn execute_single_write(
    client: &(dyn Megalodon + Send + Sync),
    write: TootWrite,
    source: &[Status],
) -> Result<(), megalodon::error::Error> {
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
                val => {
                    let mut options = PostStatusInputOptions::default();
                    let status_id = source.get(val as usize).ok_or(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Cache seems to have incorrect data",
                    ))?;
                    options.in_reply_to_id = Some(status_id.id.clone());
                    Some(options)
                }
            };
            let content = String::from_utf8_lossy(&toot.contents).into_owned();
            info!("{}", content);
            if let Err(e) = client
                .post_status(
                    String::from_utf8_lossy(&toot.contents).into_owned(),
                    options.as_ref(),
                )
                .await
            {
                error!("{}", e);
                return Err(e);
            };
        }
    }
    Ok(())
}
