//! Read-side snapshot активного окна.
//!
//! Без Arc, без Atomic, без unsafe.

use bevy::prelude::*;

use crate::voxel::format::POOL_CHUNK_COUNT;
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
        }
    }

    /// Опубликовать read-снапшот из write-менеджера.
    ///
    /// Вызывается:
    /// - после initial_copy_system;
    /// - после swap_pointers_system.
    pub fn publish_from(&mut self, write: &ChunkManager) {
        debug_assert_eq!(self.snapshots.len(), write.metadata.len());
        debug_assert_eq!(self.chunk_to_slot.len(), write.chunk_to_slot.len());

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
}