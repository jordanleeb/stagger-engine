/// Position, rotation, and scale of an entity in world space.
/// 
/// Passed to the vertex shader as a model matrix each frame.
/// Both the render system and the physics system read this component.
pub struct Transform {
    /// Position in world space.
    pub position: [f32; 3],

    /// Rotation as a Euler angle in radians, applied in XYZ order.
    pub rotation: [f32; 3],

    /// Scale along each axis. [1.0, 1.0, 1.0] is no scaling.
    pub scale: [f32; 3],
}

impl Transform {
    /// Creates a transform with no translation, rotation, or scaling.
    pub fn identity() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }

    /// Converts this transform into a column-major 4x4 model matrix.
    /// 
    /// The matrix is built by combining scale, then rotation (XYZ),
    /// then translation. This is the standard order for model matrices.
    /// 
    /// The result is passed directly to the GPU as a uniform.
    pub fn to_model_matrix(&self) -> [[f32; 4]; 4] {
        let (sx, cx) = (self.rotation[0].sin(), self.rotation[0].cos());
        let (sy, cy) = (self.rotation[1].sin(), self.rotation[1].cos());
        let (sz, cz) = (self.rotation[2].sin(), self.rotation[2].cos());

        let scale_x = self.scale[0];
        let scale_y = self.scale[1];
        let scale_z = self.scale[2];

        // Combined rotation matrix (XYZ Euler order).
        // Each column is scaled by the corresponding axis scale.
        [
            [
                cy * cz * scale_x,
                (sx * sy * cz + cx * sz) * scale_x,
                (-cx * sy * cz + sx * sz) * scale_x,
                0.0,
            ],
            [
                -cy * sz * scale_y,
                (-sx * sy * sz + cx * cz) * scale_y,
                (cx * sy * sz + sx * cz) * scale_y,
                0.0,
            ],
            [
                sy * scale_z,
                -sx * cy * scale_z,
                cx * cy * scale_z,
                0.0,
            ],
            [
                self.position[0],
                self.position[1],
                self.position[2],
                1.0,
            ],
        ]
    }
}