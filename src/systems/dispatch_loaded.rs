//! Отправка событий рендеру о загрузке чанков активного пула.
//!
//! Запускается после свапа, когда чанки уже в ReadWorld.
//! Отправляет событие ChunkLoaded один раз на чанк.
//! Флаг render_notified защищает от повторной отправки при каждом свапе.
use bevy::prelude::*;

use crate::manager::{ChunkManager, ReadWindowManager, SlotState};
use crate::queues::mesh_queue::{MeshUpdateEvent, MeshUpdateKind, MeshUpdateQueue, RenderSource};
use crate::voxel::format::{
    all_subchunks_mask, POOL_CHUNK_COUNT, SUBCHUNK_HEIGHT, SUBCHUNK_SIDE, SUBCHUNKS_Y,
};
use crate::voxel::height_map::column_index;
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

        // Инвалидируем соседей, у которых на стыке с новым чанком образуется обрыв.
        // Сосед, загрузившийся раньше, мешился с отсутствующим слотом нового чанка —
        // край считался сплошным, и его грань в сторону нового чанка была отсечена.
        // Если рельеф нового чанка ниже, эта грань обязана быть видна: обрыв есть,
        // геометрии нет. Метод — сравнение карт высот на стыке (вариант C, 2026-10-15).
        invalidate_neighbors(&read_window, &mut mesh_queue, slot, grid_x, grid_z);
    }

    stats.record(Bucket::DispatchLoaded, t.elapsed());
}

/// Помечает грязными те субчанки четырёх соседей по X/Z, на которых к стыку с
/// только что загруженным чанком образуется **обрыв**.
///
/// Зачем это нужно: сосед, загрузившийся раньше, мешился, когда слота нового чанка
/// ещё не существовало. `fill_halo` в этой ситуации считает край сплошным
/// (`HALO_OPAQUE`), поэтому грань соседа в сторону нового чанка отсекается. Если
/// рельеф нового чанка ниже, эта грань обязана быть видна — обрыв есть, геометрии
/// нет. Отсюда дыра, замеченная 2026-10-14.
///
/// Все данные берутся только с read-стороны (`read_window`), которая опубликована
/// вместе с текущим `ReadWorld`. `ChunkManager` (write-сторона) может опережать
/// снапшот, если свап отложен, — по нему ни координаты, ни границы брать нельзя.
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

        let mask = cliff_subchunk_mask(read_window, n_slot, slot, dx, dz);
        if mask == 0 {
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

/// Маска субчанков соседа `n_slot`, которыми он выше нового чанка `slot` вдоль их
/// общей грани. `0` — обрыва нет, пересобирать нечего.
///
/// Сравниваются карты высот обеих сторон по каждой из 16 колонок стыка: где сосед
/// выше нового чанка на этой колонке, там он смотрит в воздух и его грань должна
/// быть построена. Полоса такого обрыва — `[top_нового, top_соседа)`, её и
/// покрывают биты. Оценка даёт 0–2 субчанка на соседа (обычный случай — 1), тогда
/// как маска от полосы содержимого чанка давала бы 6–8.
///
/// Важно: маска считается от **соседа**, а не от нового чанка. Прежняя версия брала
/// полосу нового чанка и потому лечила стены (лишние грани), а не обрывы (пропавшие).
fn cliff_subchunk_mask(
    read_window: &ReadWindowManager,
    n_slot: usize,
    slot: usize,
    dx: i64,
    dz: i64,
) -> u64 {
    let map = &read_window.height_map;
    let mut mask = 0u64;

    for i in 0..SUBCHUNK_SIDE {
        // Локальные колонки обеих сторон вдоль общей грани. `column_index` —
        // `local_z * CHUNK_SIDE + local_x`.
        let (n_col, s_col) = match (dx, dz) {
            // Сосед западнее: его восточная кромка (x = 15) против западной нового (x = 0).
            (-1, 0) => (column_index(SUBCHUNK_SIDE - 1, i), column_index(0, i)),
            // Сосед восточнее: его западная кромка против восточной нового.
            (1, 0) => (column_index(0, i), column_index(SUBCHUNK_SIDE - 1, i)),
            // Сосед южнее: кромки по `z`.
            (0, -1) => (column_index(i, SUBCHUNK_SIDE - 1), column_index(i, 0)),
            // Сосед севернее.
            (0, 1) => (column_index(i, 0), column_index(i, SUBCHUNK_SIDE - 1)),
            _ => continue,
        };

        let n_top = map.top_y(n_slot, n_col) as usize;
        let s_top = map.top_y(slot, s_col) as usize;

        // Сосед не выше нового чанка на этой колонке — обрыва нет.
        if s_top >= n_top {
            continue;
        }

        // `top_y` — первая воздушная ячейка, поэтому обрыв это `[s_top, n_top)`.
        let sy_lo = (s_top / SUBCHUNK_HEIGHT).min(SUBCHUNKS_Y - 1);
        let sy_hi = ((n_top - 1) / SUBCHUNK_HEIGHT).min(SUBCHUNKS_Y - 1);

        if sy_lo > sy_hi {
            continue;
        }

        mask |= ((1u64 << (sy_hi - sy_lo + 1)) - 1) << sy_lo;
    }

    mask
}