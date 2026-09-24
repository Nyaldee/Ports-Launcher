
use crate::core::grid;
use crate::core::models::Port;
use crate::{CardItem, CardRow};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

fn load_cached_card_image(image_cache: &RefCell<HashMap<String, slint::Image>>, cache_dir: &Path, folder: &str) -> slint::Image {
    if let Some(img) = image_cache.borrow().get(folder) {
        return img.clone();
    }
    let image = crate::core::image_cache::cached_image_path(cache_dir, folder)
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| image::load_from_memory(&bytes).ok())
        .map(|decoded| {
            let rgba = decoded.into_rgba8();
            let (w, h) = rgba.dimensions();
            slint::Image::from_rgba8(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(rgba.as_raw(), w, h))
        })
        .unwrap_or_default();
    image_cache.borrow_mut().insert(folder.to_string(), image.clone());
    image
}

pub(crate) fn build_card_rows(
    image_cache: &RefCell<HashMap<String, slint::Image>>,
    ports: &[Port],
    cache_dir: &Path,
    columns: usize,
    selected: Option<(usize, usize)>,
) -> Vec<CardRow> {
    let items: Vec<CardItem> = ports
        .iter()
        .enumerate()
        .map(|(i, port)| CardItem {
            name: port.name.clone().into(),
            image: load_cached_card_image(image_cache, cache_dir, &port.folder),
            selected: Some((i / columns, i % columns)) == selected,
        })
        .collect();
    grid::chunk_into_rows(&items, columns).into_iter().map(|cards| CardRow { cards: slint::ModelRc::new(slint::VecModel::from(cards)) }).collect()
}
