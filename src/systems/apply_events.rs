//! Агрегация событий изменений ячеек.
//!
//! Пока система называется apply_cell_events_system.
//! Позже, когда добавим WindowWriteQueue, переименуем в aggregate_system.
use crate::manager::{ChunkManager, DirtyMask, SlotState};
use crate::queues::cell_events::CellEventQueue;
use crate::systems::perf::{Bucket, PerfStats};
use crate::voxel::format::{CHUNK_HEIGHT, CHUNK_SIDE, subchunk_index_from_cell};
use crate::voxel::height_map::{WriteHeightMap, column_index, recompute_column};
use crate::voxel::pool::WriteWorld;
use bevy::prelude::*;
/// Система применения событий ячеек к WriteWorld.
///
/// Текущий Aggregate:
/// - проверяет состояние слота;
/// - проверяет expected_generation, если оно задано;
/// - пишет ячейку в WriteWorld;
/// - обновляет карту верхних блоков затронутых колонок;
/// - помечает соответствующий субчанк грязным;
/// - один раз на затронутый чанк вызывает touch_slot();
/// - touch_slot() инкрементит generation и ставит pending_b_dirty.
pub fn apply_cell_events_system(
    mut write_world: ResMut<WriteWorld>,
    mut write_height_map: ResMut<WriteHeightMap>,
    mut cell_events: ResMut<CellEventQueue>,
    mut manager: ResMut<ChunkManager>,
    stats: Res<PerfStats>,
) {
    let t = std::time::Instant::now();
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

        // Обновляем карту верхних блоков по затронутой колонке.
        let local_y = event.cell_index % CHUNK_HEIGHT;
        let local_x = (event.cell_index / CHUNK_HEIGHT) % CHUNK_SIDE;
        let local_z = event.cell_index / (CHUNK_HEIGHT * CHUNK_SIDE);
        let col = column_index(local_x, local_z);
        // `top_y` — индекс верхнего непустого блока (`0..=CHUNK_HEIGHT - 1`).
        let top = write_height_map.0.top_y(slot, col);

        // Быстрый путь: правка не трогает верхушку колонки — карта не меняется.
        // Порядок важен: сверка с текущим `top` идёт первой, иначе замена верхнего
        // блока на другой материал молча оставила бы в карте старую высоту.
        let new_top = if top == local_y as u16 {
            // Правим ровно верхний блок — пересканируем колонку вниз.
            recompute_column(&write_world.0, slot, col)
        } else if event.cell.material_id != crate::voxel::materials::ids::AIR
            && local_y as u16 > top
        {
            // Поставили блок выше прежней верхушки.
            local_y as u16
        } else {
            top
        };

        write_height_map.0.set_top_y(slot, col, new_top);

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

    stats.record(Bucket::ApplyCellEvents, t.elapsed());
}