//! Агрегация событий изменений ячеек.
//!
//! Пока система называется `apply_cell_events_system`.
//! Позже, когда добавим `WindowWriteQueue`, переименуем в `aggregate_system`.
use bevy::prelude::*;

use crate::manager::{ChunkManager, DirtyMask, SlotState};
use crate::queues::cell_events::CellEventQueue;
use crate::voxel::format::subchunk_index_from_cell;
use crate::voxel::pool::WriteWorld;

/// Система применения событий ячеек к WriteWorld.
///
/// Текущий Aggregate:
/// - проверяет состояние слота;
/// - проверяет `expected_generation`, если оно задано;
/// - пишет ячейку в `WriteWorld`;
/// - помечает соответствующий субчанк грязным;
/// - один раз на затронутый чанк вызывает `touch_slot()`;
/// - `touch_slot()` инкрементит `generation` и ставит `pending_b_dirty`.
pub fn apply_cell_events_system(
    mut write_world: ResMut<WriteWorld>,
    mut cell_events: ResMut<CellEventQueue>,
    mut manager: ResMut<ChunkManager>,
) {
    let mut touched_slots = DirtyMask::new();

    for event in cell_events.drain() {
        let slot = event.slot_index;

        // Не пишем в незагруженный / неактивный слот.
        if manager.get_slot_state(slot) != SlotState::Ready {
            continue;
        }

        // Optimistic concurrency check.
        //
        // Если событие пришло из снапшота с конкретным generation,
        // применяем только когда текущий generation совпал.
        if let Some(expected_generation) = event.expected_generation {
            if manager.metadata[slot].generation != expected_generation {
                // Устаревшее событие.
                // Пока просто отбрасываем.
                // Позже можно возвращать GenConflict владельцу lease / системы.
                continue;
            }
        }

        // Пишем новую ячейку в write-side мир.
        *write_world.0.get_mut(slot, event.cell_index) = event.cell;

        // Определяем, какой субчанк нужно перестроить.
        let subchunk_idx = subchunk_index_from_cell(event.cell_index);

        // Помечаем субчанк грязным.
        manager.mark_subchunk_dirty(slot, subchunk_idx);

        // Запоминаем, что чанк был затронут.
        touched_slots.set(slot);
    }

    // Финализируем изменения:
    // generation++ и pending_b_dirty = true ровно один раз на чанк.
    for slot in touched_slots.iter_set() {
        manager.touch_slot(slot);
    }
}