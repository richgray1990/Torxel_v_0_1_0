//! Теневой пул чанков для дальних запросов систем обсчёта.
//!
//! Только чтение. Без двойного буфера. Динамический маппинг слотов.

use std::collections::HashMap;

use bevy::prelude::*;

use super::format::{Cell, CELLS_PER_CHUNK, CHUNK_HEIGHT, CHUNK_SIDE};
use super::topology::TorusTopology;
use bytemuck::Zeroable;

/// Размер теневого пула (чанков по стороне)
pub const SHADOW_POOL_SIDE: usize = 6;

/// Всего слотов в теневом пуле
pub const SHADOW_SLOT_COUNT: usize = SHADOW_POOL_SIDE * SHADOW_POOL_SIDE;

/// Задержка выгрузки теневого чанка в кадрах
pub const SHADOW_UNLOAD_DELAY: u64 = 300;

/// Состояние слота теневого пула
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShadowSlotState {
    Empty,
    Loading,
    Ready,
}

/// Результат запроса теневого чанка
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShadowRequestResult {
    /// Чанк готов, слот указан
    Ready(usize),
    /// Чанк поставлен в очередь загрузки, слот указан
    Queued(usize),
    /// Чанк находится в активном окне, теневой пул не нужен
    InActiveWindow,
    /// Нет свободных слотов и некого выгрузить
    NotAvailable,
}

/// Метаданные одного слота теневого пула
pub struct ShadowSlotMeta {
    pub state: ShadowSlotState,
    pub chunk_x: i64,
    pub chunk_z: i64,
    pub last_access: u64,
    pub ref_count: u32,
}

impl ShadowSlotMeta {
    pub fn new() -> Self {
        Self {
            state: ShadowSlotState::Empty,
            chunk_x: 0,
            chunk_z: 0,
            last_access: 0,
            ref_count: 0,
        }
    }
}

/// Теневой пул чанков
#[derive(Resource)]
pub struct ShadowPool {
    /// Плоский массив ячеек для всех слотов
    pub data: Box<[Cell]>,
    /// Метаданные слотов
    pub meta: Vec<ShadowSlotMeta>,
    /// Таблица маппинга: координаты чанка → индекс слота
    pub slot_map: HashMap<(i64, i64), usize>,
    /// Список свободных слотов
    pub free_slots: Vec<usize>,
    /// Топология для нормализации координат
    pub topology: TorusTopology,
    /// Глобальный счётчик кадров (для LRU)
    pub frame_counter: u64,
}

impl ShadowPool {
    pub fn new(topology: TorusTopology) -> Self {
        let total = SHADOW_SLOT_COUNT * CELLS_PER_CHUNK;
        let mut vec = Vec::with_capacity(total);
        vec.resize(total, Cell::zeroed());

        let meta = (0..SHADOW_SLOT_COUNT).map(|_| ShadowSlotMeta::new()).collect();
        let free_slots = (0..SHADOW_SLOT_COUNT).rev().collect();

        Self {
            data: vec.into_boxed_slice(),
            meta,
            slot_map: HashMap::new(),
            free_slots,
            topology,
            frame_counter: 0,
        }
    }

    /// Увеличивает счётчик кадров (вызывается каждый кадр)
    #[inline]
    pub fn tick_frame(&mut self) {
        self.frame_counter += 1;
    }

    /// Нормализует координаты чанка через топологию
    #[inline]
    fn normalize_chunk(&self, chunk_x: i64, chunk_z: i64) -> (i64, i64) {
        let (cx, cz) = self.topology.normalize_chunk(chunk_x, chunk_z);
        (cx as i64, cz as i64)
    }

    /// Чтение ячейки по мировым координатам
    pub fn read_cell(&self, x: i64, y: i64, z: i64) -> Option<&Cell> {
        if y < 0 || y >= CHUNK_HEIGHT as i64 {
            return None;
        }

        let chunk_x = x.div_euclid(CHUNK_SIDE as i64);
        let chunk_z = z.div_euclid(CHUNK_SIDE as i64);

        let local_x = x.rem_euclid(CHUNK_SIDE as i64) as usize;
        let local_y = y as usize;
        let local_z = z.rem_euclid(CHUNK_SIDE as i64) as usize;

        let (norm_x, norm_z) = self.normalize_chunk(chunk_x, chunk_z);

        let slot = self.slot_map.get(&(norm_x, norm_z))?;

        if self.meta[*slot].state != ShadowSlotState::Ready {
            return None;
        }

        let cell_idx = (local_z * CHUNK_SIDE + local_x) * CHUNK_HEIGHT + local_y;
        Some(self.get(*slot, cell_idx))
    }

    /// Чтение чанка целиком
    pub fn read_chunk(&self, chunk_x: i64, chunk_z: i64) -> Option<&[Cell]> {
        let (norm_x, norm_z) = self.normalize_chunk(chunk_x, chunk_z);
        let slot = self.slot_map.get(&(norm_x, norm_z))?;

        if self.meta[*slot].state != ShadowSlotState::Ready {
            return None;
        }

        Some(self.chunk_slice(*slot))
    }

