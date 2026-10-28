//! Формат файла мира.
use crate::voxel::format::{CELLS_PER_CHUNK, Cell};
use crate::voxel::height_map::COLUMNS_PER_CHUNK;
use bevy::prelude::*;
use bytemuck::{Pod, Zeroable};
/// Магическое число заголовка мира
pub const WORLD_MAGIC: u32 = 0x54584C56; // "TXLV"
/// Магическое число заголовка чанка
pub const CHUNK_MAGIC: u32 = 0x43484E4B; // "CHNK"
/// Текущая версия формата
pub const FORMAT_VERSION: u32 = 3;
/// Версия заголовка чанка.
///
/// 3 — Заголовок несёт column_tops и весит 532 Б. Чанки старого файла
/// лежат по другому смещению, поэтому is_valid обязан их отсечь: иначе ячейки
/// читались бы с середины чужого заголовка
/// 
pub const CHUNK_VERSION: u16 = 2;
/// Флаги чанка
pub mod chunk_flags {
    pub const COMPRESSED: u16 = 0x0001;
    pub const DIRTY: u16 = 0x0002;
    pub const EMPTY: u16 = 0x0004;
}
/// Заголовок одного чанка в файле
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ChunkHeader {
    pub magic: u32,
    pub version: u16,
    pub flags: u16,
    pub data_size: u32,
    pub checksum: u32,
    pub cell_count: u32,
    pub column_tops: [u16; COLUMNS_PER_CHUNK],
}
unsafe impl Pod for ChunkHeader {}
unsafe impl Zeroable for ChunkHeader {}
const _: () = assert!(
    std::mem::size_of::<ChunkHeader>() == 532,
    "ChunkHeader must be exactly 540 bytes"
);
impl ChunkHeader {
    pub fn new(data_size: u32, cell_count: u32) -> Self {
        Self {
            magic: CHUNK_MAGIC,
            version: CHUNK_VERSION,
            flags: 0,
            data_size,
            checksum: 0,
            cell_count,
            column_tops: [0; COLUMNS_PER_CHUNK],
        }
    }

    pub fn is_valid(&self) -> bool {
        self.magic == CHUNK_MAGIC && self.version == CHUNK_VERSION
    }

    pub fn is_compressed(&self) -> bool {
        self.flags & chunk_flags::COMPRESSED != 0
    }
}

impl Default for ChunkHeader {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

/// Заголовок файла мира
#[repr(C)]
#[derive(Resource, Clone, Copy, Debug)]
pub struct WorldHeader {
pub magic: u32,
pub version: u32,
pub chunks_x: u32,
pub chunks_z: u32,
pub chunk_height: u32,
pub spawn_x: i32,
pub spawn_z: i32,
pub chunk_header_size: u32,
pub world_seed: u32,
pub _padding: [u8; 28],
}
unsafe impl Pod for WorldHeader {}
unsafe impl Zeroable for WorldHeader {}
const _: () = assert!(
std::mem::size_of::<WorldHeader>() == 64,
"WorldHeader must be exactly 64 bytes"
);
impl WorldHeader {
pub fn new(chunks_x: u32, chunks_z: u32, chunk_height: u32) -> Self {
Self {
magic: WORLD_MAGIC,
version: FORMAT_VERSION,
chunks_x,
chunks_z,
chunk_height,
spawn_x: 0,
spawn_z: 0,
chunk_header_size: std::mem::size_of::<ChunkHeader>() as u32,
world_seed: 0,
_padding: [0; 28],
}
}

    pub fn with_spawn(mut self, spawn_x: i32, spawn_z: i32) -> Self {
        self.spawn_x = spawn_x;
        self.spawn_z = spawn_z;
        self
    }

    pub fn is_valid(&self) -> bool {
        self.magic == WORLD_MAGIC && self.version == FORMAT_VERSION
    }

    /// Размер одного чанка в файле (заголовок + данные)
    pub fn chunk_total_size(&self) -> u64 {
        self.chunk_header_size as u64 + (CELLS_PER_CHUNK * std::mem::size_of::<Cell>()) as u64
    }

    pub fn with_world_seed(&mut self, seed: u32) {
        self.world_seed = seed;
    }
}
