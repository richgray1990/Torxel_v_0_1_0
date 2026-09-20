use bytemuck::{Pod, Zeroable};
use bevy::prelude::*;

/// Размер чанка по горизонтали (ячеек)
pub const CHUNK_SIDE: usize = 16;

/// Материал для блоков за пределами мира
pub const MATERIAL_OUT_OF_WORLD: u16 = 65535;

/// Размер пула по стороне (чанков). 32×32 = 1024
pub const POOL_SIDE: usize = 32;

/// Высота чанка (ячеек)
#[cfg(feature = "height64")]
pub const CHUNK_HEIGHT: usize = 64;

#[cfg(feature = "height128")]
pub const CHUNK_HEIGHT: usize = 128;

#[cfg(feature = "height256")]
pub const CHUNK_HEIGHT: usize = 256;

#[cfg(feature = "height512")]
pub const CHUNK_HEIGHT: usize = 512;

#[cfg(feature = "height1024")]
pub const CHUNK_HEIGHT: usize = 1024;

#[cfg(not(any(feature = "height64", feature = "height128", feature = "height256", feature = "height512", feature = "height1024")))]
pub const CHUNK_HEIGHT: usize = 64;

/// Всего ячеек в одном чанке
pub const CELLS_PER_CHUNK: usize = CHUNK_SIDE * CHUNK_HEIGHT * CHUNK_SIDE;

/// Размер активного окна (чанков)
pub const WINDOW_SIDE: usize = 16;

/// Всего чанков в активном окне
pub const WINDOW_CHUNK_COUNT: usize = WINDOW_SIDE * WINDOW_SIDE;

/// Размер пула (чанков)
pub const POOL_CHUNK_COUNT: usize = 1024;

// Реэкспорт материалов для обратной совместимости
pub use super::materials::ids::AIR as MATERIAL_AIR;
pub use super::materials::ids::WATER as MATERIAL_WATER;

// ═══════════════════════════════════════════════════════════
// Субчанки рендера
// ═══════════════════════════════════════════════════════════

pub const SUBCHUNK_SIDE: usize = 16;
pub const SUBCHUNK_HEIGHT: usize = 16;
pub const SUBCHUNKS_X: usize = CHUNK_SIDE / SUBCHUNK_SIDE;
pub const SUBCHUNKS_Y: usize = CHUNK_HEIGHT / SUBCHUNK_HEIGHT;
pub const SUBCHUNKS_Z: usize = CHUNK_SIDE / SUBCHUNK_SIDE;
pub const SUBCHUNKS_PER_CHUNK: usize = SUBCHUNKS_X * SUBCHUNKS_Y * SUBCHUNKS_Z;
pub const CELLS_PER_SUBCHUNK: usize = SUBCHUNK_SIDE * SUBCHUNK_HEIGHT * SUBCHUNK_SIDE;

#[inline]
pub fn subchunk_index(local_x: usize, local_y: usize, local_z: usize) -> usize {
    let sub_x = local_x / SUBCHUNK_SIDE;
    let sub_y = local_y / SUBCHUNK_HEIGHT;
    let sub_z = local_z / SUBCHUNK_SIDE;
    (sub_z * SUBCHUNKS_Y + sub_y) * SUBCHUNKS_X + sub_x
}

#[inline]
pub fn subchunk_index_from_cell(cell_index: usize) -> usize {
    let local_y = cell_index % CHUNK_HEIGHT;
    let local_x = (cell_index / CHUNK_HEIGHT) % CHUNK_SIDE;
    let local_z = cell_index / (CHUNK_HEIGHT * CHUNK_SIDE);
    subchunk_index(local_x, local_y, local_z)
}

#[inline]
pub fn all_subchunks_mask() -> u64 {
    if SUBCHUNKS_PER_CHUNK >= 64 {
        u64::MAX
    } else {
        (1u64 << SUBCHUNKS_PER_CHUNK) - 1
    }
}

// ═══════════════════════════════════════════════════════════
// Размеры мира
// ═══════════════════════════════════════════════════════════

/// Размеры мира в единицах мира (блоках).
/// Используется для тороидальной топологии в рендере.
#[derive(Resource, Clone, Copy, Debug)]
pub struct WorldDimensions {
    pub width: f64,
    pub depth: f64,
    pub height: f64,
}

impl WorldDimensions {
    pub fn new(chunks_x: u32, chunks_z: u32) -> Self {
        Self {
            width: chunks_x as f64 * CHUNK_SIDE as f64,
            depth: chunks_z as f64 * CHUNK_SIDE as f64,
            height: CHUNK_HEIGHT as f64,
        }
    }

    #[inline]
    pub fn wrap_x(&self, x: f64) -> f64 {
        let w = self.width;
        if w <= 0.0 { return x; }
        let mut r = x % w;
        if r < 0.0 { r += w; }
        r
    }

    #[inline]
    pub fn wrap_z(&self, z: f64) -> f64 {
        let d = self.depth;
        if d <= 0.0 { return z; }
        let mut r = z % d;
        if r < 0.0 { r += d; }
        r
    }

    #[inline]
    pub fn normalize_dx(&self, from_x: f64, to_x: f64) -> f64 {
        let mut dx = to_x - from_x;
        let half = self.width / 2.0;
        if dx > half { dx -= self.width; }
        if dx < -half { dx += self.width; }
        dx
    }

    #[inline]
    pub fn normalize_dz(&self, from_z: f64, to_z: f64) -> f64 {
        let mut dz = to_z - from_z;
        let half = self.depth / 2.0;
        if dz > half { dz -= self.depth; }
        if dz < -half { dz += self.depth; }
        dz
    }
}

// ═══════════════════════════════════════════════════════════
// Ячейка
// ═══════════════════════════════════════════════════════════

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cell {
    pub material_id: u16,
    pub phase_state: u8,
    pub liquid_level: u8,
    pub temperature: i16,
    pub chemical_mask: u16,

    pub vel_x: i16,
    pub vel_y: i16,
    pub vel_z: i16,
    pub _padding_vel: i16,

    pub pressure: i32,
    pub excess_level: i32,

    pub reserved_future: [u8; 40],
}

unsafe impl Pod for Cell {}
unsafe impl Zeroable for Cell {}