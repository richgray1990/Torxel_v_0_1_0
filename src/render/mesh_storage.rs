//! Хранилище мешей субчанков.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::voxel::format::{WINDOW_CHUNK_COUNT, SUBCHUNKS_PER_CHUNK};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubchunkRenderState {
    Empty,
    Building,
    Ready,
    Dirty,
    Rebuilding,
    Unloading,
}

pub struct SubchunkRenderData {
    pub current_mesh: Option<Handle<Mesh>>,
    pub pending_mesh: Option<Handle<Mesh>>,
    pub state: SubchunkRenderState,
    pub entity: Option<Entity>,
    pub chunk_x: i64,
    pub chunk_z: i64,
    pub subchunk_index: usize,
}

impl SubchunkRenderData {
    pub fn new(chunk_x: i64, chunk_z: i64, subchunk_index: usize) -> Self {
        Self {
            current_mesh: None,
            pending_mesh: None,
            state: SubchunkRenderState::Empty,
            entity: None,
            chunk_x,
            chunk_z,
            subchunk_index,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SubchunkKey {
    pub chunk_x: i64,
    pub chunk_z: i64,
    pub subchunk_index: usize,
}

#[derive(Resource)]
pub struct MeshStorage {
    pub subchunks: HashMap<SubchunkKey, SubchunkRenderData>,
    pub entity_count: usize,
}

impl MeshStorage {
    pub fn new() -> Self {
        Self {
            subchunks: HashMap::with_capacity(WINDOW_CHUNK_COUNT * SUBCHUNKS_PER_CHUNK),
            entity_count: 0,
        }
    }

    #[inline]
    pub fn get(&self, key: &SubchunkKey) -> Option<&SubchunkRenderData> {
        self.subchunks.get(key)
    }

    #[inline]
    pub fn get_mut(&mut self, key: &SubchunkKey) -> Option<&mut SubchunkRenderData> {
        self.subchunks.get_mut(key)
    }

    #[inline]
    pub fn insert(&mut self, key: SubchunkKey, data: SubchunkRenderData) {
        self.subchunks.insert(key, data);
    }

    #[inline]
    pub fn remove(&mut self, key: &SubchunkKey) -> Option<SubchunkRenderData> {
        self.subchunks.remove(key)
    }

    #[inline]
    pub fn contains(&self, key: &SubchunkKey) -> bool {
        self.subchunks.contains_key(key)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.subchunks.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.subchunks.is_empty()
    }

    pub fn ready_subchunks(&self) -> impl Iterator<Item = (&SubchunkKey, &SubchunkRenderData)> {
        self.subchunks.iter().filter(|(_, d)| d.state == SubchunkRenderState::Ready)
    }

    pub fn dirty_subchunks(&self) -> impl Iterator<Item = (&SubchunkKey, &SubchunkRenderData)> {
        self.subchunks.iter().filter(|(_, d)| d.state == SubchunkRenderState::Dirty)
    }

    pub fn pending_subchunks(&self) -> impl Iterator<Item = (&SubchunkKey, &SubchunkRenderData)> {
        self.subchunks.iter().filter(|(_, d)| {
            d.state == SubchunkRenderState::Building || d.state == SubchunkRenderState::Rebuilding
        })
    }
}

impl Default for MeshStorage {
    fn default() -> Self {
        Self::new()
    }
}