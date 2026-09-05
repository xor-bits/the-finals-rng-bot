use std::{env, sync::Arc};

use eyre::Result;
use rand::{RngExt, rngs::ThreadRng};
use serenity::{
    Client,
    all::{
        Command, Context, CreateInteractionResponse, CreateInteractionResponseMessage,
        EventHandler, GatewayIntents, Interaction, Ready,
    },
    async_trait,
};
use tokio::{signal, sync::Mutex};
use vello_common::paint::ImageId;

use self::{
    data::{Dataset, load_dataset},
    renderer::Renderer,
};

pub mod cmd;
pub mod data;
pub mod renderer;

pub struct Loadout<'a> {
    pub name: &'a str,
    pub special: ImageId,
    pub weapon: ImageId,
    pub gadgets: [ImageId; 3],
}

impl<'a> Loadout<'a> {
    pub fn pick(rng: &mut ThreadRng, name: &'a str, dataset: &data::Dataset) -> Self {
        let class = rng.random_range(0..3);
        let class = &dataset.classes[class];

        let special = rng.random_range(0..class.specials.len());
        let special = class.specials[special];

        let weapon = rng.random_range(0..class.weapons.len());
        let weapon = class.weapons[weapon];

        let gadget_0 = rng.random_range(0..class.gadgets.len());
        let mut gadget_1 = rng.random_range(0..class.gadgets.len() - 1);
        let mut gadget_2 = rng.random_range(0..class.gadgets.len() - 2);

        // messy `class.gadgets.sample(rng, 3)` without allocations or unpredictable loops
        // fixes duplicates without biased picks
        if gadget_1 >= gadget_0 {
            gadget_1 += 1;
        }
        if gadget_2 >= std::cmp::min(gadget_0, gadget_1) {
            gadget_2 += 1;
        }
        if gadget_2 >= std::cmp::max(gadget_0, gadget_1) {
            gadget_2 += 1;
        }

        let gadgets = [
            class.gadgets[gadget_0],
            class.gadgets[gadget_1],
            class.gadgets[gadget_2],
        ];

        Self {
            name,
            special,
            weapon,
            gadgets,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Mode {
    Duos,
    Trios,
    Quads,
}

impl Mode {
    pub const fn teams(self) -> usize {
        match self {
            Mode::Duos => 2,
            Mode::Trios => 3,
            Mode::Quads => 4,
        }
    }
}

pub struct Handler {
    renderer: Mutex<Renderer>,
    dataset: Dataset,
}

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Interaction::Command(cmd) = interaction else {
            return;
        };

        let result: Result<_, String> = match cmd.data.name.as_str() {
            "start" => cmd::start::run(self, &ctx, &cmd).await,
            _ => Err("???".to_string()),
        };

        let msg = match result {
            Ok(msg) => msg,
            Err(msg) => CreateInteractionResponseMessage::new().content(msg),
        };
        let response = CreateInteractionResponse::Message(msg);
        if let Err(err) = cmd.create_response(&ctx.http, response).await {
            eprintln!("failed to respond to a command: {err}");
        }
    }

    async fn ready(&self, ctx: Context, data_about_bot: Ready) {
        eprintln!("{} is connected", data_about_bot.user.name);

        let old_commands = ctx.http.get_global_commands().await.unwrap_or_else(|err| {
            eprintln!("failed to get global commands: {err}");
            Vec::new()
        });
        for old_command in old_commands.iter() {
            if ["start"].contains(&old_command.name.as_str()) {
                continue;
            }

            eprintln!("deleting old command: {}", old_command.name);
            if let Err(err) = ctx.http.delete_global_command(old_command.id).await {
                eprintln!("failed to delete old global command: {err}");
            }
        }

        for (name, cmd) in [("start", cmd::start::register())] {
            if old_commands
                .iter()
                .map(|c| c.name.as_str())
                .any(|old| old == name)
            {
                eprintln!("command {name} already registered");
                continue;
            }

            if let Err(err) = Command::create_global_command(&ctx.http, cmd).await {
                eprintln!("failed to create new global command: {err}");
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let token = env::var("TOKEN")?;

    let mut renderer = Renderer::new()?;
    let dataset = load_dataset(&mut renderer.res).await?;

    let handler = Arc::new(Handler {
        renderer: Mutex::new(renderer),
        dataset,
    });

    let intents = GatewayIntents::empty();
    let mut client = Client::builder(&token, intents)
        .event_handler_arc(handler)
        .await?;

    tokio::select! {
        r = client.start() => r?,
        r = signal::ctrl_c() => r?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use vello_cpu::Resources;

    use crate::{Loadout, data::load_dataset};

    #[tokio::test]
    async fn no_dupes() {
        let mut res = Resources::new();
        let dataset = load_dataset(&mut res).await.unwrap();

        let mut rng = rand::rng();
        for _ in 0..100000 {
            let loadout = Loadout::pick(&mut rng, "player", &dataset);

            assert_ne!(loadout.gadgets[0], loadout.gadgets[1]);
            assert_ne!(loadout.gadgets[0], loadout.gadgets[2]);
            assert_ne!(loadout.gadgets[1], loadout.gadgets[2]);
        }
    }
}
