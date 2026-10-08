//! Сборщик мусора: выгрузка чанков и сохранение на диск.
use bevy::prelude::*;

use crate::voxel::format::POOL_CHUNK_COUNT;
use crate::voxel::pool::ReadWorld;
use crate::io::channels::SaveRequest;
use crate::manager::{ChunkManager, SlotState};

/// Система сборки мусора.
pub fn chunk_garbage_collector_system(
    mut manager: ResMut<ChunkManager>,
    read_world: Res<ReadWorld>,
) {
    for slot in 0..POOL_CHUNK_COUNT {
        let (state, is_active, unload_timer, file_dirty, grid_x, grid_z) = {
            let meta = &manager.metadata[slot];
            (
                meta.state,
                meta.is_active,
                meta.unload_timer,
                meta.file_dirty,
                meta.grid_x,
                meta.grid_z,
            )
        };

        // Работаем только с готовыми чанками.
        if state != SlotState::Ready {
            continue;
        }

        // Если чанк активен — не выгружаем.
        if is_active {
            continue;
        }

        // Тикаем таймер выгрузки.
        if unload_timer > 0 {
            manager.metadata[slot].unload_timer = unload_timer - 1;
            continue;
        }

        // Таймер исчерпан — выгружаем чанк.
        if file_dirty {
            // Чанк грязный — отправляем на сохранение.
            let data = read_world.0.chunk_slice(slot).to_vec();

            manager.save_queue.push(SaveRequest {
                slot,
                chunk_x: grid_x as usize,
                chunk_z: grid_z as usize,
                data,
            });

            manager.set_slot_state(slot, SlotState::AwaitingSave);
        } else {
            // Чанк чистый — освобождаем слот.
            manager.release_slot(slot);
        }
    }
}