//! Плагин менеджера чанков.

use bevy::prelude::*;

use crate::render::{
    CameraController, camera_input_system, camera_transform_system,
};
use crate::render::mesh_storage::MeshStorage;
use crate::render::render_systems::{
    process_mesh_events_system, build_meshes_system, swap_meshes_system,
    update_mesh_entities_system, cleanup_mesh_entities_system,
};

use crate::voxel::pool::{ChunkPool, ReadWorld, WriteWorld};
use crate::voxel::shadow_pool::ShadowPool;
use crate::voxel::topology::TorusTopology;
use crate::io::channels::IoManager;
use crate::io::file_format::WorldHeader;
use crate::io::staging::{ActiveStagingBuffer, ShadowStagingBuffer};
use crate::io::worker::IoWorker;
use crate::manager::{
    ChunkManager, ShadowCopyManager, SlotMetadata, SlotState,
};
use crate::queues::cell_events::CellEventQueue;
use crate::queues::mesh_queue::MeshUpdateQueue;
use crate::queues::shadow_requests::ShadowRequestBuffer;
use crate::queues::PostSwapDirtyBuffer;
use crate::signals::{ComputePipeline, SwapSignal};
use crate::systems::{
    apply_cell_events_system, chunk_garbage_collector_system, dispatch_dirty_events_system,
    dispatch_loaded_events_system,
    initial_copy_system, initialize_window_system, poll_background_tasks_system,
    poll_shadow_tasks_system, post_swap_copy_system, process_shadow_requests_system,
    request_swap_system, shadow_copy_system, shadow_gc_system,
    swap_pointers_system, update_window_system, InitialCopyDone, WindowInitialized,
};
use crate::systems::debug::{debug_report_system, DebugTimer};

/// Системные сеты (уровень 1 — строгая цепочка)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameFlowSet {
    /// Фаза опроса фоновых задач (параллельно: активный + теневой)
    PollPhase,
    /// Начальное копирование
    InitialCopy,
    /// Фаза обновления (параллельно: окно + теневые запросы)
    UpdatePhase,
    /// Копирование из теневого пула в активный
    ShadowCopy,
    /// Обмен указателей активного пула
    SwapPointers,
    /// Передача событий рендеру о загрузке чанков
    DispatchLoadedEvents,
    /// Передача событий рендеру об изменении чанков
    DispatchDirtyEvents,
    /// Окно параллелизма (рендер + обсчёт)
    ParallelWork,
    /// Запрос свапа на следующем кадре
    RequestSwap,
    /// Фаза сборки мусора (параллельно: активный + теневой)
    GCPhase,
}

/// Системные сеты внутри PollPhase (параллельно)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PollPhaseSet {
    PollTasks,
    PollShadowTasks,
}

/// Системные сеты внутри UpdatePhase (параллельно)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum UpdatePhaseSet {
    UpdateWindow,
    ShadowRequests,
}

/// Системные сеты внутри ParallelWork (параллельные ветки)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParallelWorkSet {
    Render,
    PostSwapBranch,
}

/// Системные сеты внутри PostSwapBranch (цепочка)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PostSwapBranchSet {
    PostSwapCopy,
    ComputeBlock,
}

/// Системные сеты внутри ComputeBlock (цепочка)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ComputeBlockSet {
    Subsystems,
    Aggregate,
}

/// Системные сеты внутри GCPhase (параллельно)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GCPhaseSet {
    GarbageCollect,
    ShadowGarbageCollect,
}

/// Плагин менеджера чанков
pub struct TorxelPlugin {
    pub topology: TorusTopology,
    pub world_path: std::path::PathBuf,
}

