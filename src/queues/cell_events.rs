//! Очередь событий изменений ячеек (границы чанков, физика).
use bevy::prelude::*;
use smallvec::SmallVec;

use crate::voxel::format::Cell;

/// Тип события ячейки
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellEventKind {
    /// Ячейка добавлена / заменена
    SetCell,
    /// Жидкость перетекла
    LiquidTransfer,
    /// Газ перетёк
    GasTransfer,
    /// Фазовый переход
    PhaseTransition,
}

/// Событие изменения ячейки
#[derive(Clone, Copy, Debug)]
pub struct CellEvent {
    pub kind: CellEventKind,
    pub slot_index: usize,
    pub cell_index: usize,
    pub cell: Cell,
    /// Если `Some`, событие применяется только когда текущий `generation` чанка совпадает.
    ///
    /// `None` — доверенное событие:
    /// - начальный тест;
    /// - системное редактирование без снапшота;
    /// - загрузчик, если он когда-нибудь будет писать через события.
    ///
    /// `Some(gen)` — событие рассчитывалось по снапшоту с поколением `gen`.
    pub expected_generation: Option<u64>,
}

impl CellEvent {
    /// Удобный конструктор для доверенного SetCell без проверки поколения.
    #[inline]
    pub fn set_cell(slot_index: usize, cell_index: usize, cell: Cell) -> Self {
        Self {
            kind: CellEventKind::SetCell,
            slot_index,
            cell_index,
            cell,
            expected_generation: None,
        }
    }

    /// Удобный конструктор для SetCell с проверкой поколения.
    #[inline]
    pub fn set_cell_expected(
        slot_index: usize,
        cell_index: usize,
        cell: Cell,
        expected_generation: u64,
    ) -> Self {
        Self {
            kind: CellEventKind::SetCell,
            slot_index,
            cell_index,
            cell,
            expected_generation: Some(expected_generation),
        }
    }
}

/// Очередь событий ячеек
#[derive(Resource, Default)]
pub struct CellEventQueue {
    pub events: SmallVec<[CellEvent; 256]>,
}

impl CellEventQueue {
    pub fn push(&mut self, event: CellEvent) {
        self.events.push(event);
    }

    pub fn drain(&mut self) -> impl Iterator<Item = CellEvent> + '_ {
        self.events.drain(..)
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}