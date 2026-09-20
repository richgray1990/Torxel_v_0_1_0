//! Пулы памяти для чанков.

use bevy::prelude::*;

use super::format::{Cell, CELLS_PER_CHUNK, POOL_CHUNK_COUNT};
use bytemuck::Zeroable;

/// Пул чанков — плоский массив ячеек
pub struct ChunkPool {
    pub data: Box<[Cell]>,
    pub slot_count: usize,
}

impl ChunkPool {
    pub fn new(slot_count: usize) -> Self {
        let total = slot_count * CELLS_PER_CHUNK;
        let mut vec = Vec::with_capacity(total);
        vec.resize(total, Cell::zeroed());
        Self {
            data: vec.into_boxed_slice(),
            slot_count,
        }
    }

    pub fn new_max() -> Self {
        Self::new(POOL_CHUNK_COUNT)
    }

    #[inline(always)]
    pub fn get(&self, slot: usize, cell_index: usize) -> &Cell {
        debug_assert!(slot < self.slot_count, "slot out of range");
        debug_assert!(cell_index < CELLS_PER_CHUNK, "cell_index out of range");
        &self.data[slot * CELLS_PER_CHUNK + cell_index]
    }

    #[inline(always)]
    pub fn get_mut(&mut self, slot: usize, cell_index: usize) -> &mut Cell {
        debug_assert!(slot < self.slot_count, "slot out of range");
        debug_assert!(cell_index < CELLS_PER_CHUNK, "cell_index out of range");
        &mut self.data[slot * CELLS_PER_CHUNK + cell_index]
    }

    #[inline]
    pub fn copy_chunk_from(&mut self, src: &ChunkPool, slot: usize) {
        debug_assert!(slot < self.slot_count, "slot out of range");
        debug_assert!(slot < src.slot_count, "source slot out of range");
        let start = slot * CELLS_PER_CHUNK;
        let end = start + CELLS_PER_CHUNK;
        self.data[start..end].copy_from_slice(&src.data[start..end]);
    }

    #[inline]
    pub fn copy_chunk_within(&mut self, from_slot: usize, to_slot: usize) {
        debug_assert!(from_slot < self.slot_count, "from_slot out of range");
        debug_assert!(to_slot < self.slot_count, "to_slot out of range");
        let from_start = from_slot * CELLS_PER_CHUNK;
        let from_end = from_start + CELLS_PER_CHUNK;
        let to_start = to_slot * CELLS_PER_CHUNK;
        self.data.copy_within(from_start..from_end, to_start);
    }

    #[inline]
    pub fn fill_chunk_air(&mut self, slot: usize) {
        debug_assert!(slot < self.slot_count, "slot out of range");
        let start = slot * CELLS_PER_CHUNK;
        let end = start + CELLS_PER_CHUNK;
        self.data[start..end].fill(Cell::zeroed());
    }

    #[inline]
    pub fn chunk_slice(&self, slot: usize) -> &[Cell] {
        debug_assert!(slot < self.slot_count, "slot out of range");
        let start = slot * CELLS_PER_CHUNK;
        let end = start + CELLS_PER_CHUNK;
        &self.data[start..end]
    }

    #[inline]
    pub fn chunk_slice_mut(&mut self, slot: usize) -> &mut [Cell] {
        debug_assert!(slot < self.slot_count, "slot out of range");
        let start = slot * CELLS_PER_CHUNK;
        let end = start + CELLS_PER_CHUNK;
        &mut self.data[start..end]
    }
}

impl Default for ChunkPool {
    fn default() -> Self {
        Self::new_max()
    }
}

/// Ресурс мира для чтения
#[derive(Resource)]
pub struct ReadWorld(pub ChunkPool);

/// Ресурс мира для записи
#[derive(Resource)]
pub struct WriteWorld(pub ChunkPool);