use std::{env, sync::Arc};

use eyre::Result;
use rand::{RngExt, rngs::ThreadRng, seq::SliceRandom};
use serde::Deserialize;
use serenity::{
    Client,
    all::{
        Command, Context, CreateInteractionResponse, CreateInteractionResponseMessage,
        EventHandler, GatewayIntents, Interaction, Ready,
    },
    async_trait,
};
use tokio::{signal, sync::Mutex};

use self::{
    data::{Dataset, Item, load_dataset},
    renderer::Renderer,
};

pub mod cmd;
pub mod data;
pub mod renderer;

#[derive(Clone, Copy)]
pub enum LoadoutBias {
    Fair,
    NoHeavy,
    OnlyHeavy,
}

#[derive(Default, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum Gamemode {
    #[default]
    Any,
    Melee,
    Shotgun,
    Auto,
    Semi,
    Bombs,
}

pub struct Loadout<'a> {
    pub name: &'a str,
    pub special: Item,
    pub weapon: Item,
    pub gadgets: [Item; 3],
}

impl<'a> Loadout<'a> {
    pub fn null() -> Self {
        Self {
            name: "",
            special: Item::null(),
            weapon: Item::null(),
            gadgets: [Item::null(); 3],
        }
    }

    pub fn pick(
        rng: &mut ThreadRng,
        name: &'a str,
        dataset: &data::Dataset,
        bias: LoadoutBias,
        mode: Gamemode,
    ) -> Self {
        let mut class_low = 0;
        let mut class_high = 2;
        match bias {
            LoadoutBias::NoHeavy => {
                class_high = 1;
            }
            LoadoutBias::OnlyHeavy => {
                class_low = 2;
                class_high = 2;
            }
            _ => {}
        }
        match mode {
            Gamemode::Bombs
                // Light has no explosive weapons
                if class_low == 0 => {
                    class_low = 1;
                }
            _ => {}
        }

        let class = rng.random_range(class_low..=class_high);
        let class_special = &dataset.class(Gamemode::Any)[class];
        let class_weapon = &dataset.class(mode)[class];
        let class_gadget = if mode == Gamemode::Bombs {
            class_weapon
        } else {
            class_special
        };

        let special = rng.random_range(0..class_special.specials.len());
        let special = class_special.specials[special];

        let weapon = rng.random_range(0..class_weapon.weapons.len());
        let weapon = class_weapon.weapons[weapon];

        let gadget_0 = rng.random_range(0..class_gadget.gadgets.len());
        let mut gadget_1 = rng.random_range(0..class_gadget.gadgets.len() - 1);
        let mut gadget_2 = rng.random_range(0..class_gadget.gadgets.len() - 2);

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
            class_gadget.gadgets[gadget_0],
            class_gadget.gadgets[gadget_1],
            class_gadget.gadgets[gadget_2],
        ];

        Self {
            name,
            special,
            weapon,
            gadgets,
        }
    }
}

pub struct Teams<'a> {
    large_team_players: u8,
    large_teams: u8,
    normal_team_players: u8,
    // normal_teams: u8,
    players: u8,
    loadouts: [Loadout<'a>; 16],
}

impl<'a> Teams<'a> {
    pub fn pick(
        rng: &mut ThreadRng,
        players: &'a str,
        dataset: &Dataset,
        teams: u8,
        auto_balance: bool,
        mode: Gamemode,
    ) -> Self {
        let mut player_iter = players.split(',');

        let mut players = 0u8;
        let mut names = [(); 16].map(|_| {
            let name = player_iter
                .next()
                .inspect(|_| {
                    players += 1;
                })
                .unwrap_or("?")
                .trim();
            &name[0..name.len().min(30)]
        });
        names[0..players as usize].shuffle(rng);

        let normal_team_players = players / teams;
        let large_team_players = normal_team_players + 1;
        let large_teams = players % teams;
        let normal_teams = teams - large_teams;

        assert_eq!(
            normal_team_players * normal_teams + large_team_players * large_teams,
            players
        );
        assert_eq!(normal_teams + large_teams, teams);

        let mut i = 0u8;
        let loadouts = names.map(|name| {
            let bias = if auto_balance && i < large_teams * large_team_players {
                LoadoutBias::NoHeavy
            } else if auto_balance && large_teams != 0 {
                LoadoutBias::OnlyHeavy
            } else {
                LoadoutBias::Fair
            };
            i += 1;
            Loadout::pick(rng, name, dataset, bias, mode)
        });

        Teams {
            large_team_players,
            large_teams,
            normal_team_players,
            // normal_teams,
            players,
            loadouts,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &'_ [Loadout<'_>]> {
        let unfair_fair_split = (self.large_teams * self.large_team_players) as usize;
        let players = self.players as usize;

        let large_team_players = &self.loadouts[0..unfair_fair_split];
        let normal_team_players = &self.loadouts[unfair_fair_split..players];

        large_team_players
            .chunks(self.large_team_players as _)
            .chain(normal_team_players.chunks_exact(self.normal_team_players as _))
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
    let test = env::var("TEST").is_ok();
    let token = if test {
        String::new()
    } else {
        env::var("TOKEN")?
    };

    let mut renderer = Renderer::new()?;
    let dataset = load_dataset(&mut renderer.res).await?;

    if test {
        let mut rng = rand::rng();
        let players = "player 1,player 2,player 3,player 4,player 5,player 6,player 7";
        let teams = Teams::pick(&mut rng, players, &dataset, 3, true, Gamemode::Melee);

        for (i, team) in teams.iter().enumerate() {
            let png = renderer.render(team, i)?;
            std::fs::write(format!("team{i}.png"), png)?;
        }

        return Ok(());
    }

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

    use crate::{Gamemode, Loadout, LoadoutBias, data::load_dataset};

    #[tokio::test]
    async fn no_dupes() {
        let mut res = Resources::new();
        let dataset = load_dataset(&mut res).await.unwrap();

        let mut rng = rand::rng();
        for mode in [
            Gamemode::Any,
            Gamemode::Melee,
            Gamemode::Shotgun,
            Gamemode::Auto,
            Gamemode::Semi,
            Gamemode::Bombs,
        ] {
            for bias in [
                LoadoutBias::Fair,
                LoadoutBias::NoHeavy,
                LoadoutBias::OnlyHeavy,
            ] {
                for _ in 0..100000 {
                    let loadout = Loadout::pick(&mut rng, "player", &dataset, bias, mode);
                    assert_ne!(loadout.gadgets[0].image, loadout.gadgets[1].image);
                    assert_ne!(loadout.gadgets[0].image, loadout.gadgets[2].image);
                    assert_ne!(loadout.gadgets[1].image, loadout.gadgets[2].image);
                }
            }
        }
    }
}
