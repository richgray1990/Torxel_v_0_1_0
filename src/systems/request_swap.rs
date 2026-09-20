//! Запрос свапа на следующем кадре.

use bevy::prelude::*;

use crate::manager::ChunkManager;
use crate::signals::{ComputePhase, ComputePipeline, SwapSignal};
use crate::voxel::format::POOL_CHUNK_COUNT;

/// Запрашиваем свап для следующего кадра, если есть изменения
pub fn request_swap_system(
    mut signal: ResMut<SwapSignal>,
    pipeline: Res<ComputePipeline>,
    manager: Res<ChunkManager>,
) {
    // Если свап заблокирован (сохранение не завершено) — не трогаем сигнал
    if pipeline.phase == ComputePhase::AwaitingSwap {
        return;
    }

    // Если свап ещё не завершился в этом кадре — не трогаем
    if pipeline.phase == ComputePhase::AwaitingPostSwap {
        return;
    }

    // ИСПРАВЛЕНИЕ: Запрашиваем свап только если есть грязные чанки
    let has_dirty = (0..POOL_CHUNK_COUNT).any(|slot| {
        manager.is_pending_b_dirty(slot)
    });

    if has_dirty {
        signal.request();
    }
}