use std::{
    collections::HashMap,
    fs,
    io::{BufReader, BufWriter, Cursor, Write},
    mem::swap,
    path::Path,
};

use eyre::Result;
use reqwest::Certificate;
use serde::Deserialize;
use tokio::io::AsyncReadExt;
use vello_common::paint::ImageId;
use vello_cpu::{Pixmap, Resources};

use crate::Gamemode;

pub struct Dataset {
    classes_any: [Class; 3],
    classes_melee: [Class; 3],
    classes_shotgun: [Class; 3],
    classes_auto: [Class; 3],
    classes_semi: [Class; 3],
    classes_bombs: [Class; 3],
}

impl Dataset {
    pub const fn class(&self, mode: Gamemode) -> &[Class; 3] {
        match mode {
            Gamemode::Any => &self.classes_any,
            Gamemode::Melee => &self.classes_melee,
            Gamemode::Shotgun => &self.classes_shotgun,
            Gamemode::Auto => &self.classes_auto,
            Gamemode::Semi => &self.classes_semi,
            Gamemode::Bombs => &self.classes_bombs,
        }
    }

    pub const fn class_mut(&mut self, mode: Gamemode) -> &mut [Class; 3] {
        match mode {
            Gamemode::Any => &mut self.classes_any,
            Gamemode::Melee => &mut self.classes_melee,
            Gamemode::Shotgun => &mut self.classes_shotgun,
            Gamemode::Auto => &mut self.classes_auto,
            Gamemode::Semi => &mut self.classes_semi,
            Gamemode::Bombs => &mut self.classes_bombs,
        }
    }
}

#[derive(Default)]
pub struct Class {
    pub weapons: Vec<Item>,
    pub gadgets: Vec<Item>,
    pub specials: Vec<Item>,
}

#[derive(Clone, Copy)]
pub struct Item {
    pub image: ImageId,
    pub name: &'static str,
}

impl Item {
    pub fn null() -> Self {
        Self {
            image: ImageId::new(0),
            name: "",
        }
    }
}

#[derive(Deserialize)]
struct ClassDesc {
    #[serde(default)]
    #[serde(rename = "Weapons")]
    weapons: HashMap<&'static str, EntryDesc>,
    #[serde(default)]
    #[serde(rename = "Gadgets")]
    gadgets: HashMap<&'static str, EntryDesc>,
    #[serde(default)]
    #[serde(rename = "Specializations")]
    specializations: HashMap<&'static str, EntryDesc>,

    #[serde(default)]
    __serde_lifetime_bugfix: &'static str,
}

#[derive(Deserialize)]
struct EntryDesc {
    hr_img_url: &'static str,
    #[serde(default)]
    kind: Gamemode,
    #[serde(skip)]
    loaded: LoadedImage,
}

#[derive(Default)]
enum LoadedImage {
    #[default]
    None,
    Image(Pixmap),
    Id(ImageId),
}

#[derive(Clone, Copy)]
enum ClassKind {
    Light,
    Medium,
    Heavy,
}

impl ClassKind {
    pub const fn id(self) -> usize {
        match self {
            ClassKind::Light => 0,
            ClassKind::Medium => 1,
            ClassKind::Heavy => 2,
        }
    }
}

async fn load_item(cachedir: &Path, url: &str) -> Result<Pixmap> {
    let Some(filename) = url.rsplit('/').next() else {
        eyre::bail!("bad hr_img_url: '{url}'");
    };
    let cached_item_path = cachedir.join(filename);

    let exists = tokio::fs::try_exists(&cached_item_path).await?;
    if exists {
        let mut cached_item_file = tokio::fs::File::open(&cached_item_path)
            .await?
            .into_std()
            .await;

        let pixmap = tokio::task::spawn_blocking(move || {
            let reader = BufReader::new(&mut cached_item_file);
            Pixmap::from_png(reader)
        })
        .await??;
        return Ok(pixmap);
    }

    let open_cache_future = async {
        Ok::<_, tokio::io::Error>(
            tokio::fs::File::create(&cached_item_path)
                .await?
                .into_std()
                .await,
        )
    };

    let mut client_builder = reqwest::Client::builder();
    for cert in webpki_root_certs::TLS_SERVER_ROOT_CERTS {
        client_builder = client_builder.add_root_certificate(Certificate::from_der(cert)?);
    }
    let client = client_builder.build()?;

    let fetch_png_future = async { client.get(url).send().await?.bytes().await };

    let (cached_image_file, fetched_png) = tokio::join!(open_cache_future, fetch_png_future);

    let mut cached_image_file = cached_image_file?;
    let fetched_png = fetched_png?;

    let pixmap = tokio::task::spawn_blocking(move || -> Result<_> {
        let mut writer = BufWriter::new(&mut cached_image_file);
        writer.write_all(&fetched_png)?;

        Ok(Pixmap::from_png(Cursor::new(&*fetched_png))?)
    })
    .await??;
    Ok(pixmap)
}

