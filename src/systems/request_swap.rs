//! Запрос свапа на следующем кадре.

use bevy::prelude::*;

use crate::manager::ChunkManager;
use crate::signals::{ComputePhase, ComputePipeline, SwapSignal};
use crate::systems::perf::{Bucket, PerfStats};
use crate::voxel::format::POOL_CHUNK_COUNT;

/// Запрашиваем свап для следующего кадра, если есть изменения
pub fn request_swap_system(
    mut signal: ResMut<SwapSignal>,
    pipeline: Res<ComputePipeline>,
    manager: Res<ChunkManager>,
    stats: Res<PerfStats>,
) {
    let t = std::time::Instant::now();

    // Если свап заблокирован (сохранение не завершено) — не трогаем сигнал
    if pipeline.phase == ComputePhase::AwaitingSwap {
        stats.record(Bucket::RequestSwap, t.elapsed());
        return;
    }

    // Если свап ещё не завершился в этом кадре — не трогаем
    if pipeline.phase == ComputePhase::AwaitingPostSwap {
        stats.record(Bucket::RequestSwap, t.elapsed());
        return;
    }

    // ИСПРАВЛЕНИЕ: Запрашиваем свап только если есть грязные чанки
    let has_dirty = (0..POOL_CHUNK_COUNT).any(|slot| {
        manager.is_pending_b_dirty(slot)
    });

    if has_dirty {
        signal.request();
    }

    stats.record(Bucket::RequestSwap, t.elapsed());
}