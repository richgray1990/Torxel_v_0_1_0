//! Обработка запросов теневого пула от систем обсчёта.
//!
//! Не проверяет активное окно. Если чанк в активном окне,
//! система обсчёта сама читает из ReadWorld.

use bevy::prelude::*;

use crate::io::channels::{IoManager, ShadowLoadRequest};
use crate::queues::shadow_requests::{ShadowRequestBuffer, ShadowRequestKind};
use crate::voxel::shadow_pool::{ShadowPool, ShadowRequestResult};

/// Система обработки теневых запросов
pub fn process_shadow_requests_system(
    mut shadow_buffer: ResMut<ShadowRequestBuffer>,
    mut shadow_pool: ResMut<ShadowPool>,
    io_manager: Res<IoManager>,
) {
    for request in shadow_buffer.drain() {
        match request.kind {
            ShadowRequestKind::Request => {
                let chunk_x = request.chunk_x as i64;
                let chunk_z = request.chunk_z as i64;

                let result = shadow_pool.request_chunk(chunk_x, chunk_z);

                match result {
                    ShadowRequestResult::Queued(slot) => {
                        io_manager.queue_shadow_load(ShadowLoadRequest {
                            slot,
                            chunk_x: request.chunk_x,
                            chunk_z: request.chunk_z,
                        });
                    }
                    ShadowRequestResult::Ready(_) => {
                        // Уже в пуле, ничего не делаем
                    }
                    ShadowRequestResult::InActiveWindow => {
                        // Не используется в этой реализации
                    }
                    ShadowRequestResult::NotAvailable => {
                        eprintln!(
                            "Shadow pool full, cannot load chunk ({}, {})",
                            request.chunk_x, request.chunk_z
                        );
                    }
                }
            }
            ShadowRequestKind::Release => {
                let chunk_x = request.chunk_x as i64;
                let chunk_z = request.chunk_z as i64;
                shadow_pool.release_chunk(chunk_x, chunk_z);
            }
        }
    }
}