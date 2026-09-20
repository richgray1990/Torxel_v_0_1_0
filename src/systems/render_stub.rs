//! Заглушки для очистки очередей.

use bevy::prelude::*;

use crate::queues::mesh_queue::MeshUpdateQueue;

/// Заглушка рендера: очищает очередь обновлений мешей
pub fn render_stub_system(mut mesh_queue: ResMut<MeshUpdateQueue>) {
    mesh_queue.clear();
}