    /// Запрос теневого чанка
    pub fn request_chunk(&mut self, chunk_x: i64, chunk_z: i64) -> ShadowRequestResult {
        let (norm_x, norm_z) = self.normalize_chunk(chunk_x, chunk_z);

        // Проверяем, есть ли уже в пуле
        if let Some(&slot) = self.slot_map.get(&(norm_x, norm_z)) {
            self.meta[slot].last_access = self.frame_counter;
            self.meta[slot].ref_count += 1;
            return match self.meta[slot].state {
                ShadowSlotState::Ready => ShadowRequestResult::Ready(slot),
                ShadowSlotState::Loading => ShadowRequestResult::Queued(slot),
                ShadowSlotState::Empty => ShadowRequestResult::NotAvailable,
            };
        }

        // Ищем свободный слот
        let slot = if let Some(s) = self.free_slots.pop() {
            s
        } else {
            // Пул заполнен — выгружаем LRU
            match self.find_lru() {
                Some(lru_slot) => {
                    self.evict_slot(lru_slot);
                    lru_slot
                }
                None => return ShadowRequestResult::NotAvailable,
            }
        };

        // Выделяем слот под новый чанк
        self.meta[slot].state = ShadowSlotState::Loading;
        self.meta[slot].chunk_x = norm_x;
        self.meta[slot].chunk_z = norm_z;
        self.meta[slot].last_access = self.frame_counter;
        self.meta[slot].ref_count = 1;
        self.slot_map.insert((norm_x, norm_z), slot);

        ShadowRequestResult::Queued(slot)
    }

    /// Освобождение теневого чанка
    pub fn release_chunk(&mut self, chunk_x: i64, chunk_z: i64) {
        let (norm_x, norm_z) = self.normalize_chunk(chunk_x, chunk_z);

        if let Some(&slot) = self.slot_map.get(&(norm_x, norm_z)) {
            if self.meta[slot].ref_count > 0 {
                self.meta[slot].ref_count -= 1;
            }
        }
    }

    /// Применение загруженных данных к слоту
    pub fn apply_loaded_chunk(&mut self, slot: usize, data: &[Cell]) {
        debug_assert!(slot < SHADOW_SLOT_COUNT, "shadow slot out of range");
        let start = slot * CELLS_PER_CHUNK;
        let end = start + CELLS_PER_CHUNK;
        self.data[start..end].copy_from_slice(data);
        self.meta[slot].state = ShadowSlotState::Ready;
    }

    /// Помечает слот как ошибочный
    pub fn mark_failed(&mut self, slot: usize) {
        self.meta[slot].state = ShadowSlotState::Empty;
        let key = (self.meta[slot].chunk_x, self.meta[slot].chunk_z);
        self.slot_map.remove(&key);
        self.meta[slot].chunk_x = 0;
        self.meta[slot].chunk_z = 0;
        self.meta[slot].ref_count = 0;
        self.free_slots.push(slot);
    }

    /// Проверка готовности чанка
    pub fn is_ready(&self, chunk_x: i64, chunk_z: i64) -> bool {
        let (norm_x, norm_z) = self.normalize_chunk(chunk_x, chunk_z);
        match self.slot_map.get(&(norm_x, norm_z)) {
            Some(&slot) => self.meta[slot].state == ShadowSlotState::Ready,
            None => false,
        }
    }

    /// Поиск наименее используемого слота для выгрузки
    fn find_lru(&self) -> Option<usize> {
        let mut best_slot: Option<usize> = None;
        let mut best_access = u64::MAX;
        let mut best_refs = u32::MAX;

        for slot in 0..SHADOW_SLOT_COUNT {
            let meta = &self.meta[slot];
            if meta.state == ShadowSlotState::Empty {
                continue;
            }
            if meta.ref_count > 0 {
                continue;
            }
            if meta.last_access < best_access
                || (meta.last_access == best_access && meta.ref_count < best_refs)
            {
                best_access = meta.last_access;
                best_refs = meta.ref_count;
                best_slot = Some(slot);
            }
        }

        best_slot
    }

    /// Выгрузка слота
    fn evict_slot(&mut self, slot: usize) {
        let key = (self.meta[slot].chunk_x, self.meta[slot].chunk_z);
        self.slot_map.remove(&key);
        self.meta[slot].state = ShadowSlotState::Empty;
        self.meta[slot].chunk_x = 0;
        self.meta[slot].chunk_z = 0;
        self.meta[slot].ref_count = 0;
        self.free_slots.push(slot);

        // Очищаем данные
        let start = slot * CELLS_PER_CHUNK;
        let end = start + CELLS_PER_CHUNK;
        self.data[start..end].fill(Cell::zeroed());
    }

    /// Выгрузка по таймеру (вызывается из shadow_gc)
    pub fn tick_unload(&mut self) {
        let current_frame = self.frame_counter;
        let mut to_evict = Vec::new();

        for slot in 0..SHADOW_SLOT_COUNT {
            let meta = &self.meta[slot];
            if meta.state != ShadowSlotState::Ready {
                continue;
            }
            if meta.ref_count > 0 {
                continue;
            }
            if current_frame - meta.last_access > SHADOW_UNLOAD_DELAY {
                to_evict.push(slot);
            }
        }

        for slot in to_evict {
            self.evict_slot(slot);
        }
    }

    /// Доступ к ячейке по слоту и индексу
    #[inline]
    pub fn get(&self, slot: usize, cell_index: usize) -> &Cell {
        debug_assert!(slot < SHADOW_SLOT_COUNT, "shadow slot out of range");
        debug_assert!(cell_index < CELLS_PER_CHUNK, "cell_index out of range");
        &self.data[slot * CELLS_PER_CHUNK + cell_index]
    }

    /// Срез данных чанка
    #[inline]
    pub fn chunk_slice(&self, slot: usize) -> &[Cell] {
        debug_assert!(slot < SHADOW_SLOT_COUNT, "shadow slot out of range");
        let start = slot * CELLS_PER_CHUNK;
        let end = start + CELLS_PER_CHUNK;
        &self.data[start..end]
    }
}