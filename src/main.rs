use std::{
    collections::HashMap,
    fs::File,
    io::Write,
    sync::{LazyLock, RwLock},
};

use serde::{Deserialize, Serialize};
use serenity::all::{
    ClientBuilder, Command, CommandInteraction, CommandOptionType, CommandType, Context,
    CreateAutocompleteResponse, CreateCommand, CreateCommandOption,
    CreateInteractionResponseMessage, EventHandler, GatewayIntents, Interaction, Ready,
};

#[derive(Deserialize)]
struct Config {
    token: String,
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
        Ok(_) => Ok(format!("Successfully ran `/service {action} {service}`")),
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

fn allowed_service(id: u64, service: &str) -> bool {
    let lock = SERVERS_CFG.read().unwrap();
    // dbg!(id, &lock, service);
    lock.servers_services
        .get(&id)
        .is_some_and(|e| e.iter().any(|e| e == service))
}

fn check_status(guild_id: u64) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let lock = SERVERS_CFG.read().unwrap();
    let Some(services) = lock.servers_services.get(&guild_id) else {
        return String::new();
    };
    for service in services {
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
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "service",
                        "the service in question",
                    )
                    .required(true)
                    .set_autocomplete(true),
                )
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

        Command::create_global_command(
            &ctx,
            CreateCommand::new("add_service")
                .kind(CommandType::ChatInput)
                .description("admin thing")
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "service",
                        "which service to add",
                    )
                    .required(true),
                ),
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
            Interaction::Autocomplete(cmd) => {
                if cmd.data.autocomplete().unwrap().name == "service" {
                    let _ = cmd
                        .create_response(
                            &ctx,
                            serenity::all::CreateInteractionResponse::Autocomplete({
                                let mut opts = CreateAutocompleteResponse::new();
                                let lock = SERVERS_CFG.read().unwrap();
                                let Some(services) =
                                    lock.servers_services.get(&cmd.guild_id.unwrap().get())
                                else {
                                    return;
                                };
                                for service in services {
                                    opts = opts.add_string_choice(service, service);
                                }

                                opts
                            }),
                        )
                        .await;
                }
            }

            // serenity::all::InteractionType::Ping => todo!(),
            Interaction::Command(cmd) => match cmd.data.name.as_str() {
                "service" => {
                    let _ = match async {
                        let Some((action, service)) = get_args(&cmd) else {
                            return Err("Invalid args".to_string());
                        };
                        if !(SERVICE_ACTIONS.contains(&action)
                            && allowed_service(cmd.guild_id.unwrap().get(), service))
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
                                    .content(check_status(cmd.guild_id.unwrap().get()))
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                }
                "add_service" => {
                    if cmd.user
                        != ctx
                            .http
                            .get_current_application_info()
                            .await
                            .unwrap()
                            .owner
                            .unwrap()
                    {
                        return;
                    }
                    let Some(service) = cmd
                        .data
                        .options
                        .first()
                        .and_then(|e| e.value.as_str().filter(|_| e.name == "service"))
                    else {
                        return;
                    };

                    let guild_id = cmd.guild_id.unwrap().get();
                    {
                        let mut lock = SERVERS_CFG.write().unwrap();

                        lock.servers_services
                            .entry(guild_id)
                            .or_default()
                            .push(service.to_string());
                    }
                    {
                        let mut path = String::from(server_cfg_location());
                        path += ".tmp";
                        let mut file = File::create(&path).unwrap();

                        file.write_all(
                            toml::to_string_pretty(&*SERVERS_CFG.read().unwrap())
                                .unwrap()
                                .as_bytes(),
                        )
                        .unwrap();
                        drop(file);
                        std::fs::rename(&path, server_cfg_location()).unwrap();
                    }

                    let _ = cmd
                        .create_response(
                            ctx,
                            serenity::all::CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(format!(
                                        "added {service} to allowed services for this server"
                                    ))
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

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
struct ServersCfg {
    servers_services: HashMap<u64, Vec<String>>,
}

const fn server_cfg_location() -> &'static str {
    if cfg!(debug_assertions) {
        "dev.servers.toml"
    } else {
        "servers.toml"
    }
}
static SERVERS_CFG: LazyLock<RwLock<ServersCfg>> = LazyLock::new(|| {
    toml::from_str::<ServersCfg>(
        &std::fs::read_to_string(server_cfg_location()).unwrap_or_default(),
    )
    .unwrap_or_default()
    .into()
});

#[tokio::main]
async fn main() {
    let mut client = ClientBuilder::new(&CFG.token, GatewayIntents::non_privileged())
        .event_handler(Handler {})
        .await
        .unwrap();

    client.start().await.unwrap();
}
