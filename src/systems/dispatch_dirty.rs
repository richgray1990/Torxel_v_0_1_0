//! Передача событий из PostSwapDirtyBuffer в MeshUpdateQueue.

use bevy::prelude::*;

use crate::queues::mesh_queue::MeshUpdateQueue;
use crate::queues::PostSwapDirtyBuffer;
use crate::systems::perf::{Bucket, PerfStats};

/// Сливаем события из буфера PostSwapCopy в основную очередь рендера
pub fn dispatch_dirty_events_system(
    mut dirty_buffer: ResMut<PostSwapDirtyBuffer>,
    mut mesh_queue: ResMut<MeshUpdateQueue>,
    stats: Res<PerfStats>,
) {
    let t = std::time::Instant::now();

    if dirty_buffer.is_empty() {
        stats.record(Bucket::DispatchDirty, t.elapsed());
        return;
    }

    for event in dirty_buffer.drain() {
        mesh_queue.push(event);
    }

    stats.record(Bucket::DispatchDirty, t.elapsed());
}