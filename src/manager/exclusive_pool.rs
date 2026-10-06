//! Система аренды изолированных пулов для произвольных подсистем.
//!
//! Без Arc, без Atomic, без unsafe.
//!
//! Модель:
//! - lease копирует нужные чанки из ReadWorld в локальный буфер;
//! - lease запоминает base generation каждого чанка из ReadWindowManager;
//! - ReadWrite lease может локально менять ячейки;
//! - submit делает pre-check по ReadWindowManager;
//! - submit создаёт CellEvent с expected_generation;
//! - Aggregate делает финальный check по write-side generation.

use bevy::prelude::*;

use crate::manager::ReadWindowManager;
use crate::queues::cell_events::{CellEvent, CellEventQueue};
use crate::voxel::format::{Cell, CELLS_PER_CHUNK};
use crate::voxel::pool::ReadWorld;

/// Идентификатор аренды.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LeaseId(pub u64);

/// Режим аренды.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseMode {
    /// Только чтение. Submit не требуется, достаточно release.
    ReadOnly,

    /// Чтение + локальная запись.
    /// Изменения возвращаются через submit_read_write в CellEventQueue.
    ReadWrite,
}

/// Владелец аренды. Нужен для диагностики и будущих приоритетов.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseOwner {
    Debug,
    Physics,
    Hydro,
    Fire,
    Custom(u32),
}

/// Ошибки аренды.
#[derive(Debug)]
pub enum LeaseError {
    /// Достигнуто максимальное количество активных leases.
    NoCapacity,

    /// Запрошено больше слотов, чем разрешено для одного lease.
    TooManySlots {
        requested: usize,
        max: usize,
    },

    /// Lease не найден. Возможно уже выпущен или отправлен.
    InvalidLease,

    /// Неверный режим lease для операции.
    WrongMode {
        expected: LeaseMode,
        actual: LeaseMode,
    },

    /// Запрошенный слот не находится в состоянии Ready в read-снапшоте.
    SlotNotReady {
        slot: usize,
    },

    /// Слот не входит в lease.
    SlotNotInLease {
        slot: usize,
    },

    /// Индекс ячейки вне диапазона чанка.
    CellOutOfRange {
        cell_index: usize,
    },

    /// Generation чанка изменился с момента выдачи lease.
    GenConflict {
        slot: usize,
        expected: u64,
        actual: u64,
    },
}

/// Компактный буфер аренды.
///
/// Хранит только запрошенные чанки, а не весь активный пул.
/// `global_slots` отсортирован и уникален.
#[derive(Debug)]
pub struct LeaseBuffer {
    /// Глобальные индексы слотов активного окна, отсортированные.
    pub global_slots: Vec<usize>,

    /// Данные чанков. Длина = global_slots.len() * CELLS_PER_CHUNK.
    pub data: Vec<Cell>,
}

impl LeaseBuffer {
    pub fn new() -> Self {
        Self {
            global_slots: Vec::new(),
            data: Vec::new(),
        }
    }

    /// Создать буфер, скопировав указанные слоты из ReadWorld.
    ///
    /// Входные слоты сортируются и дедуплицируются.
    pub fn from_slots(read_world: &ReadWorld, slots: &[usize]) -> Self {
        let mut global_slots = slots.to_vec();
        global_slots.sort_unstable();
        global_slots.dedup();

        let mut data = Vec::with_capacity(global_slots.len() * CELLS_PER_CHUNK);

        for &slot in &global_slots {
            data.extend_from_slice(read_world.0.chunk_slice(slot));
        }

        Self { global_slots, data }
    }

    /// Локальный индекс слота в буфере.
    #[inline]
    pub fn local_index(&self, global_slot: usize) -> Option<usize> {
        self.global_slots.binary_search(&global_slot).ok()
    }

    /// Чтение ячейки по глобальному слоту и локальному индексу ячейки внутри чанка.
    #[inline]
    pub fn get(&self, global_slot: usize, cell_index: usize) -> Option<&Cell> {
        let local = self.local_index(global_slot)?;

        if cell_index >= CELLS_PER_CHUNK {
            return None;
        }

        Some(&self.data[local * CELLS_PER_CHUNK + cell_index])
    }

    /// Запись ячейки по глобальному слоту и локальному индексу ячейки внутри чанка.
    #[inline]
    pub fn get_mut(&mut self, global_slot: usize, cell_index: usize) -> Option<&mut Cell> {
        let local = self.local_index(global_slot)?;

        if cell_index >= CELLS_PER_CHUNK {
            return None;
        }

        Some(&mut self.data[local * CELLS_PER_CHUNK + cell_index])
    }

