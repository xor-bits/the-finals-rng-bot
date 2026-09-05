use eyre::Result;
use rand::{RngExt, rngs::ThreadRng, seq::SliceRandom};
use vello_common::paint::ImageId;

use self::{data::load_dataset, renderer::Renderer};

mod data;
mod renderer;

pub struct Loadout<'a> {
    pub name: &'a str,
    pub special: ImageId,
    pub weapon: ImageId,
    pub gadgets: [ImageId; 3],
}

#[derive(Clone, Copy)]
pub enum Mode {
    Duel,
    Trios,
    Quads,
}

impl Mode {
    pub const fn teams(self) -> usize {
        match self {
            Mode::Duel => 2,
            Mode::Trios => 3,
            Mode::Quads => 4,
        }
    }
}

fn pick_loadout<'a>(rng: &mut ThreadRng, name: &'a str, dataset: &data::Dataset) -> Loadout<'a> {
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

    Loadout {
        name,
        special,
        weapon,
        gadgets,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut renderer = Renderer::new()?;
    let dataset = load_dataset(&mut renderer.res).await?;

    let mut rng = rand::rng();

    let players = [
        "player 1", "player 2", "player 3", "player 4", "player 5", "player 6",
    ];
    let mut loadouts = players.map(|name| pick_loadout(&mut rng, name, &dataset));

    loadouts.shuffle(&mut rng);

    let result = renderer.render(&loadouts, Mode::Trios)?;

    std::fs::write("out.png", result)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use vello_cpu::Resources;

    use crate::{data::load_dataset, pick_loadout};

    #[tokio::test]
    async fn no_dupes() {
        let mut res = Resources::new();
        let dataset = load_dataset(&mut res).await.unwrap();

        let mut rng = rand::rng();
        for _ in 0..100000 {
            let loadout = pick_loadout(&mut rng, "player", &dataset);

            assert_ne!(loadout.gadgets[0], loadout.gadgets[1]);
            assert_ne!(loadout.gadgets[0], loadout.gadgets[2]);
            assert_ne!(loadout.gadgets[1], loadout.gadgets[2]);
        }
    }
}
