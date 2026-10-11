//! Read-side snapshot активного окна.
//!
//! Без Arc, без Atomic, без unsafe.

use bevy::prelude::*;

use crate::voxel::format::{POOL_CHUNK_COUNT, CHUNK_HEIGHT};
use crate::voxel::topology::TorusTopology;

use super::{ChunkManager, SlotMetadata, SlotState};

#[derive(Clone, Debug)]
pub struct ReadSlotSnapshot {
    pub grid_x: i64,
    pub grid_z: i64,
    pub state: SlotState,
    pub generation: u64,

    /// Высотные границы содержимого чанка. Используются мешированием, чтобы
    /// не сканировать пустые субчанки. Живут в снапшоте, а не в `ChunkManager`,
    /// потому что иначе система мешей получила бы конфликт доступа с пост-свапом.
    pub min_solid_y: u16,
    pub max_solid_y: u16,
    pub min_liquid_y: u16,
    pub max_liquid_y: u16,

    /// Границы недостоверны (чанк правили после загрузки) — сканировать весь чанк.
    pub bounds_stale: bool,
}

impl ReadSlotSnapshot {
    #[inline]
    pub fn free() -> Self {
        Self {
            grid_x: -1,
            grid_z: -1,
            state: SlotState::Empty,
            generation: 0,
            min_solid_y: CHUNK_HEIGHT as u16,
            max_solid_y: 0,
            min_liquid_y: CHUNK_HEIGHT as u16,
            max_liquid_y: 0,
            bounds_stale: true,
        }
    }

    #[inline]
    pub fn from_metadata(meta: &SlotMetadata) -> Self {
        Self {
            grid_x: meta.grid_x,
            grid_z: meta.grid_z,
            state: meta.get_state(),
            generation: meta.generation,
            min_solid_y: meta.min_solid_y,
            max_solid_y: meta.max_solid_y,
            min_liquid_y: meta.min_liquid_y,
            max_liquid_y: meta.max_liquid_y,
            bounds_stale: meta.bounds_stale,
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
    pub fn content_y_range(&self, slot: usize) -> Option<(u16, u16)> {
        let snap = &self.snapshots[slot];

        if snap.bounds_stale {
            return None;
        }

        Some((
            snap.min_solid_y.min(snap.min_liquid_y),
            snap.max_solid_y.max(snap.max_liquid_y),
        ))
    }
}