    #[inline]
    pub fn slot_count(&self) -> usize {
        self.global_slots.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.global_slots.is_empty()
    }
}

impl Default for LeaseBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Состояние активной аренды.
#[derive(Debug)]
pub struct LeaseState {
    pub id: LeaseId,
    pub mode: LeaseMode,
    pub buffer: LeaseBuffer,

    /// Base generation для каждого слота из buffer.global_slots.
    ///
    /// Индексы совпадают: base_generations[i] соответствует global_slots[i].
    pub base_generations: Vec<u64>,

    /// Изменённые ячейки: (global_slot, cell_index).
    ///
    /// Для первого этапа дубликаты допустимы.
    /// Aggregate всё равно применит generation++ один раз на чанк.
    pub dirty_cells: Vec<(usize, usize)>,

    pub owner: LeaseOwner,
}

/// Реестр исключительных пулов.
#[derive(Resource, Debug)]
pub struct ExclusivePoolRegistry {
    leases: Vec<LeaseState>,
    next_id: u64,
    max_leases: usize,
    max_slots_per_lease: usize,
}

impl ExclusivePoolRegistry {
    pub fn new(max_leases: usize, max_slots_per_lease: usize) -> Self {
        Self {
            leases: Vec::new(),
            next_id: 0,
            max_leases,
            max_slots_per_lease,
        }
    }

    #[inline]
    pub fn active_lease_count(&self) -> usize {
        self.leases.len()
    }

    fn find_index(&self, id: LeaseId) -> Option<usize> {
        self.leases.iter().position(|lease| lease.id == id)
    }

    /// Взять ReadOnly lease.
    pub fn lease_read_only(
        &mut self,
        read_world: &ReadWorld,
        read_window: &ReadWindowManager,
        slots: &[usize],
        owner: LeaseOwner,
    ) -> Result<LeaseId, LeaseError> {
        self.lease_impl(
            read_world,
            read_window,
            slots,
            owner,
            LeaseMode::ReadOnly,
        )
    }

    /// Взять ReadWrite lease.
    pub fn lease_read_write(
        &mut self,
        read_world: &ReadWorld,
        read_window: &ReadWindowManager,
        slots: &[usize],
        owner: LeaseOwner,
    ) -> Result<LeaseId, LeaseError> {
        self.lease_impl(
            read_world,
            read_window,
            slots,
            owner,
            LeaseMode::ReadWrite,
        )
    }

    fn lease_impl(
        &mut self,
        read_world: &ReadWorld,
        read_window: &ReadWindowManager,
        slots: &[usize],
        owner: LeaseOwner,
        mode: LeaseMode,
    ) -> Result<LeaseId, LeaseError> {
        if self.leases.len() >= self.max_leases {
            return Err(LeaseError::NoCapacity);
        }

        if slots.len() > self.max_slots_per_lease {
            return Err(LeaseError::TooManySlots {
                requested: slots.len(),
                max: self.max_slots_per_lease,
            });
        }

        let buffer = LeaseBuffer::from_slots(read_world, slots);

        // Все слоты должны быть Ready в read-снапшоте.
        for &slot in &buffer.global_slots {
            if !read_window.is_ready(slot) {
                return Err(LeaseError::SlotNotReady { slot });
            }
        }

        let base_generations: Vec<u64> = buffer
            .global_slots
            .iter()
            .map(|&slot| read_window.generation(slot))
            .collect();

        let id = LeaseId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);

        self.leases.push(LeaseState {
            id,
            mode,
            buffer,
            base_generations,
            dirty_cells: Vec::new(),
            owner,
        });

