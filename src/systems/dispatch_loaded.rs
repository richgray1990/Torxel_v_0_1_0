//! Отправка событий рендеру о загрузке чанков активного пула.
//!
//! Запускается после свапа, когда чанки уже в ReadWorld.
//! Отправляет событие ChunkLoaded один раз на чанк.
//! Флаг render_notified защищает от повторной отправки при каждом свапе.
use bevy::prelude::*;

use crate::manager::{ChunkManager, SlotState};
use crate::queues::mesh_queue::{MeshUpdateEvent, MeshUpdateKind, MeshUpdateQueue, RenderSource};
use crate::voxel::format::{all_subchunks_mask, POOL_CHUNK_COUNT};

pub fn dispatch_loaded_events_system(
    mut manager: ResMut<ChunkManager>,
    mut mesh_queue: ResMut<MeshUpdateQueue>,
    initial_copy: Res<crate::systems::initial_copy::InitialCopyDone>,
) {
    // Не отправляем события пока initial_copy не завершён.
    // Иначе меш построится из пустого ReadWorld.
    if !initial_copy.done {
        return;
    }

    for slot in 0..POOL_CHUNK_COUNT {
        let (state, is_active, render_notified, grid_x, grid_z) = {
            let meta = &manager.metadata[slot];
            (
                meta.state,
                meta.is_active,
                meta.render_notified,
                meta.grid_x,
                meta.grid_z,
            )
        };

        // Чанк должен быть загружен.
        if state != SlotState::Ready {
            continue;
        }

        // Чанк должен быть в активном окне.
        if !is_active {
            continue;
        }

        // Не отправляем событие повторно.
        if render_notified {
            continue;
        }

        // Отправляем событие ChunkLoaded со всеми субчанками.
        mesh_queue.push(MeshUpdateEvent {
            kind: MeshUpdateKind::ChunkLoaded,
            source: RenderSource::Active,
            slot_index: slot,
            grid_x,
            grid_z,
            dirty_subchunks: all_subchunks_mask(),
        });

        // Помечаем, что уведомление отправлено.
        manager.metadata[slot].render_notified = true;
    }
}