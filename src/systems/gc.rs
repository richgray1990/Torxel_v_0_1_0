//! Сборщик мусора: выгрузка чанков и сохранение на диск.
use bevy::prelude::*;
use crate::voxel::format::POOL_CHUNK_COUNT;
use crate::voxel::pool::ReadWorld;
use crate::io::channels::SaveRequest;
use crate::manager::{ChunkManager, SlotState};

/// Система сборки мусора
pub fn chunk_garbage_collector_system(
    mut manager: ResMut<ChunkManager>,
    read_world: Res<ReadWorld>,
) {
    for slot in 0..POOL_CHUNK_COUNT {
        // Читаем все нужные значения ДО любых мутаций.
        let state = manager.metadata[slot].state;

        // Работаем только с готовыми чанками
        if state != SlotState::Ready {
            continue;
        }

        // Если чанк в активном или теневом окне — не выгружаем
        if manager.metadata[slot].is_active {
            continue;
        }

        // Тикаем таймер выгрузки
        let timer = manager.metadata[slot].unload_timer;
        if timer > 0 {
            manager.metadata[slot].unload_timer = timer - 1;
            continue;
        }

        // Таймер исчерпан — выгружаем чанк
        let file_dirty = manager.metadata[slot].file_dirty;
        let grid_x = manager.metadata[slot].grid_x;
        let grid_z = manager.metadata[slot].grid_z;

        if file_dirty {
            // Чанк грязный — отправляем на сохранение
            let data = read_world.0.chunk_slice(slot).to_vec();
            manager.save_queue.push(SaveRequest {
                slot,
                chunk_x: grid_x as usize,
                chunk_z: grid_z as usize,
                data,
            });
            manager.set_slot_state(slot, SlotState::AwaitingSave);
        } else {
            // Чанк чистый — просто освобождаем слот
            // Сбрасываем флаг уведомления рендера,
            // чтобы при повторной загрузке событие отправилось снова
            manager.metadata[slot].render_notified = false;
            manager.set_slot_state(slot, SlotState::Empty);
        }
    }
}