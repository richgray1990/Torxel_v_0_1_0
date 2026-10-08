//! Начальное копирование данных из WriteWorld в ReadWorld.
use bevy::prelude::*;

use crate::voxel::format::POOL_CHUNK_COUNT;
use crate::voxel::pool::{ReadWorld, WriteWorld};

use crate::manager::{ChunkManager, ReadWindowManager, SlotState};
use crate::signals::{ComputePhase, ComputePipeline};

/// Флаг завершения начального копирования.
#[derive(Resource, Default)]
pub struct InitialCopyDone {
    pub done: bool,
}

/// Система начального копирования.
pub fn initial_copy_system(
    mut read_world: ResMut<ReadWorld>,
    write_world: Res<WriteWorld>,
    mut manager: ResMut<ChunkManager>,
    mut read_window: ResMut<ReadWindowManager>,
    mut pipeline: ResMut<ComputePipeline>,
    mut initial_copy: ResMut<InitialCopyDone>,
) {
    if initial_copy.done {
        return;
    }

    // Ждём, пока все начальные чанки перестанут быть Queued/Loading.
    let all_resolved = manager.metadata.iter().all(|meta| {
        meta.state != SlotState::Queued && meta.state != SlotState::Loading
    });

    if !all_resolved {
        return;
    }

    let mut copied = 0;

    for slot in 0..POOL_CHUNK_COUNT {
        let meta = &manager.metadata[slot];

        if meta.state == SlotState::Ready && meta.is_active {
            read_world.0.copy_chunk_from(&write_world.0, slot);

            manager.metadata[slot].pending_b_dirty = false;
            manager.metadata[slot].render_notified = false;

            copied += 1;
        }
    }

    println!(
        "[INITIAL_COPY] DONE! Copied {} chunks from WriteWorld to ReadWorld",
        copied
    );

    read_window.publish_from(&manager);

    initial_copy.done = true;
    pipeline.phase = ComputePhase::Computing;
}