        Ok(id)
    }

    /// Получить immutable доступ к буферу.
    #[inline]
    pub fn buffer(&self, id: LeaseId) -> Option<&LeaseBuffer> {
        let idx = self.find_index(id)?;
        Some(&self.leases[idx].buffer)
    }

    /// Получить mutable доступ к буферу.
    ///
    /// Важно: после прямой записи через buffer_mut нужно отдельно вызвать mark_dirty.
    /// Для удобства есть set_cell(), который делает запись и mark_dirty вместе.
    #[inline]
    pub fn buffer_mut(&mut self, id: LeaseId) -> Option<&mut LeaseBuffer> {
        let idx = self.find_index(id)?;
        Some(&mut self.leases[idx].buffer)
    }

    /// Пометить ячейку изменённой в ReadWrite lease.
    pub fn mark_dirty(
        &mut self,
        id: LeaseId,
        global_slot: usize,
        cell_index: usize,
    ) -> Result<(), LeaseError> {
        let idx = self.find_index(id).ok_or(LeaseError::InvalidLease)?;

        if self.leases[idx].mode != LeaseMode::ReadWrite {
            return Err(LeaseError::WrongMode {
                expected: LeaseMode::ReadWrite,
                actual: self.leases[idx].mode,
            });
        }

        if self.leases[idx].buffer.local_index(global_slot).is_none() {
            return Err(LeaseError::SlotNotInLease { slot: global_slot });
        }

        if cell_index >= CELLS_PER_CHUNK {
            return Err(LeaseError::CellOutOfRange { cell_index });
        }

        self.leases[idx].dirty_cells.push((global_slot, cell_index));
        Ok(())
    }

    /// Удобный метод: записать ячейку в ReadWrite lease и пометить её грязной.
    pub fn set_cell(
        &mut self,
        id: LeaseId,
        global_slot: usize,
        cell_index: usize,
        cell: Cell,
    ) -> Result<(), LeaseError> {
        let idx = self.find_index(id).ok_or(LeaseError::InvalidLease)?;

        if self.leases[idx].mode != LeaseMode::ReadWrite {
            return Err(LeaseError::WrongMode {
                expected: LeaseMode::ReadWrite,
                actual: self.leases[idx].mode,
            });
        }

        if self.leases[idx].buffer.local_index(global_slot).is_none() {
            return Err(LeaseError::SlotNotInLease { slot: global_slot });
        }

        if cell_index >= CELLS_PER_CHUNK {
            return Err(LeaseError::CellOutOfRange { cell_index });
        }

        if let Some(target) = self.leases[idx].buffer.get_mut(global_slot, cell_index) {
            *target = cell;
        } else {
            return Err(LeaseError::CellOutOfRange { cell_index });
        }

        self.leases[idx].dirty_cells.push((global_slot, cell_index));
        Ok(())
    }

    /// Выпустить lease без коммита изменений.
    ///
    /// Подходит для ReadOnly и для отмены ReadWrite.
    pub fn release(&mut self, id: LeaseId) -> Result<(), LeaseError> {
        let idx = self.find_index(id).ok_or(LeaseError::InvalidLease)?;
        self.leases.remove(idx);
        Ok(())
    }

    /// Отправить ReadWrite lease в очередь событий.
    ///
    /// Делает быстрый pre-check по ReadWindowManager.
    /// Если pre-check пройден, создаёт CellEvent с expected_generation
    /// и кладёт их в CellEventQueue.
    ///
    /// Финальную проверку generation делает Aggregate.
    pub fn submit_read_write(
        &mut self,
        id: LeaseId,
        read_window: &ReadWindowManager,
        queue: &mut CellEventQueue,
    ) -> Result<(), LeaseError> {
        let idx = self.find_index(id).ok_or(LeaseError::InvalidLease)?;

        if self.leases[idx].mode != LeaseMode::ReadWrite {
            let actual = self.leases[idx].mode;
            self.leases.remove(idx);
            return Err(LeaseError::WrongMode {
                expected: LeaseMode::ReadWrite,
                actual,
            });
        }

        // Забираем lease из реестра.
        // Дальше работаем с локальным значением, чтобы не держать borrow self.
        let lease = self.leases.remove(idx);

        // Pre-check по read-снапшоту.
        //
        // Это best-effort защита от заведомо устаревших результатов.
        // Финальную гарантию даёт Aggregate.
        for (i, &slot) in lease.buffer.global_slots.iter().enumerate() {
            let expected = lease.base_generations[i];
            let actual = read_window.generation(slot);

            if actual != expected {
                return Err(LeaseError::GenConflict {
                    slot,
                    expected,
                    actual,
                });
            }
        }

        // Собираем события.
        let mut events = Vec::with_capacity(lease.dirty_cells.len());

        for &(slot, cell_index) in &lease.dirty_cells {
            let local = lease
                .buffer
                .local_index(slot)
                .ok_or(LeaseError::SlotNotInLease { slot })?;

            let expected_generation = lease.base_generations[local];

            let cell = *lease
                .buffer
                .get(slot, cell_index)
                .ok_or(LeaseError::CellOutOfRange { cell_index })?;

            events.push(CellEvent::set_cell_expected(
                slot,
                cell_index,
                cell,
                expected_generation,
            ));
        }

        // Отправляем в очередь Aggregate.
        for event in events {
            queue.push(event);
        }

        Ok(())
    }
}