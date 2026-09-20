//! Сигналы и фазы конвейера.

use bevy::prelude::*;

/// Фаза конвейера обсчёта
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ComputePhase {
    /// Свап заблокирован (сохранение не завершено)
    AwaitingSwap,
    /// Свап выполнен, грязные чанки ещё не скопированы
    AwaitingPostSwap,
    /// Всё готово, обсчёт может работать
    Computing,
}

impl ComputePhase {
    #[inline(always)]
    pub fn is_computing(self) -> bool {
        matches!(self, Self::Computing)
    }

    #[inline(always)]
    pub fn is_awaiting_swap(self) -> bool {
        matches!(self, Self::AwaitingSwap)
    }

    #[inline(always)]
    pub fn is_awaiting_post_swap(self) -> bool {
        matches!(self, Self::AwaitingPostSwap)
    }
}

/// Семафор конвейера
#[derive(Resource)]
pub struct ComputePipeline {
    pub phase: ComputePhase,
}

impl Default for ComputePipeline {
    fn default() -> Self {
        Self {
            phase: ComputePhase::Computing,
        }
    }
}

/// Сигнал запроса свапа
#[derive(Resource, Default)]
pub struct SwapSignal {
    pub requested: bool,
}

impl SwapSignal {
    #[inline(always)]
    pub fn request(&mut self) {
        self.requested = true;
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.requested = false;
    }
}