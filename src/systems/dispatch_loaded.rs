//! Отправка событий рендеру о загрузке чанков активного пула.
//!
//! Запускается после свапа, когда чанки уже в ReadWorld.
//! Отправляет событие ChunkLoaded один раз на чанк.
//! Флаг render_notified защищает от повторной отправки при каждом свапе.

use bevy::prelude::*;

use crate::manager::{ChunkManager, SlotState};
use crate::queues::mesh_queue::{MeshUpdateEvent, MeshUpdateKind, MeshUpdateQueue, RenderSource};
use crate::voxel::format::{all_subchunks_mask, WINDOW_CHUNK_COUNT};

pub fn dispatch_loaded_events_system(
    manager: Res<ChunkManager>,
    mut mesh_queue: ResMut<MeshUpdateQueue>,
    initial_copy: Res<crate::systems::initial_copy::InitialCopyDone>,
) {
    // Не отправляем события пока initial_copy не завершён
    // Иначе меш построится из пустого ReadWorld
    if !initial_copy.done {
        return;
    }

    for slot in 0..WINDOW_CHUNK_COUNT {
        // Чанк должен быть загружен
        if manager.get_slot_state(slot) != SlotState::Ready {
            continue;
        }

        // Чанк должен быть в активном пуле (не теневой)
        if !manager.metadata[slot].is_active() {
            continue;
        }

        // Не отправляем событие повторно
        if manager.metadata[slot].is_render_notified() {
            continue;
        }

        // Отправляем событие ChunkLoaded со всеми субчанками
        mesh_queue.push(MeshUpdateEvent {
            kind: MeshUpdateKind::ChunkLoaded,
            source: RenderSource::Active,
            slot_index: slot,
            grid_x: manager.metadata[slot].grid_x,
            grid_z: manager.metadata[slot].grid_z,
            dirty_subchunks: all_subchunks_mask(),
        });

        // Помечаем что уведомление отправлено
        manager.metadata[slot].set_render_notified(true);
    }
}