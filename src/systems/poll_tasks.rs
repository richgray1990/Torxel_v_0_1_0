//! Опрос фоновых задач активного пула.
use bevy::prelude::*;

use crate::io::channels::IoManager;
use crate::io::staging::ActiveStagingBuffer;
use crate::voxel::pool::WriteWorld;
use crate::voxel::height_map::WriteHeightMap;
use crate::manager::{ChunkManager, SlotState};
use crate::systems::perf::{Bucket, PerfStats, FrameFlowStart};

/// Система опроса фоновых задач активного пула.
pub fn poll_background_tasks_system(
    io_manager: Res<IoManager>,
    mut staging: ResMut<ActiveStagingBuffer>,
    mut write_world: ResMut<WriteWorld>,
    mut write_height_map: ResMut<WriteHeightMap>,
    mut manager: ResMut<ChunkManager>,
    stats: Res<PerfStats>,
    mut flow_start: ResMut<FrameFlowStart>,
) {
    // Первая система цепочки `GameFlowSet`. Здесь же закрывается `FrameTail` —
    // время от конца цепочки ПРОШЛОГО кадра до её начала в этом: `PostUpdate`,
    // `Last`, extract, prepare, present. Ни один другой бакет туда не достаёт.
    //
    // `GameFlowWall + FrameTail` должны в сумме дать `Frame`. Если `Frame` больше —
    // остаток уходит на планировщик между системами внутри самой цепочки.
    let now = std::time::Instant::now();

    if let Some(prev_end) = flow_start.end.take() {
        stats.record(Bucket::FrameTail, now.duration_since(prev_end));
    }

    flow_start.start = Some(now);

    let t = std::time::Instant::now();
    // Забираем ответы на загрузку из канала.
    while let Some(response) = io_manager.try_recv_load_response() {
        let slot = response.slot;

        // Слот мог быть переиспользован под другой чанк, пока ответ шёл через канал.
        // Принимаем ответ только если слот всё ещё грузит тот же самый чанк.
        if !slot_still_loading(&manager, slot, response.chunk_x, response.chunk_z) {
            continue;
        }

        if response.success {
            let meta = &manager.metadata[slot];
            staging.push_loaded(crate::io::staging::StagedChunk {
                slot,
                grid_x: meta.grid_x,
                grid_z: meta.grid_z,
                data: response.data,
                min_solid_y: response.min_solid_y,
                max_solid_y: response.max_solid_y,
                min_liquid_y: response.min_liquid_y,
                max_liquid_y: response.max_liquid_y,
                column_tops: response.column_tops,
            });
        } else {
            manager.set_slot_state(slot, SlotState::Failed);
        }
    }

    // Забираем ответы на сохранение из канала.
    while let Some(response) = io_manager.try_recv_save_response() {
        staging.push_saved(response);
    }

    // Применяем загруженные чанки к WriteWorld.
    for staged_chunk in staging.take_loaded() {
        let slot = staged_chunk.slot;

        write_world
            .0
            .chunk_slice_mut(slot)
            .copy_from_slice(&staged_chunk.data);

        write_height_map
            .0
            .column_slice_mut(slot)
            .copy_from_slice(&staged_chunk.column_tops);

        manager.metadata[slot].min_solid_y = staged_chunk.min_solid_y;
        manager.metadata[slot].max_solid_y = staged_chunk.max_solid_y;
        manager.metadata[slot].min_liquid_y = staged_chunk.min_liquid_y;
        manager.metadata[slot].max_liquid_y = staged_chunk.max_liquid_y;

        manager.set_slot_state(slot, SlotState::Ready);
        manager.metadata[slot].generation = 0;
        manager.metadata[slot].render_notified = false;

        // Если чанк активен, нужен swap, чтобы он попал в ReadWorld.
        if manager.metadata[slot].is_active {
            manager.set_pending_b_dirty(slot, true);
        }
    }

    // Применяем результаты сохранения.
    for save_response in staging.take_saved() {
        let slot = save_response.slot;

        if save_response.success {
            manager.set_slot_state(slot, SlotState::Ready);
            manager.set_file_dirty(slot, false);

            // Если чанк активен, публикуем его готовность через swap.
            if manager.metadata[slot].is_active {
                manager.metadata[slot].render_notified = false;
                manager.set_pending_b_dirty(slot, true);
            }
        } else {
            manager.set_slot_state(slot, SlotState::Failed);
        }
    }

    stats.record(Bucket::PollTasks, t.elapsed());
}

/// Слот всё ещё грузит тот же чанк, который запрашивал ответ.
#[inline(always)]
fn slot_still_loading(
    manager: &ChunkManager,
    slot: usize,
    chunk_x: usize,
    chunk_z: usize,
) -> bool {
    let meta = &manager.metadata[slot];

    meta.state == SlotState::Loading
        && meta.grid_x == chunk_x as i64
        && meta.grid_z == chunk_z as i64
}