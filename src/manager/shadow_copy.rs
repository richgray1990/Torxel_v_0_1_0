//! Управление копированием данных из теневого пула в активный.
//!
//! Внешний код вызывает `request_copy()`. Система `shadow_copy_system`
//! проверяет готовность, валидирует геометрию и выполняет копирование.

use bevy::prelude::*;

use crate::voxel::format::WINDOW_SIDE;
use crate::voxel::shadow_pool::{ShadowPool, ShadowSlotState};

/// Состояние процесса копирования
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShadowCopyState {
    Idle,
    Validating,
    Copying,
    Complete,
    Failed,
}

/// Результат валидации
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShadowCopyValidation {
    Ready,
    Loading,
    Missing,
    GeometryMismatch,
}

/// Менеджер копирования из теневого пула в активный
#[derive(Resource)]
pub struct ShadowCopyManager {
    pub state: ShadowCopyState,
    pub target_center_x: i64,
    pub target_center_z: i64,
    pub target_min_x: i64,
    pub target_min_z: i64,
    pub target_max_x: i64,
    pub target_max_z: i64,
    pub pending_count: usize,
}

impl ShadowCopyManager {
    pub fn new() -> Self {
        Self {
            state: ShadowCopyState::Idle,
            target_center_x: 0,
            target_center_z: 0,
            target_min_x: 0,
            target_min_z: 0,
            target_max_x: 0,
            target_max_z: 0,
            pending_count: 0,
        }
    }

    /// Внешняя функция запроса копирования
    pub fn request_copy(&mut self, target_center_x: i64, target_center_z: i64) {
        if self.state == ShadowCopyState::Validating || self.state == ShadowCopyState::Copying {
            return;
        }

        self.target_center_x = target_center_x;
        self.target_center_z = target_center_z;

        let half = (WINDOW_SIDE / 2) as i64;
        self.target_min_x = target_center_x - half;
        self.target_min_z = target_center_z - half;
        self.target_max_x = target_center_x + half;
        self.target_max_z = target_center_z + half;

        self.state = ShadowCopyState::Validating;
        self.pending_count = 0;
    }

    #[inline]
    pub fn has_pending_request(&self) -> bool {
        self.state == ShadowCopyState::Validating || self.state == ShadowCopyState::Copying
    }

    /// Валидация геометрии и готовности данных
    pub fn validate(&mut self, shadow_pool: &ShadowPool) -> ShadowCopyValidation {
        let mut loading_count = 0;
        let mut missing_count = 0;

        for dz in 0..WINDOW_SIDE {
            for dx in 0..WINDOW_SIDE {
                let expected_chunk_x = self.target_min_x + dx as i64;
                let expected_chunk_z = self.target_min_z + dz as i64;

                let (norm_x, norm_z) = shadow_pool.topology.normalize_chunk(
                    expected_chunk_x,
                    expected_chunk_z,
                );
                let norm_x = norm_x as i64;
                let norm_z = norm_z as i64;

                match shadow_pool.slot_map.get(&(norm_x, norm_z)) {
                    Some(&slot) => {
                        let meta = &shadow_pool.meta[slot];

                        if meta.chunk_x != norm_x || meta.chunk_z != norm_z {
                            return ShadowCopyValidation::GeometryMismatch;
                        }

                        match meta.state {
                            ShadowSlotState::Ready => {}
                            ShadowSlotState::Loading => {
                                loading_count += 1;
                            }
                            ShadowSlotState::Empty => {
                                missing_count += 1;
                            }
                        }
                    }
                    None => {
                        missing_count += 1;
                    }
                }
            }
        }

        self.pending_count = loading_count + missing_count;

        if missing_count > 0 {
            ShadowCopyValidation::Missing
        } else if loading_count > 0 {
            ShadowCopyValidation::Loading
        } else {
            ShadowCopyValidation::Ready
        }
    }

    /// Копирование данных из теневого пула в активный
    pub fn execute_copy(
        &self,
        shadow_pool: &ShadowPool,
        active_pool_data: &mut [crate::voxel::format::Cell],
    ) -> bool {
        let cells_per_chunk = crate::voxel::format::CELLS_PER_CHUNK;
        print!("src/manager/shadow_copy.rs execute_copy called");
        for dz in 0..WINDOW_SIDE {
            for dx in 0..WINDOW_SIDE {
                let active_slot = dz * WINDOW_SIDE + dx;

                let expected_chunk_x = self.target_min_x + dx as i64;
                let expected_chunk_z = self.target_min_z + dz as i64;

                let (norm_x, norm_z) = shadow_pool.topology.normalize_chunk(
                    expected_chunk_x,
                    expected_chunk_z,
                );
                let norm_x = norm_x as i64;
                let norm_z = norm_z as i64;

                let shadow_slot = match shadow_pool.slot_map.get(&(norm_x, norm_z)) {
                    Some(&s) => s,
                    None => return false,
                };

                if shadow_pool.meta[shadow_slot].state != ShadowSlotState::Ready {
                    return false;
                }

                let src_start = shadow_slot * cells_per_chunk;
                let src_end = src_start + cells_per_chunk;
                let dst_start = active_slot * cells_per_chunk;
                let dst_end = dst_start + cells_per_chunk;

                active_pool_data[dst_start..dst_end]
                    .copy_from_slice(&shadow_pool.data[src_start..src_end]);
            }
        }
        print!("src/manager/shadow_copy.rs execute_copy done");
        true
    }

    /// Сброс состояния после завершения
    pub fn reset(&mut self) {
        self.state = ShadowCopyState::Idle;
        self.pending_count = 0;
    }
}

impl Default for ShadowCopyManager {
    fn default() -> Self {
        Self::new()
    }
}