impl Plugin for TorxelPlugin {
    fn build(&self, app: &mut App) {
        let header = WorldHeader::new(
            self.topology.chunks_x as u32,
            self.topology.chunks_z as u32,
            crate::voxel::format::CHUNK_HEIGHT as u32,
        );

        let (io_manager, worker_channels) = IoManager::new();
        let _worker = IoWorker::spawn(worker_channels, self.world_path.clone(), header);

        // Регистрация ресурсов
        app.insert_resource(ChunkManager::new(self.topology.clone()));
        app.insert_resource(ReadWorld(ChunkPool::new_max()));
        app.insert_resource(WriteWorld(ChunkPool::new_max()));
        app.insert_resource(ShadowPool::new(self.topology.clone()));
        app.insert_resource(ComputePipeline::default());
        app.insert_resource(io_manager);
        app.insert_resource(header);
        app.init_resource::<SwapSignal>();
        app.init_resource::<MeshUpdateQueue>();
        app.init_resource::<CellEventQueue>();
        app.init_resource::<ShadowRequestBuffer>();
        app.init_resource::<PostSwapDirtyBuffer>();
        app.init_resource::<ActiveStagingBuffer>();
        app.init_resource::<ShadowStagingBuffer>();
        app.init_resource::<WindowInitialized>();
        app.init_resource::<InitialCopyDone>();
        app.init_resource::<ShadowCopyManager>();
        app.init_resource::<CameraController>();
        app.init_resource::<MeshStorage>();
        app.init_resource::<DebugTimer>();

        // ═══════════════════════════════════════════════════════════
        // Уровень 1: строгая цепочка фаз
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (
                GameFlowSet::PollPhase,
                GameFlowSet::InitialCopy,
                GameFlowSet::UpdatePhase,
                GameFlowSet::ShadowCopy,
                GameFlowSet::SwapPointers,
                GameFlowSet::DispatchLoadedEvents,
                GameFlowSet::DispatchDirtyEvents,
                GameFlowSet::ParallelWork,
                GameFlowSet::RequestSwap,
                GameFlowSet::GCPhase,
            )
                .chain(),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 2: PollPhase — параллельно
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (PollPhaseSet::PollTasks, PollPhaseSet::PollShadowTasks)
                .in_set(GameFlowSet::PollPhase),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 2: UpdatePhase — параллельно
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (UpdatePhaseSet::UpdateWindow, UpdatePhaseSet::ShadowRequests)
                .in_set(GameFlowSet::UpdatePhase),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 2: ParallelWork — параллельные ветки
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (ParallelWorkSet::Render, ParallelWorkSet::PostSwapBranch)
                .in_set(GameFlowSet::ParallelWork),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 3: PostSwapBranch — цепочка
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (PostSwapBranchSet::PostSwapCopy, PostSwapBranchSet::ComputeBlock)
                .chain()
                .in_set(ParallelWorkSet::PostSwapBranch),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 4: ComputeBlock — цепочка
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (ComputeBlockSet::Subsystems, ComputeBlockSet::Aggregate)
                .chain()
                .in_set(PostSwapBranchSet::ComputeBlock),
        );

        // ═══════════════════════════════════════════════════════════
        // Уровень 2: GCPhase — параллельно
        // ═══════════════════════════════════════════════════════════
        app.configure_sets(
            Update,
            (GCPhaseSet::GarbageCollect, GCPhaseSet::ShadowGarbageCollect)
                .in_set(GameFlowSet::GCPhase),
        );

        // ═══════════════════════════════════════════════════════════
        // Регистрация систем
        // ═══════════════════════════════════════════════════════════

        // Startup
        app.add_systems(Startup, initialize_window_system);

        // PollPhase (параллельно)
        app.add_systems(
            Update,
            poll_background_tasks_system.in_set(PollPhaseSet::PollTasks),
        );
        app.add_systems(
            Update,
            poll_shadow_tasks_system.in_set(PollPhaseSet::PollShadowTasks),
        );

        // InitialCopy
        app.add_systems(
            Update,
            initial_copy_system.in_set(GameFlowSet::InitialCopy),
        );

        // UpdatePhase (параллельно)
        app.add_systems(
            Update,
            update_window_system.in_set(UpdatePhaseSet::UpdateWindow),
        );
        app.add_systems(
            Update,
            process_shadow_requests_system.in_set(UpdatePhaseSet::ShadowRequests),
        );

        // SwapPointers
        app.add_systems(
            Update,
            swap_pointers_system.in_set(GameFlowSet::SwapPointers),
        );

        // DispatchLoadedEvents
        app.add_systems(
            Update,
            dispatch_loaded_events_system.in_set(GameFlowSet::DispatchLoadedEvents),
        );

        // DispatchDirtyEvents
        app.add_systems(
            Update,
            dispatch_dirty_events_system.in_set(GameFlowSet::DispatchDirtyEvents),
        );

        // ═══════════════════════════════════════════════════════════
        // ParallelWork — две параллельные ветки
        // ═══════════════════════════════════════════════════════════

        // Ветка Render: цепочка систем рендера
        app.add_systems(
            Update,
            (
                camera_input_system,
                process_mesh_events_system,
                build_meshes_system,
                swap_meshes_system,
                update_mesh_entities_system,
                cleanup_mesh_entities_system,
                camera_transform_system,
            )
                .chain()
                .in_set(ParallelWorkSet::Render),
        );

        // Ветка PostSwapBranch
        app.add_systems(
            Update,
            post_swap_copy_system.in_set(PostSwapBranchSet::PostSwapCopy),
        );
        app.add_systems(
            Update,
            apply_cell_events_system.in_set(ComputeBlockSet::Aggregate),
        );

        // RequestSwap
        app.add_systems(
            Update,
            request_swap_system.in_set(GameFlowSet::RequestSwap),
        );

        // GCPhase (параллельно)
        app.add_systems(
            Update,
            chunk_garbage_collector_system.in_set(GCPhaseSet::GarbageCollect),
        );
        app.add_systems(
            Update,
            shadow_gc_system.in_set(GCPhaseSet::ShadowGarbageCollect),
        );
        app.add_systems(
            Update,
            shadow_copy_system.in_set(GameFlowSet::ShadowCopy),
        );

        app.add_systems(Update, debug_report_system);
    }
}