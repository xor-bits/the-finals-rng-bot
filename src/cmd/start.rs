use eyre::Result;
use serenity::all::{
    Colour, CommandInteraction, CommandOptionType, Context, CreateAttachment, CreateCommand,
    CreateCommandOption, CreateEmbed, CreateInteractionResponseMessage, ResolvedValue,
};

use crate::{Handler, Teams, renderer};

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
            .add_int_choice("1", 1)
            .add_int_choice("2", 2)
            .add_int_choice("3", 3)
            .add_int_choice("4", 4),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::Boolean,
                "noautobalance",
                "use fair RNG even when team sizes are unfair",
            )
            .required(false),
        )
}

pub async fn run(
    handler: &Handler,
    _ctx: &Context,
    interaction: &CommandInteraction,
) -> Result<CreateInteractionResponseMessage, String> {
    let mut players = None;
    let mut teams = None;
    let mut noautobalance = false;

    for opt in interaction.data.options() {
        match (opt.name, opt.value) {
            ("players", ResolvedValue::String(s)) => players = Some(s),
            ("teams", ResolvedValue::Integer(i)) => teams = Some(i),
            ("noautobalance", ResolvedValue::Boolean(b)) => noautobalance = b,
            _ => {}
        }
    }

    let (Some(players), Some(teams)) = (players, teams) else {
        return Err("missing or invalid options".to_string());
    };

    let teams = match teams {
        1..=4 => teams as u8,
        _ => return Err("invalid team size".to_string()),
    };

    let teams = Teams::pick(
        &mut rand::rng(),
        players,
        &handler.dataset,
        teams,
        !noautobalance,
    );

    let mut renderer = handler.renderer.lock().await;
    let result = teams
        .iter()
        .enumerate()
        .map(|(i, team)| {
            let png = renderer.render(team, i)?;
            Ok((png, team))
        })
        .collect::<Result<Vec<_>>>();
    drop(renderer);

    let Ok(result) = result.map_err(|err| {
        eprintln!("failed to render a team: {err}");
    }) else {
        return Err("internal error".to_string());
    };

    let mut response = CreateInteractionResponseMessage::new();
    for (i, (png, team)) in result.into_iter().enumerate() {
        let color = renderer::TEAM_COLORS[i % renderer::TEAM_COLORS.len()].to_rgba8();

        let attachment = format!("team{i}.png");
        response = response.add_file(CreateAttachment::bytes(png, attachment.clone()));

        let mut embed = CreateEmbed::new()
            .title(format!("Team {}", i + 1))
            .color(Colour::from_rgb(color.r, color.g, color.b))
            .attachment(attachment);

        for player in team {
            embed = embed.field(
                player.name,
                format!(
                    "{}, {}, {}, {}, {}",
                    player.special.name,
                    player.weapon.name,
                    player.gadgets[0].name,
                    player.gadgets[1].name,
                    player.gadgets[2].name,
                ),
                true,
            );
        }
        response = response.add_embed(embed);
    }

    Ok(response)
}
