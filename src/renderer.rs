use std::{fs::File, io::Read, sync::Arc};

use eyre::Result;
use skrifa::{MetadataProvider, instance::Size};
use vello_common::paint::ImageId;
use vello_cpu::{
    Glyph, Image, ImageSource, Pixmap, RenderContext, Resources,
    color::{AlphaColor, Srgb, palette::css},
    kurbo::{Affine, Rect},
    peniko::{Blob, Extend, FontData, ImageQuality, ImageSampler},
};

use crate::{Loadout, Mode};

const LAYOUT_LOADOUT: &[Element] = &[
    Element::Padding(20),
    Element::IconSpecial,
    Element::Padding(10),
    Element::Separator,
    Element::Padding(10),
    Element::IconWeapon,
    Element::Padding(10),
    Element::Separator,
    Element::Padding(10),
    Element::IconGadget(0),
    Element::Padding(10),
    Element::IconGadget(1),
    Element::Padding(10),
    Element::IconGadget(2),
    Element::Padding(20),
];
const LAYOUT_MAIN: &[Element] = &[
    // Element::Padding(20),
    //
    Element::Teams,
    //
    // Element::Padding(20),
];
const LAYOUT_TEAM: &[Element] = &[
    Element::Padding(10),
    //
    Element::Players,
    //
    Element::Padding(10),
];
const LAYOUT_PLAYER: &[Element] = &[
    Element::Padding(10),
    Element::LoadoutName,
    Element::Padding(10),
    Element::Loadout,
    // Element::Padding(2),
    Element::LoadoutLabels,
    Element::Padding(10),
    //
];

const ICON_SIZE: u16 = 128;
const W: u16 = calculate_width();
const TEAM_COLORS: [AlphaColor<Srgb>; 4] = [
    AlphaColor::from_rgb8(0x0d, 0x9c, 0xd5),
    AlphaColor::from_rgb8(0xfa, 0x32, 0xa9),
    AlphaColor::from_rgb8(0xec, 0x57, 0x18),
    AlphaColor::from_rgb8(0x9b, 0x43, 0xec),
];

#[derive(Debug, Clone, Copy)]
enum Element {
    Padding(u16),
    Separator,
    /// only for the main split
    Teams,
    /// only for the player splits
    Players,
    /// only for team splits
    LoadoutName,
    /// only for team splits
    Loadout,
    /// only for team splits
    LoadoutLabels,
    /// only for loadout splits
    IconSpecial,
    /// only for loadout splits
    IconWeapon,
    /// only for loadout splits
    IconGadget(u8),
}

impl Element {
    pub const fn size(self) -> u16 {
        match self {
            Self::Padding(p) => p,
            Self::Separator => 2,
            Self::LoadoutName => 48,
            Self::LoadoutLabels => 16,
            Self::Teams | Self::Players => 0,
            _ => ICON_SIZE,
        }
    }

