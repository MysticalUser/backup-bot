#![feature(result_option_inspect)]
#![feature(write_all_vectored)]
#![feature(async_closure)]
#![feature(is_some_with)]

// TODO: Recovery from backup

use serenity::{
    prelude::*,
    async_trait,
    framework::{
        standard::{macros::group, CommandResult},
        StandardFramework
    },
    model::{
        id::{
            GuildId,
            ChannelId,
        },
        channel::{
            ChannelType,
            Message
        },
        gateway::Ready,
        application::{
            command::{Command, CommandOptionType},
            interaction::{
                Interaction,
                InteractionResponseType,
                application_command::CommandDataOption
            }
        },
    },
    Result,
    json::Value,
    model::channel::GuildChannel
};
use std::{
    fs::{
        self,
        create_dir,
        File
    },
    io::Write,
    path::PathBuf,
};
use reqwest::{
    header::CONTENT_TYPE,
    Url
};
use filenamify::filenamify;
use mime::{APPLICATION_OCTET_STREAM, Mime};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use tokio::fs::create_dir_all;

static DOWNLOAD_ATTACHMENTS_BY_DEFAULT: bool = false;
const BOT_TOKEN: &str = "MTAyMzA2MDEwMDgyMDI1MDY5NQ.GVNXWS.OMr6zDeZrP136Vp7bKRNXeVR1uF5dgX0l5bBqU";
const BACKUP_PATH: Option<&str> = Some("D:\\Documents");
const CLOUD_BACKUP_PATH: &str = "D:\\MEGAsync";
const DIGITAL_ISLAMIC_LIBRARY_IGNORED_CHANNELS: &[u64] = &[
    866485359542140958, // #tasawwuf (will break the bot if not ignored for some reason)

    859279327266209852, // #applications-n-requests
    979233287002279956, // #memes
    912533380728487978, // #bots
    859281904141860914, // #list-of-sus-imposters
    912533380728487978, // #gayming
    954822380063174696, // #affialiates-network
    856072145628037160, // #roles
    856064697069862932, // #announcements
    856071493245730857, // #partnerships
];
const DIGITAL_ISLAMIC_LIBRARY_IGNORED_CHANNEL_CATEGORIES: &[u64] = &[
    859220515997614102, // informational pings
    856064816607002674, // mod chat
    859908944319479828, // server contribution
    865336860401991741, // jail
    857498139246329867, // verification
];

const URL_PATTERN: &str = r#"(?i)\b((?:https?://|www\d{0,3}[.]|[a-z0-9.\-]+[.][a-z]{2,4}/)(?:[^\s()<>]+|\(([^\s()<>]+|(\([^\s()<>]+\)))*\))+(?:\(([^\s()<>]+|(\([^\s()<>]+\)))*\)|[^\s`!()\[\]{};:'".,<>?«»“”‘’]))"#;

#[group]
struct General;
struct Handler;

#[derive(Serialize, Deserialize)]
struct ChannelArchive {
    name: String,
    id: u64,
    category: CategoryArchive,
    messages: Vec<MessageArchive>,
}
#[derive(Serialize, Deserialize)]
struct CategoryArchive {
    name: String,
    id: u64,
}
#[derive(Serialize, Deserialize)]
struct MessageArchive {
    content: String,
    attachments: Vec<AttachmentArchive>,
    author_id: u64,
    timestamp: chrono::NaiveDateTime,
}
#[derive(Serialize, Deserialize)]
struct AttachmentArchive {
    filename: String,
    url: String,
    bytes: Option<ByteBuf>,
}

struct BackupOptions {
    download_attachments: bool,
    cloud_backup: bool,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        Command::create_global_application_command(&ctx.http, |command| {
            command
                .name("backup-all")
                .description("Backs up all channels of the server.")
                .create_option(|option| {
                    option
                        .name("download-attachments")
                        .description("If true, backs up channel attachments.")
                        .kind(CommandOptionType::Boolean)
                })
                .create_option(|option| {
                    option
                        .name("cloud-backup")
                        .description("If true, backs up to cloud.")
                        .kind(CommandOptionType::Boolean)
                })
        }).await.expect("Failed to created global slash command");

        //Command::create_global_application_command(&ctx.http, |command| {
        //    command
        //        .name("recover")
        //        .description("Recovers server from backup. WARNING: Will create channels in bulk.")
        //}).await.expect("Failed to created global slash command");
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        fn get_bool_option(
            command_options: &[CommandDataOption],
            option_index: usize,
            default_value: bool
        ) -> bool {
            command_options
                .get(option_index)
                .and_then(|x| x.clone().value)
                .map(|x| match x { Value::Bool(x) => x, _ => panic!("Unexpected object") })
                .unwrap_or(default_value)
        }

