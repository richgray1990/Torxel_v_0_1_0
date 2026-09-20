//! Координатор чанков, зон и жизненного цикла.

pub mod metadata;
pub mod manager;
pub mod masks;
pub mod shadow_copy;

pub use metadata::{SlotMetadata, SlotState};
pub use manager::ChunkManager;
pub use masks::{DirtyMask, ShadowMask};
pub use shadow_copy::{ShadowCopyManager, ShadowCopyState, ShadowCopyValidation};