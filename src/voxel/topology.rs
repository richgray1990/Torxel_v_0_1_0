//! Тороидальная топология мира.

use super::format::{CHUNK_HEIGHT, CHUNK_SIDE, WINDOW_SIDE};

/// Тороидальная топология мира
#[derive(Clone, Debug)]
pub struct TorusTopology {
    pub chunks_x: u64,
    pub chunks_z: u64,
    pub min_block_x: i64,
    pub min_block_z: i64,
}

impl TorusTopology {
    pub fn new(chunks_x: u64, chunks_z: u64) -> Self {
        Self {
            chunks_x,
            chunks_z,
            min_block_x: 0,
            min_block_z: 0,
        }
    }

    pub fn with_offset(mut self, min_block_x: i64, min_block_z: i64) -> Self {
        self.min_block_x = min_block_x;
        self.min_block_z = min_block_z;
        self
    }

    /// Нормализует координату чанка (тороидальная обёртка)
    #[inline(always)]
    pub fn normalize_chunk(&self, grid_x: i64, grid_z: i64) -> (usize, usize) {
        let cx = grid_x.rem_euclid(self.chunks_x as i64) as usize;
        let cz = grid_z.rem_euclid(self.chunks_z as i64) as usize;
        (cx, cz)
    }

    /// Нормализует координату блока.
    /// Возвращает (chunk_x, chunk_z, local_x, local_y, local_z).
    /// None если y вне диапазона.
    #[inline(always)]
    pub fn normalize_block(
        &self,
        x: i64,
        y: i64,
        z: i64,
    ) -> Option<(usize, usize, usize, usize, usize)> {
        if y < 0 || y >= CHUNK_HEIGHT as i64 {
            return None;
        }

        let rel_x = x - self.min_block_x;
        let rel_z = z - self.min_block_z;

        let chunk_x = rel_x.div_euclid(CHUNK_SIDE as i64);
        let chunk_z = rel_z.div_euclid(CHUNK_SIDE as i64);

        let local_x = rel_x.rem_euclid(CHUNK_SIDE as i64) as usize;
        let local_z = rel_z.rem_euclid(CHUNK_SIDE as i64) as usize;
        let local_y = y as usize;

        let (cx, cz) = self.normalize_chunk(chunk_x, chunk_z);

        Some((cx, cz, local_x, local_y, local_z))
    }

    /// Индекс ячейки внутри чанка
    #[inline(always)]
    pub fn cell_index(&self, local_x: usize, local_y: usize, local_z: usize) -> usize {
        (local_z * CHUNK_SIDE + local_x) * CHUNK_HEIGHT + local_y
    }

    /// Обратное преобразование: индекс ячейки → локальные координаты
    #[inline(always)]
    pub fn cell_index_to_local(&self, cell_index: usize) -> (usize, usize, usize) {
        let local_y = cell_index % CHUNK_HEIGHT;
        let local_x = (cell_index / CHUNK_HEIGHT) % CHUNK_SIDE;
        let local_z = cell_index / (CHUNK_HEIGHT * CHUNK_SIDE);
        (local_x, local_y, local_z)
    }

    /// Мировые координаты чанка
    #[inline(always)]
    pub fn chunk_to_world(&self, chunk_x: usize, chunk_z: usize) -> (i64, i64) {
        let world_x = (chunk_x * CHUNK_SIDE) as i64 + self.min_block_x;
        let world_z = (chunk_z * CHUNK_SIDE) as i64 + self.min_block_z;
        (world_x, world_z)
    }

    /// Возвращает индекс слота в активном окне.
    /// None если чанк вне окна.
    /// ИСПРАВЛЕНО: работает с тороидальной топологией
    #[inline(always)]
    pub fn slot_index_in_window(
        &self,
        chunk_x: usize,
        chunk_z: usize,
        window_min_x: i64,
        window_min_z: i64,
    ) -> Option<usize> {
        // Нормализуем координаты чанка относительно window_min через тор
        let dx = (chunk_x as i64 - window_min_x).rem_euclid(self.chunks_x as i64);
        let dz = (chunk_z as i64 - window_min_z).rem_euclid(self.chunks_z as i64);

        if dx >= WINDOW_SIDE as i64 || dz >= WINDOW_SIDE as i64 {
            return None;
        }

        Some((dz as usize) * WINDOW_SIDE + (dx as usize))
    }
}