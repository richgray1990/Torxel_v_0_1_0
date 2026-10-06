//! Системы свапа пулов.

use bevy::prelude::*;

use crate::voxel::format::POOL_CHUNK_COUNT;
use crate::voxel::pool::{ReadWorld, WriteWorld};
use crate::manager::masks::DirtyMask;
use crate::manager::{ChunkManager, ReadWindowManager};
use crate::queues::mesh_queue::{MeshUpdateEvent, MeshUpdateKind, RenderSource};
use crate::queues::PostSwapDirtyBuffer;
use crate::signals::{ComputePhase, ComputePipeline, SwapSignal};

/// Фаза 1: Мгновенный обмен указателей пулов.
///
/// После этой системы рендер может читать новый ReadWorld.
/// Держит ResMut обоих пулов на ~2 наносекунды.
pub fn swap_pointers_system(
    mut read_world: ResMut<ReadWorld>,
    mut write_world: ResMut<WriteWorld>,
    mut signal: ResMut<SwapSignal>,
    mut pipeline: ResMut<ComputePipeline>,
    manager: Res<ChunkManager>,
    mut read_window: ResMut<ReadWindowManager>,
) {
    if !signal.requested {
        return;
    }

    // Если есть незавершённые сохранения — откладываем свап
    if manager.has_saving_slots() {
        pipeline.phase = ComputePhase::AwaitingSwap;
        return;
    }

    // Обмен указателей пулов
    std::mem::swap(&mut read_world.0, &mut write_world.0);

    // Публикуем read-снапшот метаданных
    read_window.publish_from(&manager);

    // Свап состоялся, рендер уже может читать новые данные
    signal.requested = false;
    pipeline.phase = ComputePhase::AwaitingPostSwap;
}

/// Фаза 2: Копирование грязных чанков из нового ReadWorld в новый WriteWorld.
///
/// Подготовка WriteWorld к следующему кадру обсчёта.
/// Работает параллельно с рендером:
/// - Рендер читает Res<ReadWorld> + Res<ReadWindowManager>
/// - Мы читаем Res<ReadWorld> + пишем ResMut<WriteWorld> + ResMut<ChunkManager>
///
/// Конфликта ресурсов нет — ветки параллельны.
pub fn post_swap_copy_system(
    read_world: Res<ReadWorld>,
    mut write_world: ResMut<WriteWorld>,
    mut manager: ResMut<ChunkManager>,
    mut dirty_buffer: ResMut<PostSwapDirtyBuffer>,
    mut pipeline: ResMut<ComputePipeline>,
) {
    // Работаем только если свап состоялся
    if pipeline.phase != ComputePhase::AwaitingPostSwap {
        return;
    }

    // Собираем грязные слоты
    let mut dirty_mask = DirtyMask::new();

    for i in 0..POOL_CHUNK_COUNT {
        if manager.is_pending_b_dirty(i) {
            dirty_mask.set(i);
            manager.set_file_dirty(i, true);
        }
    }

    // Копируем грязные чанки и складываем события в буфер
    for i in dirty_mask.iter_set() {
        write_world.0.copy_chunk_from(&read_world.0, i);

        // Забираем маску грязных субчанков и сбрасываем
        let sub_mask = manager.metadata[i].take_dirty_subchunks();

        dirty_buffer.push(MeshUpdateEvent {
            kind: MeshUpdateKind::ChunkDirty,
            slot_index: i,
            grid_x: manager.metadata[i].grid_x,
            grid_z: manager.metadata[i].grid_z,
            source: RenderSource::Active,
            dirty_subchunks: sub_mask,
        });

        manager.set_pending_b_dirty(i, false);
    }

    // Готово — обсчёт может работать
    pipeline.phase = ComputePhase::Computing;
}