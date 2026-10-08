//! Координатор чанков и зон.
use bevy::prelude::*;

use crate::voxel::format::{Cell, POOL_CHUNK_COUNT, WINDOW_SIDE};
use crate::voxel::pool::ChunkPool;
use crate::voxel::topology::TorusTopology;
use crate::io::channels::{LoadRequest, SaveRequest};

use super::metadata::{SlotMetadata, SlotState};

/// Задержка выгрузки неактивного чанка в кадрах.
pub const UNLOAD_DELAY_FRAMES: u32 = 60;

/// Тонкий координатор (без тяжёлых данных).
#[derive(Resource)]
pub struct ChunkManager {
    pub metadata: Vec<SlotMetadata>,
    pub topology: TorusTopology,

    /// Маппинг нормализованных координат чанка в физический слот пула.
    ///
    /// Индекс: `chunk_z * chunks_x + chunk_x`.
    /// Значение: `slot` или `-1`, если чанк не загружен.
    pub chunk_to_slot: Vec<i32>,

    // Границы активного окна (ненормализованные, могут быть отрицательными).
    pub window_min_x: i64,
    pub window_min_z: i64,
    pub window_max_x: i64,
    pub window_max_z: i64,

    // Центр окна в нормализованных координатах чанка.
    pub window_center_x: i64,
    pub window_center_z: i64,

    // Очереди запросов.
    pub load_queue: Vec<LoadRequest>,
    pub save_queue: Vec<SaveRequest>,
}

impl ChunkManager {
    pub fn new(topology: TorusTopology) -> Self {
        let chunks_x = topology.chunks_x as usize;
        let chunks_z = topology.chunks_z as usize;

        let metadata = (0..POOL_CHUNK_COUNT).map(|_| SlotMetadata::new()).collect();
        let chunk_to_slot = vec![-1; chunks_x * chunks_z];

        Self {
            metadata,
            topology,
            chunk_to_slot,
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

    #[inline(always)]
    pub fn chunk_key(&self, chunk_x: usize, chunk_z: usize) -> usize {
        chunk_z * self.topology.chunks_x as usize + chunk_x
    }

    #[inline(always)]
    pub fn key_index(&self, chunk_x: i64, chunk_z: i64) -> usize {
        let (cx, cz) = self.topology.normalize_chunk(chunk_x, chunk_z);
        self.chunk_key(cx, cz)
    }

    /// Устанавливает центр окна и пересчитывает границы.
    pub fn set_window_center(&mut self, center_x: i64, center_z: i64) {
        self.window_center_x = center_x;
        self.window_center_z = center_z;

        let half = (WINDOW_SIDE / 2) as i64;

        self.window_min_x = center_x - half;
        self.window_min_z = center_z - half;
        self.window_max_x = center_x + half;
        self.window_max_z = center_z + half;
    }

    /// Проверяет, входит ли нормализованный чанк в активное окно.
    #[inline(always)]
    pub fn is_in_window(&self, chunk_x: usize, chunk_z: usize) -> bool {
        let dx = (chunk_x as i64 - self.window_min_x).rem_euclid(self.topology.chunks_x as i64);
        let dz = (chunk_z as i64 - self.window_min_z).rem_euclid(self.topology.chunks_z as i64);

        dx < WINDOW_SIDE as i64 && dz < WINDOW_SIDE as i64
    }

    /// Возвращает физический слот для нормализованного чанка.
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

    /// Назначает чанк на свободный слот.
    ///
    /// Если чанк уже назначен — возвращает существующий слот.
    pub fn assign_chunk(&mut self, chunk_x: usize, chunk_z: usize) -> Option<usize> {
        if let Some(slot) = self.slot_for_chunk(chunk_x, chunk_z) {
            return Some(slot);
        }

        let slot = self
            .metadata
            .iter()
            .position(|meta| meta.state == SlotState::Empty)?;

        let idx = self.chunk_key(chunk_x, chunk_z);
        self.chunk_to_slot[idx] = slot as i32;

        let meta = &mut self.metadata[slot];
        *meta = SlotMetadata::new();
        meta.grid_x = chunk_x as i64;
        meta.grid_z = chunk_z as i64;
        meta.state = SlotState::Queued;
        meta.is_active = true;

        Some(slot)
    }

    /// Освобождает слот и убирает чанк из `chunk_to_slot`.
    pub fn release_slot(&mut self, slot: usize) {
        let meta = &self.metadata[slot];

        if meta.grid_x >= 0 && meta.grid_z >= 0 {
            let chunk_x = meta.grid_x as usize;
            let chunk_z = meta.grid_z as usize;
            let idx = self.chunk_key(chunk_x, chunk_z);

            if self.chunk_to_slot[idx] == slot as i32 {
                self.chunk_to_slot[idx] = -1;
            }
        }

        self.metadata[slot] = SlotMetadata::new();
    }

    #[inline(always)]
    pub fn get_slot_state(&self, slot: usize) -> SlotState {
        self.metadata[slot].get_state()
    }

    #[inline(always)]
    pub fn set_slot_state(&mut self, slot: usize, state: SlotState) {
        self.metadata[slot].set_state(state);
    }

    #[inline(always)]
    pub fn is_pending_b_dirty(&self, slot: usize) -> bool {
        self.metadata[slot].pending_b_dirty
    }

    #[inline(always)]
    pub fn set_pending_b_dirty(&mut self, slot: usize, dirty: bool) {
        self.metadata[slot].pending_b_dirty = dirty;
    }

    #[inline(always)]
    pub fn is_file_dirty(&self, slot: usize) -> bool {
        self.metadata[slot].file_dirty
    }

    #[inline(always)]
    pub fn set_file_dirty(&mut self, slot: usize, dirty: bool) {
        self.metadata[slot].file_dirty = dirty;
    }

    #[inline(always)]
    pub fn mark_subchunk_dirty(&mut self, slot: usize, subchunk_idx: usize) {
        self.metadata[slot].mark_subchunk_dirty(subchunk_idx);
    }

    /// Помечает чанк изменённым.
    ///
    /// Вызывать один раз на чанк за кадр, а не на каждую ячейку.
    #[inline(always)]
    pub fn touch_slot(&mut self, slot: usize) {
        let meta = &mut self.metadata[slot];
        meta.generation = meta.generation.wrapping_add(1);
        meta.pending_b_dirty = true;
    }

    /// Проверка: есть ли хоть один слот в состоянии Saving.
    pub fn has_saving_slots(&self) -> bool {
        self.metadata.iter().any(|m| m.get_state() == SlotState::Saving)
    }

    /// Чтение ячейки из пула чтения.
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

    /// Запись ячейки в пул записи.
    pub fn write_cell(
        &mut self,
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
}