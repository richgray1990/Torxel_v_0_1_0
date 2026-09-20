//! Опрос фоновых задач теневого пула.

use bevy::prelude::*;

use crate::io::channels::IoManager;
use crate::io::staging::{ShadowStagedChunk, ShadowStagingBuffer};
use crate::voxel::shadow_pool::ShadowPool;

/// Система опроса теневых задач
pub fn poll_shadow_tasks_system(
    io_manager: Res<IoManager>,
    mut staging: ResMut<ShadowStagingBuffer>,
    mut shadow_pool: ResMut<ShadowPool>,
) {
    // Забираем ответы на теневую загрузку из канала
    while let Some(response) = io_manager.try_recv_shadow_load_response() {
        if response.success {
            staging.push_loaded(ShadowStagedChunk {
                slot: response.slot,
                chunk_x: response.chunk_x,
                chunk_z: response.chunk_z,
                data: response.data,
            });
        } else {
            shadow_pool.mark_failed(response.slot);
        }
    }

    // Применяем загруженные чанки к теневому пулу
    for staged_chunk in staging.take_loaded() {
        shadow_pool.apply_loaded_chunk(staged_chunk.slot, &staged_chunk.data);
    }

    // Тикаем счётчик кадров
    shadow_pool.tick_frame();
}