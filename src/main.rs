#![feature(result_option_inspect)]
#![feature(write_all_vectored)]
#![feature(async_closure)]
#![feature(is_some_with)]

// TODO: Recovery from backup

mod backup;

use serenity::{
    prelude::*,
    async_trait,
    framework::{
        standard::macros::group,
        StandardFramework
    },
    model::{
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
    json::Value,
};
use crate::backup::backup_server;

#[group]
struct General;
struct Handler;

#[tokio::main]
async fn main() {
    let framework = StandardFramework::new()
        .group(&GENERAL_GROUP);
    let bot_token = std::env::args().nth(1).expect("Missing bot token argument");
    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(bot_token, intents)
        .event_handler(Handler)
        .framework(framework)
        .await
        .expect("Failed to connect to client");

    client
        .start()
        .await
        .expect("Client error")
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
                        false,
                    );

                    backup_server(
                        &ctx,
                        command_channel_id,
                        guild_id,
                        download_attachments,
                    ).await.expect("Server backup failed");
                }
                _ => panic!("Command not implemented"),
            }
        }
    }
}