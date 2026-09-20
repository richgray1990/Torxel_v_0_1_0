//! Системы Bevy, разделённые по фазам графа.

pub mod poll_tasks;
pub mod poll_shadow_tasks;
pub mod update_window;
pub mod initial_copy;
pub mod shadow_copy;
pub mod gc;
pub mod apply_events;
pub mod swap;
pub mod dispatch_dirty;
pub mod dispatch_loaded;
pub mod request_swap;
pub mod shadow_requests;
pub mod shadow_gc;
pub mod render_stub;

pub use poll_tasks::poll_background_tasks_system;
pub use poll_shadow_tasks::poll_shadow_tasks_system;
pub use update_window::{initialize_window_system, update_window_system, WindowInitialized};
pub use initial_copy::{initial_copy_system, InitialCopyDone};
pub use shadow_copy::shadow_copy_system;
pub use gc::chunk_garbage_collector_system;
pub use apply_events::apply_cell_events_system;
pub use swap::{swap_pointers_system, post_swap_copy_system};
pub use dispatch_dirty::dispatch_dirty_events_system;
pub use dispatch_loaded::dispatch_loaded_events_system;
pub use request_swap::request_swap_system;
pub use shadow_requests::process_shadow_requests_system;
pub use shadow_gc::shadow_gc_system;
pub use render_stub::render_stub_system;