    pub const fn color(self) -> AlphaColor<Srgb> {
        match self {
            Self::Separator => AlphaColor::from_rgb8(0x40, 0x50, 0x5d),
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
struct Cursor {
    x: f64,
    y: f64,
}

pub struct Renderer {
    ctx: RenderContext,
    pub res: Resources,
    // pub target: Pixmap,
    font_data: FontData,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        let mut font_file = File::open("./asset/Roboto-Regular.ttf")?;
        let mut buf = Vec::new();
        _ = font_file.read_to_end(&mut buf)?;
        let font_data = FontData::new(Blob::new(Arc::new(buf.into_boxed_slice())), 0);

        Ok(Self {
            ctx: RenderContext::new(W, 800),
            res: Resources::new(),
            // target: Pixmap::new(W, H),
            font_data,
        })
    }

    pub fn render(&mut self, loadouts: &[Loadout], mode: Mode) -> Result<Vec<u8>> {
        let h = calculate_height(mode.teams(), loadouts.len());

        self.ctx.reset_and_resize(W, h);
        // self.target.resize(W, h);

        // NOTE: `Pixmap` only has `into_png` which consumes `target`
        // which forces this to deallocate and reallocate for no reason
        let mut target = Pixmap::new(W, h);

        // self.draw_rect(
        //     (0.0, 0.0),
        //     (W as f64, h as f64),
        //     AlphaColor::from_rgb8(0x2c, 0x32, 0x3d),
        // );

        let mut cursor = Cursor { x: 0.0, y: 0.0 };
        self.draw_main(&mut cursor, loadouts, mode);
        assert_eq!(cursor.y as u16, h);

        self.ctx.render(&mut target, &mut self.res);

        Ok(target.into_png()?)
    }

    fn draw_main(&mut self, cursor: &mut Cursor, mut loadouts: &[Loadout], mode: Mode) {
        let players = loadouts.len();
        let teams = mode.teams();
        let normal_team_players = players / teams;
        let large_team_players = normal_team_players + 1;
        let large_teams = players % teams;
        let normal_teams = teams - large_teams;

        let mut i = 0usize;
        for elem in LAYOUT_MAIN {
            let height = elem.size() as f64;

            if let Element::Teams = elem {
                for _ in 0..large_teams {
                    let team;
                    (team, loadouts) = loadouts.split_at(large_team_players);
                    self.draw_team(cursor, team, i);
                    i += 1;
                }
                for _ in 0..normal_teams {
                    let team;
                    (team, loadouts) = loadouts.split_at(normal_team_players);
                    self.draw_team(cursor, team, i);
                    i += 1;
                }
            }

            cursor.y += height;
        }
    }

    fn draw_team(&mut self, cursor: &mut Cursor, loadouts: &[Loadout], i: usize) {
        self.draw_rect(
            (0.0, cursor.y),
            (W as f64, calculate_height_team(loadouts.len()) as f64),
            TEAM_COLORS[i % TEAM_COLORS.len()],
        );

        for elem in LAYOUT_TEAM {
            let height = elem.size() as f64;

            if let Element::Players = elem {
                for loadout in loadouts {
                    self.draw_player(cursor, loadout);
                }
            }

            cursor.y += height;
        }
    }

    fn draw_player(&mut self, cursor: &mut Cursor, loadout: &Loadout) {
        let x = cursor.x;
        for elem in LAYOUT_PLAYER {
            let height = elem.size() as f64;

            match elem {
                Element::LoadoutName => {
                    // self.draw_rect(
                    //     (cursor.x, cursor.y),
                    //     (20.0, height),
                    //     AlphaColor::from_rgb8(0xff, 0x00, 0x00),
                    // );
                    cursor.x += 20.0;
                    self.draw_text(cursor, loadout.name, 48.0);
                    cursor.x = x;
                }
                Element::Loadout => {
                    self.draw_loadout(cursor, loadout);
                    cursor.x = x;
                }
                Element::LoadoutLabels => {
                    self.draw_loadout_labels(cursor, loadout);
                    cursor.x = x;
                }
                _ => {}
            }

            cursor.y += height;
        }
    }

    fn draw_text(&mut self, cursor: &mut Cursor, text: &str, size: f32) {
        let font_ref = skrifa::FontRef::new(self.font_data.data.data())
            .expect("invalid font should have already been caught");

        let axes = font_ref.axes();
        let location = axes.location::<&[(&str, f32)]>(&[]);
        let charmap = font_ref.charmap();
        let glyph_metrics = font_ref.glyph_metrics(Size::new(size), &location);
        let global_metrics = font_ref.metrics(Size::new(size), &location);

        let mut cursor_x = cursor.x as f32;
        // let y = cursor.y as f32;
        // let y = cursor.y as f32 + global_metrics.descent + global_metrics.ascent;
        let y = cursor.y as f32 - global_metrics.ascent
            + global_metrics
                .cap_height
                .expect("invalid font should already been caight")
            + size;

        self.ctx.set_paint(css::WHITE);
        self.ctx
            .glyph_run(&mut self.res, &self.font_data)
            .font_size(size)
            .fill_glyphs(text.chars().filter(|&ch| ch != '\n').filter_map(move |ch| {
                let id = charmap.map(ch)?;
                let advance = glyph_metrics.advance_width(id)?;
                let x = cursor_x;
                cursor_x += advance;
                Some(Glyph {
                    id: id.to_u32(),
                    x,
                    y,
                })
            }));
    }

    fn draw_loadout(&mut self, cursor: &mut Cursor, loadout: &Loadout) {
        for elem in LAYOUT_LOADOUT {
            let width = elem.size() as f64;
            let height = ICON_SIZE as f64;
            match elem {
                Element::Separator => {
                    self.ctx.set_paint(elem.color());
                    self.ctx.fill_rect(&Rect::from_points(
                        (cursor.x, cursor.y),
                        (cursor.x + width, cursor.y + height),
                    ));
                }
                Element::IconSpecial => {
                    self.draw_image(loadout.special.image, (cursor.x, cursor.y), (width, height));
                }
                Element::IconWeapon => {
                    self.draw_image(loadout.weapon.image, (cursor.x, cursor.y), (width, height));
                }
                Element::IconGadget(i) => {
                    self.draw_image(
                        loadout.gadgets[*i as usize].image,
                        (cursor.x, cursor.y),
                        (width, height),
                    );
                }
                _ => {}
            }
            cursor.x += width;
        }
    }

    fn draw_loadout_labels(&mut self, cursor: &mut Cursor, loadout: &Loadout) {
        for elem in LAYOUT_LOADOUT {
            let width = elem.size() as f64;
            match elem {
                Element::Separator => {
                    self.ctx.set_paint(elem.color());
                    self.ctx.fill_rect(&Rect::from_points(
                        (cursor.x, cursor.y),
                        (cursor.x + width, cursor.y + 16.0),
                    ));
                }
                Element::IconSpecial => {
                    self.draw_rect(
                        (cursor.x, cursor.y),
                        (width, 16.0),
                        Element::Separator.color(),
                    );
                    self.draw_text(cursor, &loadout.special.name, 16.0);
                }
                Element::IconWeapon => {
                    self.draw_rect(
                        (cursor.x, cursor.y),
                        (width, 16.0),
                        Element::Separator.color(),
                    );
                    self.draw_text(cursor, &loadout.weapon.name, 16.0);
                }
                Element::IconGadget(i) => {
                    self.draw_rect(
                        (cursor.x, cursor.y),
                        (width, 16.0),
                        Element::Separator.color(),
                    );
                    self.draw_text(cursor, &loadout.gadgets[*i as usize].name, 16.0);
                }
                _ => {}
            }
            cursor.x += width;
        }
    }

    fn draw_image(&mut self, image: ImageId, at: (f64, f64), size: (f64, f64)) {
        let image_info = self.res.resolve_image(image).expect("corrupt ImageId");
        let src_size = (image_info.width() as f64, image_info.height() as f64);

        let transform = Affine::translate(at)
            * Affine::scale_non_uniform(size.0 / src_size.0, size.1 / src_size.1);

        self.ctx.set_paint(sample_image(image));
        self.ctx.set_paint_transform(transform);
        self.ctx
            .fill_rect(&Rect::from_points(at, (at.0 + size.0, at.1 + size.1)));
    }

    fn draw_rect(&mut self, at: (f64, f64), size: (f64, f64), color: AlphaColor<Srgb>) {
        self.ctx.set_paint(color);
        self.ctx.fill_rect(&Rect::from_origin_size(at, size));
    }
}

fn sample_image(image: ImageId) -> Image {
    Image {
        image: ImageSource::opaque_id(image),
        sampler: ImageSampler {
            x_extend: Extend::Repeat,
            y_extend: Extend::Repeat,
            quality: ImageQuality::Medium,
            alpha: 1.0,
        },
    }
}

const fn calculate_width() -> u16 {
    let mut sum = 0;

    let mut i = 0usize;
    while i < LAYOUT_LOADOUT.len() {
        sum += LAYOUT_LOADOUT[i].size();
        i += 1;
    }

    sum
}

fn calculate_height_team(players: usize) -> u16 {
    let mut sum: u16 = 0;

    for elem in LAYOUT_TEAM {
        sum += elem.size();
    }
    for elem in LAYOUT_PLAYER {
        sum += elem.size() * players as u16;
    }

    sum
}

fn calculate_height(teams: usize, players: usize) -> u16 {
    let mut sum: u16 = 0;

    for elem in LAYOUT_MAIN {
        sum += elem.size();
    }
    for elem in LAYOUT_TEAM {
        sum += elem.size() * teams as u16;
    }
    for elem in LAYOUT_PLAYER {
        sum += elem.size() * players as u16;
    }

    sum
}
