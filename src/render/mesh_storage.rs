//! Хранилище мешей субчанков.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::voxel::format::{SUBCHUNKS_PER_CHUNK, WINDOW_CHUNK_COUNT};

/// Максимум субчанков в хранилище: столько записей может быть одновременно.
pub const MAX_SUBCHUNKS: usize = WINDOW_CHUNK_COUNT * SUBCHUNKS_PER_CHUNK;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubchunkRenderState {
    Empty,
    Ready,
    Dirty,
    Rebuilding,
}

pub struct SubchunkRenderData {
    // Opaque
    pub current_opaque: Option<Handle<Mesh>>,
    pub pending_opaque: Option<Handle<Mesh>>,
    pub entity_opaque: Option<Entity>,
    pub pool_slot_opaque: Option<usize>,

    // Transparent
    pub current_transparent: Option<Handle<Mesh>>,
    pub pending_transparent: Option<Handle<Mesh>>,
    pub entity_transparent: Option<Entity>,
    pub pool_slot_transparent: Option<usize>,

    // AlphaTest
    pub current_alpha_test: Option<Handle<Mesh>>,
    pub pending_alpha_test: Option<Handle<Mesh>>,
    pub entity_alpha_test: Option<Entity>,
    pub pool_slot_alpha_test: Option<usize>,

    pub state: SubchunkRenderState,

    pub chunk_x: i64,
    pub chunk_z: i64,
    pub subchunk_index: usize,

    pub in_live: bool,
}

impl SubchunkRenderData {
    pub fn new(chunk_x: i64, chunk_z: i64, subchunk_index: usize) -> Self {
        Self {
            current_opaque: None,
            pending_opaque: None,
            entity_opaque: None,
            pool_slot_opaque: None,

            current_transparent: None,
            pending_transparent: None,
            entity_transparent: None,
            pool_slot_transparent: None,

            current_alpha_test: None,
            pending_alpha_test: None,
            entity_alpha_test: None,
            pool_slot_alpha_test: None,

            state: SubchunkRenderState::Empty,

            chunk_x,
            chunk_z,
            subchunk_index,

            in_live: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SubchunkKey {
    pub chunk_x: i64,
    pub chunk_z: i64,
    pub subchunk_index: usize,
}

/// Хранилище мешей и рабочие списки.
///
/// `subchunks` — источник истины. Списки лишь подсказывают, где есть работа:
/// состояния `Empty`/`Dirty`/`Rebuilding` уже описывают её, и список строится
/// в момент перехода, а не переискивается сканом каждый кадр.
///
/// Все списки аллоцируются один раз на `MAX_SUBCHUNKS`, `len` растёт и падает
/// при неизменной ёмкости — в установившемся режиме аллокаций нет.
///
/// Инвалидация ленивая: `ChunkUnloaded` списки не чистит (это был бы проход по
/// трём спискам на каждую выгрузку). Потребитель, не найдя ключ в `subchunks`,
/// просто выбрасывает его.
#[derive(Resource)]
pub struct MeshStorage {
    pub subchunks: HashMap<SubchunkKey, SubchunkRenderData>,

    /// Субчанки в состоянии `Empty` или `Dirty` — ждут `build_subchunk_mesh`.
    pub pending_build: Vec<SubchunkKey>,

    /// Субчанки в состоянии `Rebuilding` — ждут подмены `pending_mesh`.
    pub pending_swap: Vec<SubchunkKey>,

    /// Субчанки с мешем — единственные, кому нужна позиция сущности.
    pub live: Vec<SubchunkKey>,

    /// `live` изменился с прошлого кадра (добавление или удаление ключа).
    /// Меши неподвижны, поэтому это единственный триггер для
    /// `update_mesh_entities_system`: разбор списка нужен только когда в нём
    /// появилось или исчезло что-то новое.
    pub live_dirty: bool,
}

impl MeshStorage {
    pub fn new() -> Self {
        Self {
            subchunks: HashMap::with_capacity(MAX_SUBCHUNKS),
            pending_build: Vec::with_capacity(MAX_SUBCHUNKS),
            pending_swap: Vec::with_capacity(MAX_SUBCHUNKS),
            live: Vec::with_capacity(MAX_SUBCHUNKS),
            live_dirty: false,
        }
    }

    #[inline]
    pub fn get(&self, key: &SubchunkKey) -> Option<&SubchunkRenderData> {
        self.subchunks.get(key)
    }

    #[inline]
    pub fn get_mut(&mut self, key: &SubchunkKey) -> Option<&mut SubchunkRenderData> {
        self.subchunks.get_mut(key)
    }

    #[inline]
    pub fn insert(&mut self, key: SubchunkKey, data: SubchunkRenderData) {
        self.subchunks.insert(key, data);
    }

    #[inline]
    pub fn remove(&mut self, key: &SubchunkKey) -> Option<SubchunkRenderData> {
        self.subchunks.remove(key)
    }

    #[inline]
    pub fn contains(&self, key: &SubchunkKey) -> bool {
        self.subchunks.contains_key(key)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.subchunks.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.subchunks.is_empty()
    }

    /// Кладёт субчанк в `pending_build`. Дедуп даёт вызывающий: попасть сюда
    /// можно только из `Empty` или из `Ready` (переход в `Dirty`).
    #[inline]
    pub fn push_build(&mut self, key: SubchunkKey) {
        self.pending_build.push(key);
    }

    /// Кладёт субчанк в `pending_swap`. Вызывается только при переходе
    /// `Dirty → Rebuilding`, поэтому дублей не бывает.
    #[inline]
    pub fn push_swap(&mut self, key: SubchunkKey) {
        self.pending_swap.push(key);
    }

    /// Помечает субчанк живым (есть меш, нужна позиция). Идемпотентно.
    #[inline]
    pub fn mark_live(&mut self, key: SubchunkKey) {
        if let Some(data) = self.subchunks.get_mut(&key) {
            if !data.in_live {
                data.in_live = true;
                self.live.push(key);
                self.live_dirty = true;
            }
        }
    }
}

impl Default for MeshStorage {
    fn default() -> Self {
        Self::new()
    }
}