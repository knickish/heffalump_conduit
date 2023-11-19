use log::{debug, info};
use megalodon::megalodon::AppInputOptions;
use std::{
    io::{stdin, Error, ErrorKind::Other},
    path::Path,
};
use winapi::um::consoleapi;

use crate::MASTODON_APP_NAME;

pub async fn configure(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    unsafe { consoleapi::AllocConsole() };
    println!("On what instance is your mastodon account? (e.g. mastodon.social, hachyderm.io)");
    let mut instance = String::new();
    stdin().read_line(&mut instance).map_err(Box::new)?;

    let full_instance_url = format!("https://{}/", &instance);
    let unauthenticated = megalodon::generator(
        megalodon::SNS::Mastodon,
        full_instance_url.clone(),
        None,
        Some(String::from(MASTODON_APP_NAME)),
    );

    let options = AppInputOptions {
        ..Default::default()
    };
    let app_data = unauthenticated
        .register_app(String::from(MASTODON_APP_NAME), &options)
        .await
        .map_err(Box::new)?;
    let url = app_data.url.clone().ok_or_else(|| {
        Box::new(Error::new(
            Other,
            "Failed to retrieve URL for app registration",
        ))
    })?;
    info!("Attempting to open {} for app registration.", &url);
    open::that(url).map_err(Box::new)?; // open a browser to log in and retrieve token

    println!("Please paste (ctrl + v) the authorization code generated for Heffalump:");
    let mut input = String::new();
    stdin().read_line(&mut input).map_err(Box::new)?;
    unsafe { winapi::um::wincon::FreeConsole() };
    debug!("{}", input.trim_end().to_string());

    let token_data = unauthenticated
        .fetch_access_token(
            app_data.client_id,
            app_data.client_secret,
            input.trim_end().to_string(),
            megalodon::default::NO_REDIRECT.to_string(),
        )
        .await
        .map_err(Box::new)?;

    debug!("fetched access token");

    let authenticated = megalodon::generator(
        megalodon::SNS::Mastodon,
        full_instance_url,
        Some(token_data.access_token.clone()),
        Some(String::from(MASTODON_APP_NAME)),
    );

    debug!("generated authenticated client");

    authenticated
        .verify_account_credentials()
        .await
        .map_err(Box::new)?;

    debug!("verified authenticated client");

    let to_ser = (instance, token_data.access_token);
    let serialized = serde_json::to_string(&to_ser).map_err(Box::new)?;
    std::fs::write(path, serialized).map_err(Box::new)?;

    info!("Completed writing credentials");
    Ok(())
}
