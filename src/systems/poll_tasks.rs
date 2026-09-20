//! Опрос фоновых задач активного пула.

use bevy::prelude::*;

use crate::io::channels::IoManager;
use crate::io::staging::{ActiveStagingBuffer, StagedChunk};
use crate::voxel::pool::WriteWorld;
use crate::manager::{ChunkManager, SlotState};

/// Система опроса фоновых задач активного пула.
pub fn poll_background_tasks_system(
    io_manager: Res<IoManager>,
    mut staging: ResMut<ActiveStagingBuffer>,
    mut write_world: ResMut<WriteWorld>,
    mut manager: ResMut<ChunkManager>,
) {
    // Забираем ответы на загрузку из канала
    while let Some(response) = io_manager.try_recv_load_response() {
        if response.success {
            let meta = &manager.metadata[response.slot];
            staging.push_loaded(StagedChunk {
                slot: response.slot,
                grid_x: meta.grid_x,
                grid_z: meta.grid_z,
                data: response.data,
                min_solid_y: response.min_solid_y,
                max_solid_y: response.max_solid_y,
                min_liquid_y: response.min_liquid_y,
                max_liquid_y: response.max_liquid_y,
            });
        } else {
            manager.set_slot_state(response.slot, SlotState::Failed);
        }
    }

    // Забираем ответы на сохранение из канала
    while let Some(response) = io_manager.try_recv_save_response() {
        staging.push_saved(response);
    }

    // Применяем загруженные чанки к WriteWorld
    for staged_chunk in staging.take_loaded() {
        let slot = staged_chunk.slot;

        write_world.0.chunk_slice_mut(slot).copy_from_slice(&staged_chunk.data);

        manager.metadata[slot].min_solid_y = staged_chunk.min_solid_y;
        manager.metadata[slot].max_solid_y = staged_chunk.max_solid_y;
        manager.metadata[slot].min_liquid_y = staged_chunk.min_liquid_y;
        manager.metadata[slot].max_liquid_y = staged_chunk.max_liquid_y;

        manager.set_slot_state(slot, SlotState::Ready);

        println!(
            "[LOAD] Chunk at ({}, {}) → slot {} READY",
            staged_chunk.grid_x, staged_chunk.grid_z, staged_chunk.slot
        );
    }

    // Применяем результаты сохранения
    for save_response in staging.take_saved() {
        let slot = save_response.slot;
        if save_response.success {
            manager.set_slot_state(slot, SlotState::Ready);
            manager.set_file_dirty(slot, false);
        } else {
            manager.set_slot_state(slot, SlotState::Failed);
        }
    }
}