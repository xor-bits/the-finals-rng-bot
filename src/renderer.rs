use eyre::Result;
use vello_common::paint::ImageId;
use vello_cpu::{
    Image, ImageSource, Pixmap, RenderContext, Resources,
    color::{AlphaColor, Srgb},
    kurbo::{Affine, Rect},
    peniko::{Extend, ImageQuality, ImageSampler},
};

use crate::Loadout;

const LAYOUT: &[LayoutElement] = &[
    LayoutElement::Padding,
    LayoutElement::Padding,
    LayoutElement::IconSpecial,
    LayoutElement::Padding,
    LayoutElement::Separator,
    LayoutElement::Padding,
    LayoutElement::IconWeapon,
    LayoutElement::Padding,
    LayoutElement::Separator,
    LayoutElement::Padding,
    LayoutElement::IconGadget(0),
    LayoutElement::Padding,
    LayoutElement::IconGadget(1),
    LayoutElement::Padding,
    LayoutElement::IconGadget(2),
    LayoutElement::Padding,
    LayoutElement::Padding,
];
const ICON_SIZE: u16 = 128;
const W: u16 = calculate_width(LAYOUT);
const H: u16 = LayoutElement::Padding.height() + 2 * LayoutElement::Padding.width();

#[derive(Clone, Copy)]
enum LayoutElement {
    Padding,
    Separator,
    IconSpecial,
    IconWeapon,
    IconGadget(u8),
}

impl LayoutElement {
    pub const fn width(self) -> u16 {
        match self {
            LayoutElement::Padding => 10,
            LayoutElement::Separator => 2,
            _ => ICON_SIZE,
        }
    }

    pub const fn height(self) -> u16 {
        ICON_SIZE
    }

    pub const fn color(self) -> AlphaColor<Srgb> {
        match self {
            LayoutElement::Separator => AlphaColor::from_rgb8(0x40, 0x50, 0x5d),
            _ => unreachable!(),
        }
    }
}

pub struct Renderer {
    ctx: RenderContext,
    pub res: Resources,
    // pub target: Pixmap,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            ctx: RenderContext::new(W, H),
            res: Resources::new(),
            // target: Pixmap::new(W, H),
        }
    }

    pub fn render(&mut self, loadouts: &[Loadout]) -> Result<Vec<u8>> {
        self.ctx.reset_and_resize(W, H);
        // self.target.resize(W, H);

        // NOTE: `Pixmap` only has `into_png` which consumes `target`
        // which forces this to deallocate and reallocate for no reason
        let mut target = Pixmap::new(W, H);

        self.ctx.set_paint(AlphaColor::from_rgb8(0x2c, 0x32, 0x3d));
        self.ctx
            .fill_rect(&Rect::from_points((0.0, 0.0), (W as f64, H as f64)));

        let mut cursor_x = 0.0;
        let cursor_y = 10.0;
        for elem in LAYOUT {
            let width = elem.width() as f64;
            let height = elem.height() as f64;
            match elem {
                LayoutElement::Separator => {
                    self.ctx.set_paint(elem.color());
                    self.ctx.fill_rect(&Rect::from_points(
                        (cursor_x, cursor_y),
                        (cursor_x + width, cursor_y + height),
                    ));
                }
                LayoutElement::IconSpecial => {
                    self.draw_image(loadouts[0].special, (cursor_x, cursor_y), (width, height));
                }
                LayoutElement::IconWeapon => {
                    self.draw_image(loadouts[0].weapon, (cursor_x, cursor_y), (width, height));
                }
                LayoutElement::IconGadget(i) => {
                    self.draw_image(
                        loadouts[0].gadgets[*i as usize],
                        (cursor_x, cursor_y),
                        (width, height),
                    );
                }
                _ => {}
            }
            cursor_x += width;
        }

        self.ctx.render(&mut target, &mut self.res);

        Ok(target.into_png()?)
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

const fn calculate_width(layout: &[LayoutElement]) -> u16 {
    let mut sum = 0;

    let mut i = 0usize;
    while i < layout.len() {
        sum += layout[i].width();
        i += 1;
    }

    sum
}