fn build_class(
    class: &mut Class,
    contents: &mut ClassDesc,
    resources: &mut Resources,
    filter: Option<Gamemode>,
) {
    for (category_in, category_out) in [
        (&mut contents.weapons, &mut class.weapons),
        (&mut contents.gadgets, &mut class.gadgets),
        (&mut contents.specializations, &mut class.specials),
    ] {
        for (&name, item) in category_in.iter_mut() {
            if let Some(filter) = filter
                && filter != item.kind
            {
                continue;
            }

            let mut swapped_item = LoadedImage::None;
            swap(&mut swapped_item, &mut item.loaded);

            let item_id = match swapped_item {
                LoadedImage::None => unreachable!(),
                LoadedImage::Image(pixmap) => resources.register_image(pixmap.into()),
                LoadedImage::Id(index) => index,
            };

            item.loaded = LoadedImage::Id(item_id);

            category_out.push(Item {
                image: item_id,
                name,
            });
        }
    }
}

fn build_any_class(
    names: &str,
    contents: &mut ClassDesc,
    resources: &mut Resources,
    classes: &mut [Class; 3],
    filter: Option<Gamemode>,
) {
    for name in names.split(',') {
        let kinds: &[ClassKind] = match name {
            "Light" => &[ClassKind::Light],
            "Medium" => &[ClassKind::Medium],
            "Heavy" => &[ClassKind::Heavy],
            "All" => &[ClassKind::Light, ClassKind::Medium, ClassKind::Heavy],
            _ => unimplemented!("{name}"),
        };

        for kind in kinds {
            build_class(&mut classes[kind.id()], contents, resources, filter);
        }
    }
}

pub async fn load_dataset(resources: &mut Resources) -> Result<Dataset> {
    let cachedir = Path::new("./cache");
    fs::create_dir_all(cachedir)?;

    let mut buf = String::new();
    _ = tokio::fs::File::open("./dataset.json")
        .await?
        .read_to_string(&mut buf)
        .await?;
    let buf = buf.leak();

    let mut classes: HashMap<&str, ClassDesc> = serde_json::de::from_str(buf)?;

    let results = futures::future::join_all(
        classes
            .values_mut()
            .flat_map(|class| {
                class
                    .weapons
                    .values_mut()
                    .chain(class.gadgets.values_mut())
                    .chain(class.specializations.values_mut())
            })
            .map(|item| async {
                let image = load_item(cachedir, item.hr_img_url).await?;
                item.loaded = LoadedImage::Image(image);
                Ok::<_, eyre::Error>(())
            }),
    )
    .await;
    for result in results {
        result?;
    }

    let mut dataset = Dataset {
        classes_any: [(); 3].map(|_| Class::default()),
        classes_melee: [(); 3].map(|_| Class::default()),
        classes_shotgun: [(); 3].map(|_| Class::default()),
        classes_auto: [(); 3].map(|_| Class::default()),
        classes_semi: [(); 3].map(|_| Class::default()),
        classes_bombs: [(); 3].map(|_| Class::default()),
    };

    for filter in [
        None,
        Some(Gamemode::Melee),
        Some(Gamemode::Shotgun),
        Some(Gamemode::Auto),
        Some(Gamemode::Semi),
        Some(Gamemode::Bombs),
    ] {
        for (&names, contents) in classes.iter_mut() {
            build_any_class(
                names,
                contents,
                resources,
                dataset.class_mut(filter.unwrap_or(Gamemode::Any)),
                filter,
            );
        }
    }

    Ok(dataset)
}
