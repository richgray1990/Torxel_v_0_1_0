//! Координатор чанков, зон и жизненного цикла.

pub mod metadata;
pub mod manager;
pub mod masks;
pub mod window;
pub mod exclusive_pool;

pub use metadata::{SlotMetadata, SlotState};
pub use manager::ChunkManager;
pub use masks::{DirtyMask};
pub use window::{ReadSlotSnapshot, ReadWindowManager};
pub use exclusive_pool::{
    ExclusivePoolRegistry, LeaseBuffer, LeaseError, LeaseId, LeaseMode, LeaseOwner, LeaseState,
};