        if let Interaction::ApplicationCommand(command) = interaction {
            let command_channel_id = command.channel_id;
            let guild_id = command.guild_id.expect("Failed to get guild");
            let guild_name = guild_id.name(&ctx).expect("Failed to get guild name");

            match command.data.name.as_str() {
                "backup-all" => {
                    command.create_interaction_response(&ctx.http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            .interaction_response_data(|m| m.content("Starting copy.."))
                    }).await.expect("Failed to respond to slash command");

                    let download_attachments = get_bool_option(
                        &command.data.options,
                        0,
                        DOWNLOAD_ATTACHMENTS_BY_DEFAULT,
                    );
                    let cloud_backup = get_bool_option(
                        &command.data.options,
                        1,
                        false,
                    );
                    let backup_options = BackupOptions {
                        download_attachments,
                        cloud_backup,
                    };

                    println!("Copying server {}..", guild_name);
                    backup_server(
                        &ctx,
                        command_channel_id,
                        guild_id,
                        backup_options,
                    ).await.expect("Server backup failed");
                    println!("Successfully copied server {}", guild_name);

                    command_channel_id.send_message(ctx.http, |m|
                        m.content("Successfully copied server.")
                    ).await.expect("Failed to send success message");
                }
                _ => panic!("Command not implemented"),
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let framework = StandardFramework::new()
        .group(&GENERAL_GROUP);

    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(BOT_TOKEN, intents)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Failed to connect to client");

    client
        .start()
        .await
        .expect("Client error")
}

async fn backup_server(
    ctx: &Context,
    command_channel_id: ChannelId,
    guild_id: GuildId,
    backup_options: BackupOptions,
) -> CommandResult {
    async fn to_message_archive(message: Message, download_attachments: bool) -> MessageArchive {
        let mut message_archive = MessageArchive {
            content: message.content,
            attachments: Vec::new(),
            timestamp: message.timestamp.naive_utc(),
            author_id: message.author.id.0,
        };
        for attachment in message.attachments {
            let bytes = if download_attachments {
                Some(ByteBuf::from(attachment
                    .download().await
                    .inspect_err(|e| eprintln!("Error while downloading attachment: {e}")).unwrap()))
            } else {
                None
            };
            message_archive.attachments.push(AttachmentArchive {
                filename: attachment.filename,
                url: attachment.url,
                bytes,
            });
        }
        message_archive
    }
    async fn to_channel_archive(ctx: &Context, channel: GuildChannel, download_attachments: bool) -> ChannelArchive {
        let category = channel.parent_id
            .expect("Channel has no parent category")
            .to_channel(&ctx).await.unwrap()
            .category().unwrap();
        let category_archive = CategoryArchive {
            name: category.name,
            id: category.id.0,
        };
        let mut channel_archive = ChannelArchive {
            name: channel.name.clone(),
            id: channel.id.0,
            category: category_archive,
            messages: Vec::new(),
        };
        for message in get_messages(ctx, channel.id).await.expect("Failed to get channel messages") {
            channel_archive.messages.push(to_message_archive(message, download_attachments).await);
        }
        channel_archive
    }
    async fn get_channels(ctx: &Context, guild_id: GuildId) -> Vec<GuildChannel> {
        guild_id
            .channels(&ctx.http).await.expect("Failed to get channels")
            .into_values()
            .filter(|c| c.kind == ChannelType::Text)
            .filter(|c| !DIGITAL_ISLAMIC_LIBRARY_IGNORED_CHANNELS
                .contains(&c.id.0))
            .filter(|c| !DIGITAL_ISLAMIC_LIBRARY_IGNORED_CHANNEL_CATEGORIES
                .contains(&c.parent_id.expect("Not in category").0))
            .collect::<Vec<_>>()
    }
    async fn create_or_append_message(
        ctx: &Context,
        channel_id: ChannelId,
        msg: &mut Option<Message>,
        s: &str
    ) -> Result<()> {
        if let Some(msg) = msg {
            let new_content = format!("{}\n{}", &msg.content, &s);
            msg.edit(ctx, |m| m.content(new_content)).await
        } else {
            channel_id.send_message(&ctx.http, |m| m.content(s))
                .await
                .map(|new_msg| { *msg = Some(new_msg); })
        }
    }

    let guild_name = guild_id.name(&ctx.cache).expect("Failed to get guild name");

    let server_dir = {
        let mut path = if backup_options.cloud_backup {
            PathBuf::from(CLOUD_BACKUP_PATH)
        } else {
            get_local_backup_path()
        };
        path.push(filenamify(guild_name));
        if path.exists() {
            fs::remove_dir_all(&path).expect("Failed to delete backup directory");
        }
        create_dir(&path).expect("Failed to create backup directory");
        path
    };

    let channels = get_channels(ctx, guild_id).await;
    let channel_count = channels.len();
    let mut progress_message = command_channel_id
        .send_message(&ctx.http, |m| m.content("0% done.."))
        .await?;
    for (i, channel) in channels.into_iter().enumerate() {
        let channel_archive = to_channel_archive(ctx, channel, backup_options.download_attachments).await;

        println!("Copying channel {}/{} {}", i + 1, channel_count, channel_archive.name);

        let channel_dir = {
            let mut path = server_dir.clone();
            path.push(filenamify(channel_archive.category.name));
            path.push(filenamify(channel_archive.name));
            create_dir_all(&path).await.unwrap();
            path
        };
        let attachments_dir = {
            let mut path = channel_dir.clone();
            path.push("attachments");
            if backup_options.download_attachments {
                create_dir(path.clone()).unwrap();
            }
            path
        };
        let messages_path = {
            let mut path = channel_dir.clone();
            path.push("messages.txt");
            path
        };
        let mut channel_file = File::create(messages_path).expect("Failed to open backup file");

        const MESSAGE_SEPARATOR: &str = "---------------------------------------------------------\n\n";
        let mut write = |s: &str| channel_file
            .write_all(s.as_bytes())
            .expect("Failed to write to backup file");

        let mut prev_author_id = None;
        let mut prev_timestamp = None;
        for message in channel_archive.messages.into_iter().rev() {
            let cur_author_id = message.author_id;
            let cur_timestamp = message.timestamp;
            if let (Some(prev_author_id), Some(prev_timestamp)) = (prev_author_id, prev_timestamp) {
                let duration = cur_timestamp.signed_duration_since(prev_timestamp);
                if cur_author_id != prev_author_id || duration.num_hours() >= 1 {
                    write(MESSAGE_SEPARATOR);
                }
            }
            prev_author_id = Some(cur_author_id);
            prev_timestamp = Some(cur_timestamp);

            write(&message.content);
            write("\n");

            if backup_options.download_attachments {
                for attachment in message.attachments {
                    write(&attachment.url);
                    write("\n");

                    if let Some(bytes) = attachment.bytes {
                        let attachment_path = {
                            let mut path = attachments_dir.clone();
                            path.push(attachment.filename);
                            path
                        };
                        let mut attachment_file = File::create(attachment_path).expect("Failed to create attachment file");
                        attachment_file.write_all(bytes.as_ref()).expect("Failed to write to attachment file");
                    }
                }

                let url_regex = Regex::new(URL_PATTERN)?;
                let urls = url_regex
                    .find_iter(&message.content)
                    .filter_map(|url| Url::parse(url.as_str()).inspect_err(|e| eprintln!("Failed to parse url: {e}")).ok());
                for url in urls {
                    let last_segment = url.path_segments().unwrap().last().unwrap().to_string();
                    let domain = url.domain().unwrap().to_string();
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
                                    let attachment_path_1 = {
                                        let mut path = attachments_dir.clone();
                                        path.push(filenamify(last_segment.clone()));
                                        path
                                    };
                                    let attachment_path_2 = {
                                        let mut path = attachments_dir.clone();
                                        path.push(filenamify(domain.clone()));
                                        path
                                    };
                                    let mut attachment_file = match File::create(attachment_path_1) {
                                        Ok(file) => file,
                                        Err(_) => match File::create(attachment_path_2) {
                                            Ok(file) => file,
                                            Err(e) => panic!("Failed to create attachment file: {}", e),
                                        }
                                    };
                                    match response.bytes().await {
                                        Ok(bytes) => {
                                            println!("Downloaded {}", url_string);
                                            attachment_file.write_all(&bytes).expect("Failed to write to attachment file");
                                        }
                                        Err(e) => eprintln!("{e}")
                                    }
                                }
                            }
                        }
                        Err(e) => eprintln!("{e}")
                    }
                }
            }

            write("\n");
        }

        progress_message.edit(&ctx.http, |m|
            m.content(format!("{}% done..", 100 * i / (channel_count)))
        ).await?;
        println!("Successfully copied channel.");
        command_channel_id.broadcast_typing(&ctx.http).await?;
    }
    progress_message.delete(ctx).await?;

    Ok(())
}

async fn get_messages(ctx: &Context, channel_id: ChannelId) -> Result<Vec<Message>> {
    let mut messages = channel_id
        .messages(&ctx.http, |retriever| retriever.limit(100))
        .await?;
    while messages.len() == 100 {
        if let Some(last) = messages.last() {
            let mut next_messages = channel_id
                .messages(&ctx.http, |retriever|
                    retriever.before(last).limit(100))
                .await?;
            messages.append(&mut next_messages);
        }
    }
    Ok(messages)
}

fn get_local_backup_path() -> PathBuf {
    if let Some(backup_path) = BACKUP_PATH {
        PathBuf::from(backup_path)
    } else {
        dirs::document_dir().expect("Failed to get document directory")
    }
}