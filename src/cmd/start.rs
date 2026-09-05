use rand::seq::SliceRandom;
use serenity::all::{
    CommandInteraction, CommandOptionType, Context, CreateAttachment, CreateCommand,
    CreateCommandOption, CreateInteractionResponseMessage, ResolvedValue,
};

use crate::{Handler, Loadout, Mode, data::Dataset};

pub fn register() -> CreateCommand {
    CreateCommand::new("start")
        .description("start a TheFinals private match")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "players",
                "comma separated list of player names",
            )
            .required(true),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::Integer,
                "teams",
                "amount of teams to generate",
            )
            .required(true)
            .add_int_choice("2", 2)
            .add_int_choice("3", 3)
            .add_int_choice("4", 4),
        )
}

pub async fn run(
    handler: &Handler,
    _ctx: &Context,
    interaction: &CommandInteraction,
) -> Result<CreateInteractionResponseMessage, String> {
    let mut players = None;
    let mut teams = None;

    for opt in interaction.data.options() {
        match (opt.name, opt.value) {
            ("players", ResolvedValue::String(s)) => players = Some(s),
            ("teams", ResolvedValue::Integer(i)) => teams = Some(i),
            _ => {}
        }
    }

    let (Some(players), Some(teams)) = (players, teams) else {
        return Err("missing or invalid options".to_string());
    };

    let mode = match teams {
        2 => Mode::Duos,
        3 => Mode::Trios,
        4 => Mode::Quads,
        _ => return Err("invalid team size".to_string()),
    };

    let (loadouts, valid) = make_teams(players, &handler.dataset);
    let loadouts = &loadouts[0..valid];

    let mut renderer = handler.renderer.lock().await;
    let Ok(png) = renderer.render(loadouts, mode).map_err(|err| {
        eprintln!("failed to draw results: {err}");
    }) else {
        return Err("internal error".to_string());
    };

    Ok(CreateInteractionResponseMessage::new().add_file(CreateAttachment::bytes(png, "teams.png")))
}

fn make_teams<'a>(players: &'a str, dataset: &Dataset) -> ([Loadout<'a>; 16], usize) {
    let mut player_iter = players.split(',');

    let mut rng = rand::rng();
    let mut valid = 0usize;
    let mut loadouts = [(); 16].map(|_| {
        let name = player_iter
            .next()
            .inspect(|_| {
                valid += 1;
            })
            .unwrap_or("?")
            .trim();
        let truncated_name = &name[0..name.len().min(30)];
        Loadout::pick(&mut rng, truncated_name, dataset)
    });
    loadouts[0..valid].shuffle(&mut rng);

    (loadouts, valid)
}
