//! Координатор чанков и зон.

use bevy::prelude::*;

use crate::voxel::format::{Cell, POOL_CHUNK_COUNT, WINDOW_SIDE};
use crate::voxel::pool::ChunkPool;
use crate::voxel::topology::TorusTopology;
use crate::io::channels::{LoadRequest, SaveRequest};
use super::metadata::{SlotMetadata, SlotState};

/// Тонкий координатор (без тяжёлых данных)
#[derive(Resource)]
pub struct ChunkManager {
    pub metadata: Vec<SlotMetadata>,
    pub topology: TorusTopology,

    // Границы активного окна
    pub window_min_x: i64,
    pub window_min_z: i64,
    pub window_max_x: i64,
    pub window_max_z: i64,

    // Центр окна (позиция игрока)
    pub window_center_x: i64,
    pub window_center_z: i64,

    // Очереди запросов
    pub load_queue: Vec<LoadRequest>,
    pub save_queue: Vec<SaveRequest>,
}

impl ChunkManager {
    pub fn new(topology: TorusTopology) -> Self {
        let metadata = (0..POOL_CHUNK_COUNT).map(|_| SlotMetadata::new()).collect();
        Self {
            metadata,
            topology,
            window_min_x: 0,
            window_min_z: 0,
            window_max_x: 0,
            window_max_z: 0,
            window_center_x: 0,
            window_center_z: 0,
            load_queue: Vec::new(),
            save_queue: Vec::new(),
        }
    }

    /// Устанавливает центр окна и пересчитывает границы
    pub fn set_window_center(&mut self, center_x: i64, center_z: i64) {
        self.window_center_x = center_x;
        self.window_center_z = center_z;

        let half = (WINDOW_SIDE / 2) as i64;
        self.window_min_x = center_x - half;
        self.window_min_z = center_z - half;
        self.window_max_x = center_x + half;
        self.window_max_z = center_z + half;
    }

    /// Проверяет, входит ли чанк в активное окно
    #[inline(always)]
    pub fn is_in_window(&self, chunk_x: usize, chunk_z: usize) -> bool {
        let cx = chunk_x as i64;
        let cz = chunk_z as i64;
        cx >= self.window_min_x
            && cx <= self.window_max_x
            && cz >= self.window_min_z
            && cz <= self.window_max_z
    }

    /// Возвращает индекс слота для чанка в активном окне
    #[inline(always)]
    pub fn slot_for_chunk(&self, chunk_x: usize, chunk_z: usize) -> Option<usize> {
        self.topology.slot_index_in_window(
            chunk_x,
            chunk_z,
            self.window_min_x,
            self.window_min_z,
        )
    }

    #[inline(always)]
    pub fn get_slot_state(&self, slot: usize) -> SlotState {
        self.metadata[slot].get_state()
    }

    #[inline(always)]
    pub fn set_slot_state(&self, slot: usize, state: SlotState) {
        self.metadata[slot].set_state(state);
    }

    #[inline(always)]
    pub fn is_pending_b_dirty(&self, slot: usize) -> bool {
        self.metadata[slot].pending_b_dirty.load(std::sync::atomic::Ordering::Acquire)
    }

    #[inline(always)]
    pub fn set_pending_b_dirty(&self, slot: usize, dirty: bool) {
        self.metadata[slot].pending_b_dirty.store(dirty, std::sync::atomic::Ordering::Release);
    }

    #[inline(always)]
    pub fn is_file_dirty(&self, slot: usize) -> bool {
        self.metadata[slot].file_dirty.load(std::sync::atomic::Ordering::Acquire)
    }

    #[inline(always)]
    pub fn set_file_dirty(&self, slot: usize, dirty: bool) {
        self.metadata[slot].file_dirty.store(dirty, std::sync::atomic::Ordering::Release);
    }

    /// Проверка: есть ли хоть один слот в состоянии Saving
    pub fn has_saving_slots(&self) -> bool {
        self.metadata.iter().any(|m| m.get_state() == SlotState::Saving)
    }

    /// Чтение ячейки из пула чтения
    pub fn read_cell<'a>(
        &self,
        read_world: &'a ChunkPool,
        x: i64,
        y: i64,
        z: i64,
    ) -> Option<&'a Cell> {
        let (cx, cz, lx, ly, lz) = self.topology.normalize_block(x, y, z)?;

        let slot = self.slot_for_chunk(cx, cz)?;
        let cell_idx = self.topology.cell_index(lx, ly, lz);

        if self.get_slot_state(slot) != SlotState::Ready {
            return None;
        }

        Some(read_world.get(slot, cell_idx))
    }

    /// Запись ячейки в пул записи
    pub fn write_cell(
        &self,
        write_world: &mut ChunkPool,
        x: i64,
        y: i64,
        z: i64,
        cell: Cell,
    ) -> bool {
        let Some((cx, cz, lx, ly, lz)) = self.topology.normalize_block(x, y, z) else {
            return false;
        };

        let Some(slot) = self.slot_for_chunk(cx, cz) else {
            return false;
        };

        let cell_idx = self.topology.cell_index(lx, ly, lz);

        if self.get_slot_state(slot) != SlotState::Ready {
            return false;
        }

        let _ = std::mem::replace(write_world.get_mut(slot, cell_idx), cell);
        self.set_pending_b_dirty(slot, true);

        true
    }

    /// Возвращает итератор по слотам активного окна.
    #[inline]
    pub fn active_window_slots(&self) -> std::ops::Range<usize> {
        0..crate::voxel::format::WINDOW_CHUNK_COUNT
    }
    
    /// Возвращает координаты чанка для слота активного окна.
    #[inline]
    pub fn slot_to_chunk_coords(&self, slot: usize) -> Option<(i64, i64)> {
        use crate::voxel::format::WINDOW_SIDE;

        if slot >= WINDOW_SIDE * WINDOW_SIDE {
            return None;
        }

        let dx = slot % WINDOW_SIDE;
        let dz = slot / WINDOW_SIDE;

        let chunk_x = self.window_min_x + dx as i64;
        let chunk_z = self.window_min_z + dz as i64;

        let (norm_x, norm_z) = self.topology.normalize_chunk(chunk_x, chunk_z);
        Some((norm_x as i64, norm_z as i64))
    }
}