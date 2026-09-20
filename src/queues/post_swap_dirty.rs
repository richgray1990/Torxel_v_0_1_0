//! Буфер событий изменённых чанков после свапа.
//!
//! Заполняется в PostSwapCopy, передаётся в MeshUpdateQueue
//! в начале следующего кадра через DispatchDirtyEvents.

use bevy::prelude::*;
use smallvec::SmallVec;

use super::mesh_queue::MeshUpdateEvent;

/// Буфер событий изменённых чанков (после свапа)
#[derive(Resource, Default)]
pub struct PostSwapDirtyBuffer {
    pub events: SmallVec<[MeshUpdateEvent; 64]>,
}

impl PostSwapDirtyBuffer {
    pub fn push(&mut self, event: MeshUpdateEvent) {
        self.events.push(event);
    }

    pub fn drain(&mut self) -> impl Iterator<Item = MeshUpdateEvent> + '_ {
        self.events.drain(..)
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}