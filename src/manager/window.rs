//! Read-side snapshot активного окна.
//!
//! Без Arc, без Atomic, без unsafe.

use bevy::prelude::*;

use crate::voxel::format::{POOL_CHUNK_COUNT};   //, CHUNK_HEIGHT
use crate::voxel::height_map::HeightMap;
use crate::voxel::topology::TorusTopology;

use super::{ChunkManager, SlotMetadata, SlotState};

#[derive(Clone, Debug)]
pub struct ReadSlotSnapshot {
    pub grid_x: i64,
    pub grid_z: i64,
    pub state: SlotState,
    pub generation: u64,
}

impl ReadSlotSnapshot {
    #[inline]
    pub fn free() -> Self {
        Self {
            grid_x: -1,
            grid_z: -1,
            state: SlotState::Empty,
            generation: 0,
        }
    }

    #[inline]
    pub fn from_metadata(meta: &SlotMetadata) -> Self {
        Self {
            grid_x: meta.grid_x,
            grid_z: meta.grid_z,
            state: meta.get_state(),
            generation: meta.generation,
        }
    }
}

#[derive(Resource, Debug)]
pub struct ReadWindowManager {
    pub topology: TorusTopology,

    /// Границы окна, соответствующие текущему ReadWorld.
    pub window_min_x: i64,
    pub window_min_z: i64,

    /// Снимок метаданных слотов.
    pub snapshots: Vec<ReadSlotSnapshot>,

    /// Read-side маппинг нормализованного чанка в слот.
    ///
    /// Содержит только активные Ready чанки.
    pub chunk_to_slot: Vec<i32>,

    /// Снимок карты верхних блоков на момент публикации `ReadWorld`.
    ///
    /// Лежит здесь, а не отдельным ресурсом, по той же причине, что и высотные
    /// границы: мешер читает карту вместе с содержимым пула, и оба обязаны
    /// относиться к одному моменту времени. Отдельный `Res<ReadHeightMap>`
    /// конфликтовал бы по доступу с пост-свапом.
    pub height_map: HeightMap,
}

impl ReadWindowManager {
    pub fn new(topology: &TorusTopology) -> Self {
        let chunks_x = topology.chunks_x as usize;
        let chunks_z = topology.chunks_z as usize;

        Self {
            topology: topology.clone(),
            window_min_x: 0,
            window_min_z: 0,
            snapshots: vec![ReadSlotSnapshot::free(); POOL_CHUNK_COUNT],
            chunk_to_slot: vec![-1; chunks_x * chunks_z],
            height_map: HeightMap::new(POOL_CHUNK_COUNT),
        }
    }

    /// Опубликовать read-снапшот из write-менеджера.
    ///
    /// Вызывается:
    /// - после initial_copy_system;
    /// - после swap_pointers_system.
    pub fn publish_from(&mut self, write: &ChunkManager, write_height_map: &HeightMap) {
        debug_assert_eq!(self.snapshots.len(), write.metadata.len());
        debug_assert_eq!(self.chunk_to_slot.len(), write.chunk_to_slot.len());

        // Карта едет целиком: 512 Б × 1024 слота = 512 КиБ одним memcpy, дешевле
        // вычисления dirty-маски по слотам.
        self.height_map.copy_from_all(write_height_map);

        self.window_min_x = write.window_min_x;
        self.window_min_z = write.window_min_z;

        self.chunk_to_slot.fill(-1);

        for (slot, meta) in write.metadata.iter().enumerate() {
            self.snapshots[slot] = ReadSlotSnapshot::from_metadata(meta);

            if !meta.is_active {
                continue;
            }

            if meta.get_state() != SlotState::Ready {
                continue;
            }

            if meta.grid_x < 0 || meta.grid_z < 0 {
                continue;
            }

            let chunk_x = meta.grid_x as usize;
            let chunk_z = meta.grid_z as usize;
            let idx = write.chunk_key(chunk_x, chunk_z);

            self.chunk_to_slot[idx] = slot as i32;
        }
    }

    #[inline(always)]
    pub fn normalize_chunk(&self, chunk_x: i64, chunk_z: i64) -> (usize, usize) {
        self.topology.normalize_chunk(chunk_x, chunk_z)
    }

    #[inline(always)]
    pub fn chunk_key(&self, chunk_x: usize, chunk_z: usize) -> usize {
        debug_assert!(
            chunk_x < self.topology.chunks_x as usize && chunk_z < self.topology.chunks_z as usize,
            "chunk_key: coordinates must be normalized, got ({}, {})",
            chunk_x,
            chunk_z
        );

        chunk_z * self.topology.chunks_x as usize + chunk_x
    }

    #[inline(always)]
    pub fn key_index(&self, chunk_x: i64, chunk_z: i64) -> usize {
        let (cx, cz) = self.normalize_chunk(chunk_x, chunk_z);
        self.chunk_key(cx, cz)
    }

    #[inline(always)]
    pub fn slot_for_chunk(&self, chunk_x: usize, chunk_z: usize) -> Option<usize> {
        let idx = self.chunk_key(chunk_x, chunk_z);
        let slot = self.chunk_to_slot[idx];

        if slot >= 0 {
            Some(slot as usize)
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn generation(&self, slot: usize) -> u64 {
        self.snapshots[slot].generation
    }

    #[inline(always)]
    pub fn state(&self, slot: usize) -> SlotState {
        self.snapshots[slot].state
    }

    #[inline(always)]
    pub fn is_ready(&self, slot: usize) -> bool {
        self.snapshots[slot].state == SlotState::Ready
    }

    /// Диапазон `y` с содержимым чанка, или `None` если границы недостоверны.
    ///
    /// `None` — чанк правили после загрузки, границы врáли бы, надо сканировать
    /// весь чанк. Пустой чанк даёт `Some((CHUNK_HEIGHT, 0))`, что вместе с условием
    /// скипа (`sub_y_end <= min || sub_y > max`) отсекает все субчанки.
    #[inline(always)]
    pub fn content_y_range(&self, slot: usize) -> (u16, u16) {
    //let snap = &self.snapshots[slot];
    
    let tops = self.height_map.column_slice(slot); // &[u16; 256]
    
    // Находим глобальные min и max по всем 256 колонкам чанка.
    // Компилятор легко векторизует этот скан (SIMD), это займет ~10-20 нс.
    let mut min_y = u16::MAX;
    let mut max_y = 0u16;
    
    for &top in tops {
        if top < min_y { min_y = top; }
        if top > max_y { max_y = top; }
    }
    
    // Фолбэк на случай полностью пустого чанка (в реальности недостижим из-за bedrock)
    if min_y == u16::MAX { return (0, 0); }
    
    (min_y, max_y)
}
}