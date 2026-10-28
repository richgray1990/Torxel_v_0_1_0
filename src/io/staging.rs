//! Буферы для безопасной передачи данных из фонового потока в главный.
//!

use bevy::prelude::*;

use crate::voxel::format::Cell;
use crate::voxel::height_map::COLUMNS_PER_CHUNK;
use crate::io::channels::SaveResponse;

/// Загруженный чанк активного пула
pub struct StagedChunk {
    pub slot: usize,
    pub grid_x: i64,
    pub grid_z: i64,
    pub data: Vec<Cell>,
    pub column_tops: [u16; COLUMNS_PER_CHUNK],
}

/// Буфер активного пула
#[derive(Resource, Default)]
pub struct ActiveStagingBuffer {
    pub loaded_chunks: Vec<StagedChunk>,
    pub saved_chunks: Vec<SaveResponse>,
}

impl ActiveStagingBuffer {
    pub fn take_loaded(&mut self) -> Vec<StagedChunk> {
        std::mem::take(&mut self.loaded_chunks)
    }

    pub fn take_saved(&mut self) -> Vec<SaveResponse> {
        std::mem::take(&mut self.saved_chunks)
    }

    pub fn push_loaded(&mut self, chunk: StagedChunk) {
        self.loaded_chunks.push(chunk);
    }

    pub fn push_saved(&mut self, response: SaveResponse) {
        self.saved_chunks.push(response);
    }
}