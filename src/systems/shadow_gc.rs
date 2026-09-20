//! Сборщик мусора теневого пула.

use bevy::prelude::*;

use crate::voxel::shadow_pool::ShadowPool;

/// Система выгрузки неиспользуемых теневых чанков
pub fn shadow_gc_system(mut shadow_pool: ResMut<ShadowPool>) {
    shadow_pool.tick_unload();
}