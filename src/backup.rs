use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use filenamify::filenamify;
use serenity::{
    prelude::*,
    framework::standard::CommandResult,
    model::{
        id::{
            GuildId,
            ChannelId,
        },
        channel::{
            GuildChannel,
            ChannelType,
            Message,
        },
    },
    Result,
};
use reqwest::{
    header::CONTENT_TYPE,
    Url
};
use mime::{APPLICATION_OCTET_STREAM, Mime};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

const BACKUP_PATH: Option<&str> = Some("D:\\Documents");
const DIGITAL_ISLAMIC_LIBRARY_IGNORED_CHANNELS: &[u64] = &[
    866485359542140958,  // #tasawwuf (will break the bot if not ignored for some reason)

    957488150215807016,  // general-chat
    860903535146303529,  // library-quick-search
    859100525144440922,  // discussion
    1021206357266923560, // qotd
    860190358168272926,  // bruh-museum
    859279327266209852,  // applications-n-requests
    979233287002279956,  // memes
    912533380728487978,  // bots
    859281904141860914,  // list-of-sus-imposters
    912533380728487978,  // gaming
    954822380063174696,  // affiliates-network
    856072145628037160,  // roles
    856064697069862932,  // announcements
    856071493245730857,  // partnerships
];
const DIGITAL_ISLAMIC_LIBRARY_IGNORED_CHANNEL_CATEGORIES: &[u64] = &[
    859220515997614102, // informational pings
    856064816607002674, // mod chat
    859908944319479828, // server contribution
    865336860401991741, // jail
    857498139246329867, // verification
];

const URL_PATTERN: &str = r#"(?i)\b((?:https?://|www\d{0,3}[.]|[a-z0-9.\-]+[.][a-z]{2,4}/)(?:[^\s()<>]+|\(([^\s()<>]+|(\([^\s()<>]+\)))*\))+(?:\(([^\s()<>]+|(\([^\s()<>]+\)))*\)|[^\s`!()\[\]{};:'".,<>?«»“”‘’]))"#;

#[derive(Serialize, Deserialize)]
struct ServerArchive {
    id: u64,
    name: String,
    channels: Vec<ChannelArchive>,
}

#[derive(Serialize, Deserialize)]
struct ChannelArchive {
    id: u64,
    name: String,
    category: Option<CategoryArchive>,
    messages: Vec<MessageArchive>,
}

