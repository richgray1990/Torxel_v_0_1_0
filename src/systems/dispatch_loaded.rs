//! Отправка событий рендеру о загрузке чанков активного пула.
//!
//! Запускается после свапа, когда чанки уже в ReadWorld.
//! Отправляет событие ChunkLoaded один раз на чанк.
//! Флаг render_notified защищает от повторной отправки при каждом свапе.
use bevy::prelude::*;

use crate::manager::{ChunkManager, ReadWindowManager, SlotState};
use crate::queues::mesh_queue::{MeshUpdateEvent, MeshUpdateKind, MeshUpdateQueue, RenderSource};
use crate::voxel::format::{
    all_subchunks_mask, POOL_CHUNK_COUNT, SUBCHUNK_HEIGHT, SUBCHUNKS_Y,
};
use crate::systems::perf::{Bucket, PerfStats};

pub fn dispatch_loaded_events_system(
    mut manager: ResMut<ChunkManager>,
    mut mesh_queue: ResMut<MeshUpdateQueue>,
    read_window: Res<ReadWindowManager>,
    initial_copy: Res<crate::systems::initial_copy::InitialCopyDone>,
    stats: Res<PerfStats>,
) {
    let t = std::time::Instant::now();

    // Не отправляем события пока initial_copy не завершён.
    // Иначе меш построится из пустого ReadWorld.
    if !initial_copy.done {
        stats.record(Bucket::DispatchLoaded, t.elapsed());
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

        // Свап мог быть отложен (`has_saving_slots` → `AwaitingSwap`): пока чанк не
        // опубликован в read-снапшоте, рендер его не видит. Помечать `render_notified`
        // здесь нельзя — иначе на следующем кадре строка выше заглушит блок и
        // инвалидация соседей потеряется навсегда.
        let (chunk_nx, chunk_nz) = read_window.normalize_chunk(grid_x, grid_z);
        if read_window.slot_for_chunk(chunk_nx, chunk_nz) != Some(slot) {
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

        // ВНИМАНИЕ: инвалидация соседей отключена (2026-10-14). Край активного
        // окна теперь считается сплошным (`HALO_OPAQUE` в `fill_halo`), поэтому
        // грани на границе окна не строятся вовсе — пересобирать соседей при
        // появлении нового чанка нечего. Это снимает `ChunkDirty` соседям, то есть
        // основную долю `MeshInsert` при диагональном пролёте.
        //
        // Обратная сторона: инвалидация была односторонней — сосед, загрузившийся
        // РАНЬШЕ, пересобирался позже при появлении второго. Если край окна снова
        // станет «воздухом», эту строку надо вернуть, иначе останется закрытая грань.
        // invalidate_neighbors(&read_window, &mut mesh_queue, slot, grid_x, grid_z);
    }

    stats.record(Bucket::DispatchLoaded, t.elapsed());
}

/// Помечает грязными приграничные субчанки четырёх соседей по X/Z.
///
/// Все данные берутся только с read-стороны (`read_window`), которая опубликована
/// вместе с текущим `ReadWorld`. `ChunkManager` (write-сторона) может опережать
/// снапшот, если свап отложен, — по нему ни координаты, ни границы брать нельзя.
///
/// Временно не вызывается: край активного окна считается сплошным, пересобирать
/// соседей при появлении чанка нечего. См. комментарий на месте вызова.
#[allow(dead_code)]
fn invalidate_neighbors(
    read_window: &ReadWindowManager,
    mesh_queue: &mut MeshUpdateQueue,
    slot: usize,
    grid_x: i64,
    grid_z: i64,
) {
    // Новый чанк ещё не виден рендеру — свап отложен, инвалидация преждевременна.
    let (nx, nz) = read_window.normalize_chunk(grid_x, grid_z);
    if read_window.slot_for_chunk(nx, nz) != Some(slot) {
        return;
    }

    let mask = match content_subchunk_mask(read_window, slot) {
        Some(m) => m,
        None => return,
    };

    for (dx, dz) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
        // `grid_x`/`grid_z` — индексы чанков, а не блоков: сосед отличается на 1.
        let (cnx, cnz) = read_window.normalize_chunk(grid_x + dx, grid_z + dz);

        // В `read_window.chunk_to_slot` попадают только активные Ready-чанки.
        let n_slot = match read_window.slot_for_chunk(cnx, cnz) {
            Some(s) => s,
            None => continue,
        };

        // Сосед, чья запись ещё не создана рендером, получит её от собственного
        // `ChunkLoaded` вместе со свежим мешем — инвалидировать нечего.
        if n_slot == slot {
            continue;
        }

        mesh_queue.push(MeshUpdateEvent {
            kind: MeshUpdateKind::ChunkDirty,
            source: RenderSource::Active,
            slot_index: n_slot,
            grid_x: cnx as i64,
            grid_z: cnz as i64,
            dirty_subchunks: mask,
        });
    }
}

/// Маска субчанков, перекрывающих содержимое чанка. `None` — чанк пуст,
/// инвалидировать нечего.
fn content_subchunk_mask(read_window: &ReadWindowManager, slot: usize) -> Option<u64> {
    match read_window.content_y_range(slot) {
        // Границы недостоверны (чанк правили) — берём все субчанки.
        None => Some(all_subchunks_mask()),
        // Пустой чанк: диапазон вывернут, содержимого нет.
        Some((min_y, max_y)) if min_y > max_y => None,
        Some((min_y, max_y)) => {
            let sy_lo = (min_y as usize / SUBCHUNK_HEIGHT).min(SUBCHUNKS_Y - 1);
            let sy_hi = (max_y as usize / SUBCHUNK_HEIGHT).min(SUBCHUNKS_Y - 1);

            // Диапазон зажался в границы чанка и вывернулся — содержимого нет.
            if sy_lo > sy_hi {
                return None;
            }

            let bits = sy_hi - sy_lo + 1;
            Some(((1u64 << bits) - 1) << sy_lo)
        }
    }
}