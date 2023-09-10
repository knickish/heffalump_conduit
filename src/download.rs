use megalodon::megalodon::GetTimelineOptionsWithLocal;

pub async fn feed(
    mastodon_instance: String,
    access_token: String,
    count: u32,
) -> Result<Vec<(String, String)>, megalodon::error::Error> {
    let full_instance_url = format!("https://{}/", mastodon_instance);

    let client = megalodon::generator(
        megalodon::SNS::Mastodon,
        full_instance_url,
        Some(access_token),
        None,
    );

    let options: GetTimelineOptionsWithLocal = GetTimelineOptionsWithLocal {
        only_media: None,
        limit: Some(count),
        max_id: None,
        since_id: None,
        min_id: None,
        local: None,
    };
    let res = client.get_home_timeline(Some(&options)).await?.json();

    Ok(res.iter().map(parsed_toot).collect())
}

fn parsed_toot(status: &megalodon::entities::Status) -> (String, String) {
    let mut content = {
        let with_linebreaks = status
            .reblog
            .as_ref()
            .map(|r| &r.content)
            .unwrap_or(&status.content)
            .replace("</p>", "\n");
        let mut cleared_content = String::with_capacity(with_linebreaks.len());
        let mut skipping = 0;
        for char in with_linebreaks.chars() {
            match (char, skipping) {
                ('<', _) => skipping += 1,
                ('>', _) => skipping -= 1,
                (c, s) if s == 0 => cleared_content.push(c),
                _ => (),
            }
        }
        cleared_content
    };

    let author = match &status.reblog {
        Some(reblog) => format!(
            "@{} via @{}",
            reblog.account.acct.split('@').next().unwrap(),
            status.account.acct.split('@').next().unwrap()
        ),
        None => format!("@{}", status.account.acct.split('@').next().unwrap()),
    };

    for media in &status.media_attachments {
        content.push_str(
            format!(
                "\n[img] (Alt Text:{})",
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
            0 => content.push_str(format!("\n[media] (Alt Text: No Alt Text)").as_str()),
            _ => content.push_str(format!("\n[media] (Alt Text: {})", &card.description).as_str()),
        }
    }

    (author, content)
}

#[cfg(test)]
mod test {
    use crate::download::feed;

    #[tokio::test]
    async fn test_feed() {
        let token = env!("HEFFALUMP_ACCESS_TOKEN").to_string();
        let instance = env!("HEFFALUMP_MASTADON_INST").to_string();
        for (author, content) in feed(instance, token, 100).await.unwrap() {
            println!("{}\n{}", author, content);
        }
    }
}
