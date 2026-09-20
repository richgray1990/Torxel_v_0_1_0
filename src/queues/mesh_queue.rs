//! Очередь обновлений мешей для рендера.

use bevy::prelude::*;
use smallvec::SmallVec;

/// Тип обновления меша
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshUpdateKind {
    ChunkLoaded,
    ChunkDirty,
    ChunkUnloaded,
    ShadowChunkLoaded,
    ShadowChunkUnloaded,
}


/// Источник данных для рендера
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderSource {
    Active,
    Shadow,
}

/// Событие обновления меша
#[derive(Clone, Copy, Debug)]
pub struct MeshUpdateEvent {
    pub kind: MeshUpdateKind,
    pub source: RenderSource,
    pub slot_index: usize,
    pub grid_x: i64,
    pub grid_z: i64,
    pub dirty_subchunks: u64,
}

/// Очередь обновлений для рендера
#[derive(Resource, Default)]
pub struct MeshUpdateQueue {
    pub events: SmallVec<[MeshUpdateEvent; 64]>,
}

impl MeshUpdateQueue {
    pub fn push(&mut self, event: MeshUpdateEvent) {
        self.events.push(event);
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &MeshUpdateEvent> {
        self.events.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}