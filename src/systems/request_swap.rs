//! Запрос свапа на следующем кадре.

use bevy::prelude::*;

use crate::signals::{ComputePhase, ComputePipeline, SwapSignal};

/// Запрашиваем свап для следующего кадра, если обсчёт завершился
pub fn request_swap_system(
    mut signal: ResMut<SwapSignal>,
    pipeline: Res<ComputePipeline>,
) {
    // Если свап заблокирован (сохранение не завершено) — не трогаем сигнал
    if pipeline.phase == ComputePhase::AwaitingSwap {
        return;
    }

    // Если свап ещё не завершился в этом кадре — не трогаем
    if pipeline.phase == ComputePhase::AwaitingPostSwap {
        return;
    }

    // Обсчёт завершился — запрашиваем свап для следующего кадра
    signal.request();
}