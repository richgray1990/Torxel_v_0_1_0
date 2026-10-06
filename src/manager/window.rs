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
}

impl ReadWindowManager {
    pub fn new(topology: &TorusTopology) -> Self {
        Self {
            topology: topology.clone(),
            window_min_x: 0,
            window_min_z: 0,
            snapshots: vec![ReadSlotSnapshot::free(); POOL_CHUNK_COUNT],
        }
    }

    /// Опубликовать read-снапшот из write-менеджера.
    ///
    /// Вызывается:
    /// - после initial_copy_system;
    /// - после swap_pointers_system.
    pub fn publish_from(&mut self, write: &ChunkManager) {
        debug_assert_eq!(self.snapshots.len(), write.metadata.len());

        self.window_min_x = write.window_min_x;
        self.window_min_z = write.window_min_z;

        for (slot, meta) in write.metadata.iter().enumerate() {
            self.snapshots[slot] = ReadSlotSnapshot::from_metadata(meta);
        }
    }

    #[inline(always)]
    pub fn normalize_chunk(&self, chunk_x: i64, chunk_z: i64) -> (usize, usize) {
        self.topology.normalize_chunk(chunk_x, chunk_z)
    }

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