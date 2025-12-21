use std::{sync::LazyLock, thread::sleep, time::Duration};

use serde::Deserialize;
use serenity::all::{
    Activity, ActivityData, ClientBuilder, Command, CommandInteraction, CommandOptionType,
    CommandType, Context, CreateCommand, CreateCommandOption, CreateInteractionResponseMessage,
    EventHandler, GatewayIntents, Interaction, Ready,
};

#[derive(Deserialize)]
struct Config {
    token: String,
    services: Vec<String>,
}

const SERVICE_ACTIONS: [&str; 3] = ["start", "stop", "restart"];
struct Handler {}

#[cfg(not(debug_assertions))]
async fn run_cmd(action: &str, service: &str) -> Result<String, String> {
    match tokio::process::Command::new("systemctl")
        .args([action, service])
        .output()
        .await
    {
        Ok(_) => Ok(String::from("Success!")),
        Err(v) => Err(format!("error occurred: `{}`", v)),
    }
}
#[cfg(debug_assertions)]
async fn run_cmd(action: &str, service: &str) -> Result<String, String> {
    Ok(format!("action: {action}, service: {service}"))
}
/// returns `(action, service)`
fn get_args(cmd: &CommandInteraction) -> Option<(&str, &str)> {
    cmd.data
        .options
        .iter()
        .find(|e| e.name == "action")
        .zip(cmd.data.options.iter().find(|e| e.name == "service"))
        .map(|e| (e.0.value.as_str().unwrap(), e.1.value.as_str().unwrap()))
}

fn check_status() -> String {
    use std::fmt::Write;
    let mut out = String::new();
    out.clear();
    for service in CFG.services.iter() {
        let status = std::process::Command::new("systemctl")
            .args(["is-active", service])
            .status()
            .map(|e| e.success())
            .unwrap_or(false);

        let _ = writeln!(
            &mut out,
            "{service}: {}",
            if status { "up" } else { "down" }
        );
    }

    out
}

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _ready: Ready) {
        Command::create_global_command(
            ctx.clone(),
            CreateCommand::new("service")
                .add_option({
                    let mut opt = CreateCommandOption::new(
                        CommandOptionType::String,
                        "action",
                        "what to do with the service",
                    )
                    .required(true);
                    for choice in SERVICE_ACTIONS {
                        opt = opt.add_string_choice(choice, choice)
                    }
                    opt
                })
                .add_option({
                    let mut opt = CreateCommandOption::new(
                        CommandOptionType::String,
                        "service",
                        "the service in question",
                    )
                    .required(true);
                    for i in CFG.services.iter() {
                        opt = opt.add_string_choice(i, i);
                    }
                    opt
                })
                .description("manages server services")
                .kind(CommandType::ChatInput),
        )
        .await
        .expect("failed to create command");
        Command::create_global_command(
            ctx.clone(),
            CreateCommand::new("status")
                .description("list statuses")
                .kind(CommandType::ChatInput),
        )
        .await
        .expect("failed to create command");
    }

    async fn interaction_create(
        &self,
        ctx: serenity::prelude::Context,
        interaction: serenity::all::Interaction,
    ) {
        match interaction {
            // serenity::all::InteractionType::Ping => todo!(),
            Interaction::Command(cmd) => match cmd.data.name.as_str() {
                "service" => {
                    let _ = match async {
                        let Some((action, service)) = get_args(&cmd) else {
                            return Err("Invalid args".to_string());
                        };
                        if !SERVICE_ACTIONS.contains(&action)
                            || !CFG.services.iter().any(|e| e == service)
                        {
                            return Err("Invalid service or action".to_string());
                        };

                        run_cmd(action, service).await
                    }
                    .await
                    {
                        Ok(res) | Err(res) => {
                            cmd.create_response(
                                ctx,
                                serenity::all::CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new().content(res),
                                ),
                            )
                            .await
                        }
                    };
                }
                "status" => {
                    let _ = cmd
                        .create_response(
                            ctx,
                            serenity::all::CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(check_status())
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                }
                _ => (),
            },
            // serenity::all::InteractionType::Component => todo!(),
            // serenity::all::InteractionType::Autocomplete => todo!(),
            // serenity::all::InteractionType::Modal => todo!(),
            // serenity::all::InteractionType::Unknown(_) => todo!(),
            _ => (),
        }
    }
}

static CFG: LazyLock<Config> = LazyLock::new(|| {
    toml::from_str::<Config>(
        &std::fs::read_to_string(if cfg!(debug_assertions) {
            "dev.config.toml"
        } else {
            "config.toml"
        })
        .unwrap(),
    )
    .unwrap()
});

#[tokio::main]
async fn main() {
    let mut client = ClientBuilder::new(&CFG.token, GatewayIntents::non_privileged())
        .event_handler(Handler {})
        .await
        .unwrap();

    client.start().await.unwrap();
}