#[derive(Serialize, Deserialize)]
struct CategoryArchive {
    id: u64,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct MessageArchive {
    author: (u64, String),
    content: String,
    attachments: Vec<AttachmentArchive>,
    timestamp: chrono::NaiveDateTime,
}

#[derive(Serialize, Deserialize)]
struct AttachmentArchive {
    filename: String,
    url: String,
}

pub async fn backup_server(
    ctx: &Context,
    command_channel_id: ChannelId,
    guild_id: GuildId,
    download_attachments: bool,
) -> CommandResult {
    let server_name = guild_id.name(&ctx.cache).unwrap();
    let server_filename = filenamify(server_name.clone());

    let backup_dir = get_backup_path().join(&server_filename);
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir).expect("Failed to delete backup directory");
    }
    fs::create_dir_all(&backup_dir).expect("Failed to create backup directory");

    let attachments_dir = backup_dir.clone().join("attachments");
    if download_attachments {
        fs::create_dir(&attachments_dir).expect("Failed to create attachments directory");
    }

    println!("Copying server {}..", server_name);

    let server_archive = {
        let mut server_archive = ServerArchive {
            id: guild_id.0,
            name: server_name.clone(),
            channels: Vec::new(),
        };
        let channels = get_channels(ctx, guild_id).await;
        let channel_count = channels.len();
        let mut progress_message = command_channel_id
            .send_message(&ctx.http, |m| m.content("0% done.."))
            .await?;
        for (i, channel) in channels.into_iter().enumerate() {
            println!("Copying channel {}/{} {}", i + 1, channel_count, channel.name);
            let category_archive= match channel.parent_id {
                Some(id) => Some({
                    let category = id
                        .to_channel(&ctx).await.unwrap()
                        .category().unwrap();
                    CategoryArchive {
                        name: category.name,
                        id: category.id.0,
                    }
                }),
                None => None,
            };
            let mut channel_archive = ChannelArchive {
                name: channel.name.clone(),
                id: channel.id.0,
                category: category_archive,
                messages: Vec::new(),
            };
            let messages = match get_messages(ctx, channel.id).await {
                Ok(x) => x,
                Err(e) => { eprintln!("Failed to get channel messages: {e}"); break; }
            };
            for message in messages {
                let author = message.author;
                let mut message_archive = MessageArchive {
                    author: (author.id.0, author.name),
                    content: message.content.clone(),
                    attachments: Vec::new(),
                    timestamp: message.timestamp.naive_utc(),
                };
                for attachment in message.attachments {
                    let filename = format!("{} - {}", attachment.id.0, attachment.filename);
                    message_archive.attachments.push(AttachmentArchive {
                        filename: filename.to_string(),
                        url: attachment.url.clone(),
                    });
                    if download_attachments {
                        let bytes = ByteBuf::from(attachment
                            .download().await
                            .inspect_err(|e| eprintln!("Error while downloading attachment: {e}")).unwrap());
                        let attachment_path = attachments_dir.clone().join(filename);
                        let mut attachment_file = File::create(attachment_path).expect("Failed to create attachment file");
                        attachment_file.write_all(bytes.as_ref()).expect("Failed to write to attachment file");
                    }
                }
                if download_attachments {
                    let url_regex = Regex::new(URL_PATTERN).expect("Failed to compile regex");
                    let urls = url_regex
                        .find_iter(&message.content)
                        .filter_map(|url| Url::parse(url.as_str()).inspect_err(|e| eprintln!("Failed to parse url: {e}")).ok());
                    for url in urls {
                        let last_segment = url.path_segments().unwrap().last().unwrap().to_string();
                        let url_string = url.to_string();
                        match reqwest::get(url).await {
                            Ok(response) => {
                                let headers = response.headers();
                                if let Some(content_type) = headers.get(CONTENT_TYPE) {
                                    let content_type = content_type
                                        .to_str().unwrap()
                                        .split_ascii_whitespace()
                                        .next().expect("Invalid header")
                                        .parse::<Mime>().unwrap_or(APPLICATION_OCTET_STREAM);
                                    if content_type.subtype() == mime::PDF {
                                        match response.bytes().await {
                                            Ok(bytes) => {
                                                println!("Downloaded {}", url_string);
                                                message_archive.attachments.push(AttachmentArchive {
                                                    filename: last_segment.clone(),
                                                    url: url_string,
                                                });
                                                let attachment_path = attachments_dir.clone().join(&last_segment);
                                                match File::create(&attachment_path) {
                                                    Ok(mut attachment_file) => attachment_file.write_all(&bytes).expect("Failed to write to attachment file"),
                                                    Err(e) => eprintln!("Failed to create attachment file  {}: {e}", last_segment),
                                                }
                                            }
                                            Err(e) => eprintln!("{e}"),
                                        }
                                    }
                                }
                            }
                            Err(e) => eprintln!("{e}")
                        }
                    }
                }
                channel_archive.messages.push(message_archive);
            }
            server_archive.channels.push(channel_archive);

            println!("Successfully copied channel.");
            progress_message.edit(&ctx.http, |m|
                m.content(format!("{}% done..", 100 * i / (channel_count)))
            ).await?;
            command_channel_id.broadcast_typing(&ctx.http).await?;
        }
        progress_message.delete(ctx).await?;
        command_channel_id.send_message(&ctx.http, |m| m.content("Successfully copied server.")).await?;
        server_archive
    };

    println!("Copying to PC...");
    let mut path = backup_dir.clone().join(server_filename);
    path.set_extension("json");
    let mut file = File::create(&path).expect("Failed to create file");
    let json_string = serde_json::to_string(&server_archive).expect("Failed to parse server archive to JSON");
    file.write_all(&json_string.into_bytes()).expect("Failed to write to file");

    println!("Successfully copied {} to {}", server_name, path.to_str().unwrap());

    Ok(())
}

async fn get_channels(ctx: &Context, guild_id: GuildId) -> Vec<GuildChannel> {
    guild_id
        .channels(&ctx.http).await.expect("Failed to get channels")
        .into_values()
        .filter(|c| c.kind == ChannelType::Text || c.kind == ChannelType::PublicThread)
        .filter(|c| !DIGITAL_ISLAMIC_LIBRARY_IGNORED_CHANNELS
            .contains(&c.id.0))
        .filter(|c| match c.parent_id {
            Some(id) => !DIGITAL_ISLAMIC_LIBRARY_IGNORED_CHANNEL_CATEGORIES.contains(&id.0),
            None => true,
        })
        .collect::<Vec<_>>()
}

async fn get_messages(ctx: &Context, channel_id: ChannelId) -> Result<Vec<Message>> {
    const PAGES: usize = 5;
    let mut messages = channel_id
        .messages(&ctx.http, |retriever| retriever.limit(100))
        .await?;
    for _ in 0..PAGES {
        let last = messages.last();
        if let Some(last) = last {
            let mut next_messages = channel_id
                .messages(&ctx.http, |retriever|
                    retriever.before(last).limit(100))
                .await?;
            if next_messages.is_empty() {
                break;
            }
            messages.append(&mut next_messages);
        }
    }
    Ok(messages)
}

fn get_backup_path() -> PathBuf {
    if let Some(backup_path) = BACKUP_PATH {
        PathBuf::from(backup_path)
    } else {
        dirs::document_dir().expect("Failed to get document directory")
    }
}