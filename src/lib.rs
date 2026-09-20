//! # Torxel
//!
//! Плагин менеджера чанков для воксельного мира с тороидальной топологией.

pub mod voxel;
pub mod manager;
pub mod io;
pub mod queues;
pub mod systems;
pub mod signals;
pub mod plugin;
pub mod render;

pub use voxel::format::{
    Cell, CHUNK_HEIGHT, CHUNK_SIDE, CELLS_PER_CHUNK, MATERIAL_AIR, MATERIAL_OUT_OF_WORLD,
    POOL_CHUNK_COUNT, POOL_SIDE, WINDOW_CHUNK_COUNT, WINDOW_SIDE,
};
pub use voxel::topology::TorusTopology;
pub use voxel::pool::{ChunkPool, ReadWorld, WriteWorld};
pub use voxel::shadow_pool::{
    ShadowPool, ShadowSlotState, ShadowRequestResult,
    SHADOW_POOL_SIDE, SHADOW_SLOT_COUNT,
};
pub use manager::{
    ChunkManager, SlotMetadata, SlotState,
    ShadowCopyManager, ShadowCopyState, ShadowCopyValidation,
};
pub use plugin::{
    ComputeBlockSet, GameFlowSet, GCPhaseSet, ParallelWorkSet, PollPhaseSet,
    PostSwapBranchSet, TorxelPlugin, UpdatePhaseSet,
};
pub use signals::{ComputePhase, ComputePipeline, SwapSignal};
pub use io::file_format::WorldHeader;
pub use io::staging::{ActiveStagingBuffer, ShadowStagingBuffer};
pub use queues::PostSwapDirtyBuffer;