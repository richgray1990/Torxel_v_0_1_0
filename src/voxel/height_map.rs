//! Карта верхнего блока на столбец чанка.
//!
//! Одно значение `u16` на колонку 16×16: `top_block_y + 1`, то есть `y` первой
//! воздушной ячейки над верхним блоком. `0` — колонка пуста. Значение влезает
//! в `u16`, потому что `CHUNK_HEIGHT = 512`.
//!
//! Пещеры и нависания под верхним блоком карта не различает: потребители, кому
//! нужна точность ниже верхнего блока, ей не пользуются.

use bevy::prelude::*;

use super::format::{Cell, CHUNK_SIDE, CHUNK_HEIGHT};

/// Колонок в чанке (16×16).
pub const COLUMNS_PER_CHUNK: usize = CHUNK_SIDE * CHUNK_SIDE;

/// Индекс колонки внутри чанка. Совпадает с `(local_z * CHUNK_SIDE + local_x)`
/// из раскладки ячеек, поэтому скан колонки кэш-линеен.
#[inline(always)]
pub const fn column_index(local_x: usize, local_z: usize) -> usize {
    local_z * CHUNK_SIDE + local_x
}

/// Плоский массив карт по слотам пула.
pub struct HeightMap {
    heights: Box<[u16]>,
    slot_count: usize,
}

impl std::fmt::Debug for HeightMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Печатать 262144 значения бессмысленно — только форму.
        f.debug_struct("HeightMap")
            .field("slot_count", &self.slot_count)
            .finish()
    }
}

impl HeightMap {
    pub fn new(slot_count: usize) -> Self {
        Self {
            heights: vec![0u16; slot_count * COLUMNS_PER_CHUNK].into_boxed_slice(),
            slot_count,
        }
    }

    /// Высота колонки: `0` — колонка пуста, иначе `y` первой воздушной ячейки.
    #[inline(always)]
    pub fn top_y(&self, slot: usize, col: usize) -> u16 {
        debug_assert!(slot < self.slot_count, "slot out of range");
        debug_assert!(col < COLUMNS_PER_CHUNK, "column out of range");
        self.heights[slot * COLUMNS_PER_CHUNK + col]
    }

    #[inline(always)]
    pub fn set_top_y(&mut self, slot: usize, col: usize, top: u16) {
        debug_assert!(slot < self.slot_count, "slot out of range");
        debug_assert!(col < COLUMNS_PER_CHUNK, "column out of range");
        self.heights[slot * COLUMNS_PER_CHUNK + col] = top;
    }

    /// 256 значений подряд — для LOD-меша, читается кэш-линейно.
    #[inline]
    pub fn column_slice(&self, slot: usize) -> &[u16] {
        debug_assert!(slot < self.slot_count, "slot out of range");
        let start = slot * COLUMNS_PER_CHUNK;
        &self.heights[start..start + COLUMNS_PER_CHUNK]
    }

    #[inline]
    pub fn column_slice_mut(&mut self, slot: usize) -> &mut [u16] {
        debug_assert!(slot < self.slot_count, "slot out of range");
        let start = slot * COLUMNS_PER_CHUNK;
        &mut self.heights[start..start + COLUMNS_PER_CHUNK]
    }

    #[inline]
    pub fn copy_slot_from(&mut self, src: &HeightMap, slot: usize) {
        debug_assert!(slot < self.slot_count, "slot out of range");
        debug_assert!(slot < src.slot_count, "source slot out of range");
        let start = slot * COLUMNS_PER_CHUNK;
        self.heights[start..start + COLUMNS_PER_CHUNK]
            .copy_from_slice(&src.heights[start..start + COLUMNS_PER_CHUNK]);
    }

    #[inline]
    pub fn clear_slot(&mut self, slot: usize) {
        self.column_slice_mut(slot).fill(0);
    }

    /// Копирует карту целиком из другой того же размера.
    ///
    /// Один `memcpy` на 512 КиБ вместо поразрядной dirty-маски по слотам:
    /// при публикации снапшота важно не сэкономить на копии, а не разойтись
    /// с содержимым пула.
    #[inline]
    pub fn copy_from_all(&mut self, src: &HeightMap) {
        debug_assert_eq!(self.heights.len(), src.heights.len());
        self.heights.copy_from_slice(&src.heights);
    }
}

/// Ресурс карты для чтения — спутник `ReadWorld`.
#[derive(Resource)]
pub struct ReadHeightMap(pub HeightMap);

/// Ресурс карты для записи — спутник `WriteWorld`.
#[derive(Resource)]
pub struct WriteHeightMap(pub HeightMap);

/// Считает верхние блоки всех колонок чанка одним проходом.
///
/// Читает `cells` в том же порядке, в котором они лежат в пуле, поэтому ячейка
/// колонки берётся из кэша: прогон идёт по `local_y` внутри фиксированного
/// `(local_x, local_z)`, то есть шагами по 512 Б без прыжков.
pub fn column_tops(cells: &[Cell]) -> [u16; COLUMNS_PER_CHUNK] {
    let mut tops = [0u16; COLUMNS_PER_CHUNK];
    let mut index = 0usize;

    for local_z in 0..CHUNK_SIDE {
        for local_x in 0..CHUNK_SIDE {
            for local_y in 0..CHUNK_HEIGHT {
                let cell = &cells[index];
                index += 1;

                if super::materials::is_air(cell.material_id) {
                    continue;
                }

                let col = column_index(local_x, local_z);
                // Колонка заполняется снизу вверх: большее `y` перекрывает меньшее.
                if tops[col] < (local_y + 1) as u16 {
                    tops[col] = (local_y + 1) as u16;
                }
            }
        }
    }

    tops
}

/// Пересчитывает одну колонку по данным пула.
///
/// Нужен при правке ячейки: скан сверху вниз до первого непустого блока,
/// не более `CHUNK_HEIGHT` шагов на правку.
#[inline]
pub fn recompute_column(pool: &super::pool::ChunkPool, slot: usize, col: usize) -> u16 {
    let local_x = col % CHUNK_SIDE;
    let local_z = col / CHUNK_SIDE;
    let column_base = (local_z * CHUNK_SIDE + local_x) * CHUNK_HEIGHT;

    for local_y in (0..CHUNK_HEIGHT).rev() {
        if !super::materials::is_air(pool.get(slot, column_base + local_y).material_id) {
            return (local_y + 1) as u16;
        }
    }

    0
}
