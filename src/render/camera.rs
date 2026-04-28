use crate::ecs::entity::Entity;

/// Perspective projection parameters for a camera entity.
///
/// Attach this component alongside a `Transform` to make an entity
/// act as a camera. The render system reads both components off the
/// active camera entity each frame to build the VP matrix.
pub struct Camera {
    /// Vertical field of view in radians.
    pub fov_y: f32,

    /// Distance to the near clip plane.
    pub near: f32,

    /// Distance to the far clip plane.
    pub far: f32,

    /// Viewport width divided by height.
    ///
    /// Updated by the render system each frame to match the window size.
    pub aspect: f32,
}

impl Camera {
    /// Creates a camera with the given projection parameters.
    pub fn new(fov_y: f32, near: f32, far: f32, aspect: f32) -> Self {
        Self {
            fov_y,
            near,
            far,
            aspect,
        }
    }

    /// Creates a camera with a 60 degree vertical FOV and a 0.1/1000.0 clip range.
    ///
    /// Aspect ratio is set to 1.0 and should be updated before the first frame.
    pub fn default_perspective() -> Self {
        Self {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.1,
            far: 1000.0,
            aspect: 1.0,
        }
    }

    /// Converts this camera's projection parameters into a column-major 4x4
    /// perspective projection matrix.
    ///
    /// Uses the wgpu/Vulkan depth convention where NDC depth runs from 0 to 1.
    pub fn to_projection_matrix(&self) -> [[f32; 4]; 4] {
        let f = 1.0 / (self.fov_y / 2.0).tan();

        let a = f / self.aspect;
        let b = self.far / (self.near - self.far);
        let c = (self.near * self.far) / (self.near - self.far);

        [
            [a, 0.0, 0.0, 0.0],
            [0.0, f, 0.0, 0.0],
            [0.0, 0.0, b, -1.0],
            [0.0, 0.0, c, 0.0],
        ]
    }
}

/// Identifies the currently active camera entity.
///
/// Insert this as a resource into the world. The render system reads
/// the `Camera` and `Transform` components off this entity each frame
/// to build the VP matrix.
///
/// Switching cameras is a single resource update.
pub struct ActiveCamera {
    /// The entity acting as the active camera.
    pub entity: Entity,
}

impl ActiveCamera {
    /// Creates an active camera resource pointing at `entity`.
    pub fn new(entity: Entity) -> Self {
        Self { entity }
    }
}
