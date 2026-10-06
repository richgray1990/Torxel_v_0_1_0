//! Очереди событий между системами.

pub mod mesh_queue;
pub mod cell_events;
pub mod post_swap_dirty;

pub use mesh_queue::{MeshUpdateEvent, MeshUpdateKind, MeshUpdateQueue};
pub use cell_events::{CellEvent, CellEventKind, CellEventQueue};
pub use post_swap_dirty::PostSwapDirtyBuffer;