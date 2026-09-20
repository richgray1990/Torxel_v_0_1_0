//! Система обработки копирования из теневого пула в активный.

use bevy::prelude::*;

use crate::manager::shadow_copy::{ShadowCopyManager, ShadowCopyState, ShadowCopyValidation};
use crate::voxel::pool::WriteWorld;
use crate::voxel::shadow_pool::ShadowPool;

/// Система копирования из теневого пула в активный
pub fn shadow_copy_system(
    mut copy_manager: ResMut<ShadowCopyManager>,
    shadow_pool: Res<ShadowPool>,
    mut write_world: ResMut<WriteWorld>,
) {
    match copy_manager.state {
        ShadowCopyState::Validating => {
            let validation = copy_manager.validate(&shadow_pool);

            match validation {
                ShadowCopyValidation::Ready => {
                    copy_manager.state = ShadowCopyState::Copying;
                }
                ShadowCopyValidation::Loading => {
                    // Ждём следующий кадр
                }
                ShadowCopyValidation::Missing => {
                    copy_manager.state = ShadowCopyState::Failed;
                }
                ShadowCopyValidation::GeometryMismatch => {
                    copy_manager.state = ShadowCopyState::Failed;
                }
            }
        }

        ShadowCopyState::Copying => {
            let success = copy_manager.execute_copy(
                &shadow_pool,
                &mut write_world.0.data,
            );

            if success {
                copy_manager.state = ShadowCopyState::Complete;
            } else {
                copy_manager.state = ShadowCopyState::Failed;
            }
        }

        ShadowCopyState::Complete | ShadowCopyState::Failed | ShadowCopyState::Idle => {}
    }
}