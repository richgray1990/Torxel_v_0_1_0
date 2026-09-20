//! Сборщик мусора: выгрузка чанков и сохранение на диск.

use bevy::prelude::*;

use crate::voxel::format::POOL_CHUNK_COUNT;
use crate::voxel::pool::ReadWorld;
use crate::io::channels::{IoManager, SaveRequest};
use crate::manager::{ChunkManager, SlotState};

/// Задержка выгрузки в кадрах
const UNLOAD_DELAY_FRAMES: u32 = 60;

/// Система сборки мусора
pub fn chunk_garbage_collector_system(
    mut manager: ResMut<ChunkManager>,
    read_world: Res<ReadWorld>,
    io_manager: Res<IoManager>,
) {
    for slot in 0..POOL_CHUNK_COUNT {
        let meta = &manager.metadata[slot];
        let state = meta.get_state();

        // Работаем только с готовыми чанками
        if state != SlotState::Ready {
            continue;
        }

        // Если чанк в активном или теневом окне — не выгружаем
        if meta.is_active() || meta.is_shadow() {
            continue;
        }

        // Тикаем таймер выгрузки
        let timer = meta.get_unload_timer();
        if timer > 0 {
            meta.decrement_unload_timer();
            continue;
        }

        // Таймер исчерпан — выгружаем чанк
        if meta.file_dirty.load(std::sync::atomic::Ordering::Acquire) {
            // Чанк грязный — отправляем на сохранение
            let data = read_world.0.chunk_slice(slot).to_vec();

            let chunk_x = meta.grid_x as usize;
            let chunk_z = meta.grid_z as usize;

            manager.save_queue.push(SaveRequest {
                slot,
                chunk_x,
                chunk_z,
                data,
            });

            manager.set_slot_state(slot, SlotState::AwaitingSave);
        } else {
            // Чанк чистый — просто освобождаем слот
            manager.set_slot_state(slot, SlotState::Empty);
        }
    }
}