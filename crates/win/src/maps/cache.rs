//! Maps and sprites already looked up, kept on disk so a hover is instant (and works offline) next time:
//! %LOCALAPPDATA%\MapleLens\cache\maps (`<id>.json` + `index.json`) and cache\icons\<kind>\<id>.png, the same
//! layout the Python version used.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use maplelens_core::image::decode_png_rgba;
use maplelens_core::mapinfo::data::{
    catalog_from_json, catalog_to_json, index_from_json, index_to_json, map_from_json, map_to_json,
};
use maplelens_core::mapinfo::resolve::Candidate;
use maplelens_core::mapinfo::{Location, MapInfo, resolve};
use maplelens_core::render::RgbaImage;

use crate::net::NetError;
use crate::paths;

pub fn cache_dir() -> PathBuf {
    paths::data_dir().join("cache")
}

pub fn icon_path(folder: &Path, kind: &str, id: u32) -> PathBuf {
    folder.join(kind).join(format!("{id}.png"))
}

/// Fetch a missing sprite into `folder`; true when it is on disk afterwards. Runs without any lock held.
pub fn download_icon(
    folder: &Path,
    kind: &str,
    id: u32,
    fetch: impl Fn(&str, u32) -> Result<Vec<u8>, NetError>,
) -> bool {
    let path = icon_path(folder, kind, id);
    if path.exists() {
        return true;
    }
    match fetch(kind, id).and_then(|bytes| {
        decode_png_rgba(&bytes).map_err(|e| NetError(format!("not a PNG: {e}")))?;
        paths::write_atomic(&path, &bytes).map_err(|e| NetError(e.to_string()))
    }) {
        Ok(()) => true,
        Err(error) => {
            crate::warn!("could not fetch {kind} {id}: {error}");
            false
        }
    }
}

/// Sprites from memory or disk; `get` never touches the network (`download_icon` fills the folder).
pub struct IconCache {
    folder: PathBuf,
    memory: HashMap<(String, u32), Arc<RgbaImage>>,
}

impl IconCache {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder, memory: HashMap::new() }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn get(&mut self, kind: &str, id: u32) -> Option<Arc<RgbaImage>> {
        let key = (kind.to_string(), id);
        if let Some(image) = self.memory.get(&key) {
            return Some(image.clone());
        }
        // A missing sprite is not remembered: it is looked for again once downloaded.
        let (width, height, data) = decode_png_rgba(&std::fs::read(icon_path(&self.folder, kind, id)).ok()?).ok()?;
        let image = Arc::new(RgbaImage { width, height, data });
        self.memory.insert(key, image.clone());
        Some(image)
    }
}

/// `<id>.json` per map plus `index.json` from title readings to map ids.
pub struct MapStore {
    folder: PathBuf,
    index: Option<HashMap<String, u32>>,
}

impl MapStore {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder, index: None }
    }

    fn index(&mut self) -> &mut HashMap<String, u32> {
        let folder = &self.folder;
        self.index.get_or_insert_with(|| {
            std::fs::read_to_string(folder.join("index.json")).map(|t| index_from_json(&t)).unwrap_or_default()
        })
    }

    pub fn find(&mut self, location: &Location) -> Option<u32> {
        self.index().get(&resolve::key(location)).copied()
    }

    pub fn remember(&mut self, location: &Location, id: u32) {
        let key = resolve::key(location);
        if self.index().get(&key) != Some(&id) {
            self.index().insert(key, id);
            let text = index_to_json(self.index());
            self.write("index.json", &text);
        }
    }

    pub fn load(&self, id: u32) -> Option<MapInfo> {
        map_from_json(&std::fs::read_to_string(self.folder.join(format!("{id}.json"))).ok()?)
    }

    /// Whether the map is on disk already (the prefetch skips it).
    pub fn has(&self, id: u32) -> bool {
        self.folder.join(format!("{id}.json")).exists()
    }

    /// The saved list of every map, and how many seconds old it is.
    pub fn catalog(&self) -> Option<(Vec<Candidate>, f64)> {
        let path = self.folder.join("catalog.json");
        let age = std::fs::metadata(&path).ok()?.modified().ok()?.elapsed().ok()?.as_secs_f64();
        let catalog = catalog_from_json(&std::fs::read_to_string(path).ok()?);
        (!catalog.is_empty()).then_some((catalog, age))
    }

    pub fn save_catalog(&self, catalog: &[Candidate]) {
        self.write("catalog.json", &catalog_to_json(catalog));
    }

    pub fn save(&self, info: &MapInfo) {
        self.write(&format!("{}.json", info.id), &map_to_json(info));
    }

    fn write(&self, name: &str, text: &str) {
        if let Err(error) = paths::write_atomic(&self.folder.join(name), text.as_bytes()) {
            crate::warn!("could not write the map cache {name}: {error}");
        }
    }
}
