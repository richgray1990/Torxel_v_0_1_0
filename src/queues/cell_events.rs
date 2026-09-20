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