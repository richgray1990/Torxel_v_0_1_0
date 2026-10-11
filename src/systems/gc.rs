//! Сборщик мусора: выгрузка чанков и сохранение на диск.
use bevy::prelude::*;

use crate::voxel::format::POOL_CHUNK_COUNT;
use crate::voxel::pool::ReadWorld;
use crate::io::channels::SaveRequest;
use crate::manager::{ChunkManager, SlotState, LOAD_TIMEOUT_FRAMES};
use crate::queues::mesh_queue::{MeshUpdateEvent, MeshUpdateKind, MeshUpdateQueue, RenderSource};

/// Система сборки мусора.
pub fn chunk_garbage_collector_system(
    mut manager: ResMut<ChunkManager>,
    read_world: Res<ReadWorld>,
    mut mesh_queue: ResMut<MeshUpdateQueue>,
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

        // Слот висит в Loading дольше таймаута — ответа не будет (воркер мёртв
        // или запрос потерян). Переводим в Failed, иначе initial_copy не завершится
        // и старт зависнет навсегда.
        if state == SlotState::Loading {
            let frames = manager.metadata[slot].load_frames + 1;

            if frames >= LOAD_TIMEOUT_FRAMES {
                manager.set_slot_state(slot, SlotState::Failed);
            } else {
                manager.metadata[slot].load_frames = frames;
            }

            continue;
        }

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

        // Провалившийся чанк вне окна держать нечем: gc его не выгружает (не Ready),
        // а рекью в update_window трогает только чанки нового окна. Освобождаем сразу,
        // иначе слот виснет в пуле навсегда. Активные Failed обрабатывает update_window.
        if state == SlotState::Failed {
            if !is_active {
                manager.release_slot(slot);
            }
            continue;
        }

        // Координаты должны быть валидны: слот без чанка (grid = -1) сохранять нечего,
        // а каст в usize дал бы usize::MAX и запись за пределами файла.
        if grid_x < 0 || grid_z < 0 {
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
            // Чанк чистый — снимаем меш и освобождаем слот.
            let meta = &manager.metadata[slot];
            mesh_queue.push(MeshUpdateEvent {
                kind: MeshUpdateKind::ChunkUnloaded,
                source: RenderSource::Active,
                slot_index: slot,
                grid_x: meta.grid_x,
                grid_z: meta.grid_z,
                dirty_subchunks: 0,
            });

            manager.release_slot(slot);
        }
    }
}