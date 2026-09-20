//! Контроллер изометрической камеры.

use bevy::prelude::*;

const PITCH_SPEED: f32 = 45.0;
const MOVE_SPEED: f32 = 16.0;
const ZOOM_LEVELS: [f32; 3] = [1.0, 2.0, 4.0];
const YAW_STEPS: [f32; 4] = [
    0.0,
    std::f32::consts::FRAC_PI_2,
    std::f32::consts::PI,
    3.0 * std::f32::consts::FRAC_PI_2,
];

#[derive(Resource)]
pub struct CameraController {
    pub pitch: f32,
    pub yaw_index: usize,
    pub zoom_index: usize,
    pub anchor_x: f64,
    pub anchor_y: f64,
    pub anchor_z: f64,
    pub view_half_width: f32,
    pub view_half_height: f32,
    pub camera_height: f32,
}

impl CameraController {
    pub fn new() -> Self {
        let initial_zoom = ZOOM_LEVELS[0];
        Self {
            pitch: 45.0_f32.to_radians(),
            yaw_index: 0,
            zoom_index: 0,
            anchor_x: 0.0,
            anchor_y: 64.0,
            anchor_z: 0.0,
            view_half_width: 16.0 / initial_zoom,
            view_half_height: 16.0 / initial_zoom,
            camera_height: 128.0,
        }
    }

    #[inline]
    pub fn yaw(&self) -> f32 {
        YAW_STEPS[self.yaw_index]
    }

    #[inline]
    pub fn zoom(&self) -> f32 {
        ZOOM_LEVELS[self.zoom_index]
    }

    pub fn zoom_in(&mut self) {
        if self.zoom_index < ZOOM_LEVELS.len() - 1 {
            self.zoom_index += 1;
            self.update_view_size();
        }
    }

    pub fn zoom_out(&mut self) {
        if self.zoom_index > 0 {
            self.zoom_index -= 1;
            self.update_view_size();
        }
    }

    pub fn rotate_left(&mut self) {
        self.yaw_index = (self.yaw_index + 3) % 4;
    }

    pub fn rotate_right(&mut self) {
        self.yaw_index = (self.yaw_index + 1) % 4;
    }

    fn update_view_size(&mut self) {
        let zoom = self.zoom();
        self.view_half_width = 16.0 / zoom;
        self.view_half_height = 16.0 / zoom;
    }

    #[inline]
    pub fn look_direction(&self) -> Vec3 {
        let cos_pitch = self.pitch.cos();
        let sin_pitch = self.pitch.sin();
        let cos_yaw = self.yaw().cos();
        let sin_yaw = self.yaw().sin();

        Vec3::new(
            -cos_pitch * sin_yaw,
            -sin_pitch,
            -cos_pitch * cos_yaw,
        )
        .normalize()
    }

    pub fn camera_position(&self) -> Vec3 {
        let look_dir = self.look_direction();
        let distance = self.camera_height / self.pitch.sin().max(0.001);

        Vec3::new(
            self.anchor_x as f32 - look_dir.x * distance,
            self.anchor_y as f32 - look_dir.y * distance,
            self.anchor_z as f32 - look_dir.z * distance,
        )
    }

    #[inline]
    pub fn anchor_position(&self) -> Vec3 {
        Vec3::new(
            self.anchor_x as f32,
            self.anchor_y as f32,
            self.anchor_z as f32,
        )
    }
}

impl Default for CameraController {
    fn default() -> Self {
        Self::new()
    }
}

pub fn camera_input_system(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut controller: ResMut<CameraController>,
) {
    let dt = time.delta_secs();

    if keys.pressed(KeyCode::KeyX) {
        controller.pitch += PITCH_SPEED.to_radians() * dt;
        controller.pitch = controller.pitch.min(90.0_f32.to_radians());
    }
    if keys.pressed(KeyCode::KeyZ) {
        controller.pitch -= PITCH_SPEED.to_radians() * dt;
        controller.pitch = controller.pitch.max(10.0_f32.to_radians());
    }

    if keys.just_pressed(KeyCode::KeyQ) {
        controller.rotate_left();
    }
    if keys.just_pressed(KeyCode::KeyE) {
        controller.rotate_right();
    }

    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        controller.zoom_in();
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        controller.zoom_out();
    }

    let mut move_x = 0.0f64;
    let mut move_z = 0.0f64;

    if keys.pressed(KeyCode::KeyW) { move_z -= 1.0; }
    if keys.pressed(KeyCode::KeyS) { move_z += 1.0; }
    if keys.pressed(KeyCode::KeyA) { move_x -= 1.0; }
    if keys.pressed(KeyCode::KeyD) { move_x += 1.0; }

    if move_x != 0.0 || move_z != 0.0 {
        let len = (move_x * move_x + move_z * move_z).sqrt();
        move_x /= len;
        move_z /= len;

        let yaw = controller.yaw();
        let cos_yaw = yaw.cos() as f64;
        let sin_yaw = yaw.sin() as f64;

        // ИСПРАВЛЕНИЕ: формула вращения по часовой стрелке
        // (соответствует направлению взгляда в look_direction)
        let rotated_x = move_x * cos_yaw + move_z * sin_yaw;
        let rotated_z = -move_x * sin_yaw + move_z * cos_yaw;

        let speed = MOVE_SPEED as f64 * dt as f64;
        controller.anchor_x += rotated_x * speed;
        controller.anchor_z += rotated_z * speed;
    }
}

pub fn camera_transform_system(
    controller: Res<CameraController>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
) {
    for mut transform in camera_query.iter_mut() {
        let camera_pos = controller.camera_position();
        let anchor_pos = controller.anchor_position();

        *transform = Transform::from_translation(camera_pos)
            .looking_at(anchor_pos, Vec3::Y);
    }
}