
use crate::core::grid;
use crate::core::models::Port;
use crate::{CardItem, CardRow};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

pub(crate) fn load_cached_card_image(image_cache: &RefCell<HashMap<String, slint::Image>>, cache_dir: &Path, folder: &str) -> slint::Image {
    if let Some(img) = image_cache.borrow().get(folder) {
        return img.clone();
    }
    let image = (|| {
        let path = crate::core::image_cache::cached_image_path(cache_dir, folder).ok()?;
        let bytes = std::fs::read(&path).ok()?;
        let decoded = image::load_from_memory(&bytes).ok()?;
        let rgba = decoded.into_rgba8();
        let (w, h) = rgba.dimensions();
        let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(rgba.as_raw(), w, h);
        Some(slint::Image::from_rgba8(buffer))
    })()
    .unwrap_or_default();
    image_cache.borrow_mut().insert(folder.to_string(), image.clone());
    image
}

pub(crate) fn build_card_item(image_cache: &RefCell<HashMap<String, slint::Image>>, port: &Port, cache_dir: &Path, selected: bool) -> CardItem {
    CardItem { name: port.name.clone().into(), image: load_cached_card_image(image_cache, cache_dir, &port.folder), selected }
}

pub(crate) fn build_card_rows(
    image_cache: &RefCell<HashMap<String, slint::Image>>,
    ports: &[Port],
    cache_dir: &Path,
    columns: usize,
    selected: Option<(usize, usize)>,
) -> Vec<CardRow> {
    let cols = columns.max(1);
    let items: Vec<CardItem> = ports
        .iter()
        .enumerate()
        .map(|(i, p)| build_card_item(image_cache, p, cache_dir, Some((i / cols, i % cols)) == selected))
        .collect();
    grid::chunk_into_rows(&items, columns).into_iter().map(|cards| CardRow { cards: slint::ModelRc::new(slint::VecModel::from(cards)) }).collect()
}
