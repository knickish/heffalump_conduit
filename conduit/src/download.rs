use html2text::render::text_renderer::{TaggedLine, TextDecorator};
use log::{error, info, warn};
use megalodon::{
    entities::{Attachment, Status},
    megalodon::{
        GetAccountStatusesInputOptions, GetStatusContextInputOptions, GetTimelineOptionsWithLocal,
    },
    Megalodon,
};
use std::time::Duration;

use crate::MASTODON_APP_NAME;

pub fn get_client(
    mastodon_instance: String,
    access_token: String,
) -> Box<dyn Megalodon + Send + Sync> {
    let full_instance_url = format!("https://{}/", mastodon_instance);
    megalodon::generator(
        megalodon::SNS::Mastodon,
        full_instance_url,
        Some(access_token),
        Some(String::from(MASTODON_APP_NAME)),
    )
}

pub async fn feed(
    client: &(dyn Megalodon + Send + Sync),
    count: u32,
) -> Result<(Vec<(String, String)>, Vec<Status>), megalodon::error::Error> {
    let mut res = Vec::new();
    while res.len() != count as usize {
        let options: GetTimelineOptionsWithLocal = GetTimelineOptionsWithLocal {
            only_media: None,
            limit: Some(count - res.len() as u32),
            max_id: res.iter().last().map(|t: &Status| t.id.clone()),
            since_id: None,
            min_id: None,
            local: None,
        };
        let server_response = client.get_home_timeline(Some(&options)).await;
        let mut tmp = match server_response {
            Ok(ok) => ok.json(),
            Err(megalodon::error::Error::RequestError(r))
                if r.status() == Some(http::StatusCode::TOO_MANY_REQUESTS) =>
            {
                warn!("recieved 429, sleeping");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            Err(err) => {
                return Err(err)
                    .inspect_err(|e| error!("Error while downloading timeline posts: {}", e))
            }
        };

        if tmp.len() == 0 {
            break;
        }
        // dont show replies in main feed
        tmp.retain(|p| p.in_reply_to_account_id.is_none());
        res.extend(tmp);
    }

    Ok((res.iter().map(parsed_toot).collect(), res))
}

pub async fn self_posts(
    client: &(dyn Megalodon + Send + Sync),
    count: u32,
) -> Result<(Vec<(String, String)>, Vec<Status>), megalodon::error::Error> {
    let acct = client.verify_account_credentials().await?;
    let mut res = Vec::new();
    while res.len() != count as usize {
        let options = GetAccountStatusesInputOptions {
            only_media: None,
            limit: Some(count - res.len() as u32),
            max_id: None,
            since_id: None,
            pinned: None,
            exclude_replies: None,
            exclude_reblogs: None,
        };
        let tmp = client
            .get_account_statuses(acct.json.id.clone(), Some(&options))
            .await;

        let tmp = match tmp {
            Ok(ok) => ok.json(),
            Err(megalodon::error::Error::RequestError(r))
                if r.status() == Some(http::StatusCode::TOO_MANY_REQUESTS) =>
            {
                warn!("recieved 429, sleeping");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            Err(err) => {
                return Err(err)
                    .inspect_err(|e| error!("Error while downloading self posts: {}", e))
            }
        };

        res.extend(tmp);
    }

    Ok((res.iter().map(parsed_toot).collect(), res))
}

pub async fn replies(
    client: &(dyn Megalodon + Send + Sync),
    posts: impl Iterator<Item = &Status>,
    max_replies_each: usize,
) -> Result<Vec<(Vec<(String, String)>, Vec<Status>)>, megalodon::error::Error> {
    info!("Getting replies");
    let mut statuses = Vec::new();
    let options = GetStatusContextInputOptions {
        limit: Some(max_replies_each as u32),
        ..Default::default()
    };
    for post in posts {
        let rep = loop {
            match client
                .get_status_context(post.id.clone(), Some(&options))
                .await
            {
                Ok(ok) => break ok.json(),
                Err(megalodon::error::Error::RequestError(r))
                    if r.status() == Some(http::StatusCode::TOO_MANY_REQUESTS) =>
                {
                    warn!("recieved 429, sleeping");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
                Err(err) => {
                    return Err(err)
                        .inspect_err(|e| error!("Error while downloading replies: {}", e))
                }
            }
        };

        let replies = rep
            .descendants
            .into_iter()
            .take(max_replies_each)
            .collect::<Vec<_>>();
        statuses.push((replies.iter().map(parsed_toot).collect(), replies));
    }

    Ok(statuses)
}

fn parsed_toot(status: &megalodon::entities::Status) -> (String, String) {
    let mut content = {
        let unformatted = status
            .reblog
            .as_ref()
            .map(|r| &r.content)
            .unwrap_or(&status.content);
        html2text::from_read_with_decorator(unformatted.as_bytes(), usize::MAX, HeffalumpDecorator)
    };

    let author = match &status.reblog {
        Some(reblog) => format!(
            "@{} via @{}",
            reblog.account.acct.split('@').next().unwrap(),
            status.account.acct.split('@').next().unwrap()
        ),
        None => format!("@{}", status.account.acct.split('@').next().unwrap()),
    };
    let mut attachments: Box<dyn Iterator<Item = &Attachment>> =
        Box::new(status.media_attachments.iter());
    if let Some(reblog) = &status.reblog {
        attachments = Box::new(attachments.chain(reblog.media_attachments.iter()));
    }

    for media in attachments {
        content.push_str(
            format!(
                "\n[img] (Alt Text: {})",
                media
                    .description
                    .clone()
                    .unwrap_or_else(|| String::from("No Alt Text"))
            )
            .as_str(),
        )
    }

    if let Some(card) = &status.card {
        match card.description.len() {
            0 => content.push_str("\n[media] (Alt Text: No Alt Text)"),
            _ => content.push_str(format!("\n[media] (Alt Text: {})", &card.description).as_str()),
        }
    }

    (author, content)
}

#[derive(Clone)]
struct HeffalumpDecorator;

impl TextDecorator for HeffalumpDecorator {
    type Annotation = ();

    fn decorate_link_start(&mut self, _url: &str) -> (String, Self::Annotation) {
        (String::default(), ())
    }

    fn decorate_link_end(&mut self) -> String {
        String::default()
    }

    fn decorate_em_start(&self) -> (String, Self::Annotation) {
        ("*".to_string(), ())
    }

    fn decorate_em_end(&self) -> String {
        "*".to_string()
    }

    fn decorate_strong_start(&self) -> (String, Self::Annotation) {
        ("**".to_string(), ())
    }

    fn decorate_strong_end(&self) -> String {
        "**".to_string()
    }

    fn decorate_strikeout_start(&self) -> (String, Self::Annotation) {
        ("".to_string(), ())
    }

    fn decorate_strikeout_end(&self) -> String {
        "".to_string()
    }

    fn decorate_code_start(&self) -> (String, Self::Annotation) {
        ("`".to_string(), ())
    }

    fn decorate_code_end(&self) -> String {
        "`".to_string()
    }

    fn decorate_preformat_first(&self) -> Self::Annotation {}
    fn decorate_preformat_cont(&self) -> Self::Annotation {}

    fn decorate_image(&mut self, _src: &str, title: &str) -> (String, Self::Annotation) {
        (format!("[{}]", title), ())
    }

    fn header_prefix(&self, level: usize) -> String {
        "#".repeat(level) + " "
    }

    fn quote_prefix(&self) -> String {
        "> ".to_string()
    }

    fn unordered_item_prefix(&self) -> String {
        "* ".to_string()
    }

    fn ordered_item_prefix(&self, i: i64) -> String {
        format!("{}. ", i)
    }

    fn finalise(&mut self, _links: Vec<String>) -> Vec<TaggedLine<()>> {
        Vec::new()
    }

    fn make_subblock_decorator(&self) -> Self {
        self.clone()
    }
}

#[cfg(test)]
mod test {
    use crate::download::{feed, get_client};

    #[tokio::test]
    async fn test_feed() {
        let token = env!("HEFFALUMP_ACCESS_TOKEN").to_string();
        let instance = env!("HEFFALUMP_MASTADON_INST").to_string();
        let client = get_client(instance, token);
        for (author, content) in feed(client.as_ref(), 100).await.unwrap().0 {
            println!("{}\n{}", author, content);
        }
    }
}
