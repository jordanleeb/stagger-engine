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

    /// Converts this transform into a column-major 4x4 view matrix.
    /// 
    /// The view matrix is the inverse of the model matrix. For a transform
    /// built from rotation and translation with unit scale, the inverse
    /// has a closed form: the upper-left 3x3 is the transpose of the
    /// rotation matrix, and the translation column is -R^T * position.
    /// 
    /// The assumes scale is [1, 1, 1]. Non-unit sclae breaks the
    /// orthogonality of the rotation matrix and invalidates this formula.
    pub fn to_view_matrix(&self) -> [[f32; 4]; 4] {
        let (sx, cx) = (self.rotation[0].sin(), self.rotation[0].cos());
        let (sy, cy) = (self.rotation[1].sin(), self.rotation[1].cos());
        let (sz, cz) = (self.rotation[2].sin(), self.rotation[2].cos());

        // The camera's basis vectors in world space.
        // These are the columns of the rotation matrix, which become
        // the rows of R^T when we transpose.
        let right   = [cy * cz, sx * sy * cz + cx * sz, -cx * sy * cz + sx * sz];
        let up      = [-cy * sz, -sx * sy * sz + cx * cz, cx * sy * sz + sx * cz];
        let forward = [sy, -sx * cy, cx * cy];

        let p = self.position;

        // Translation entries are -dot(basis, position) for each axis.
        let tx = -(right[0] * p[0] + right[1] * p[1] + right[2] * p[2]);
        let ty = -(up[0] * p[0] + up[1] * p[1] + up[2] * p[2]);
        let tz = -(forward[0] * p[0] + forward[1] * p[1] + forward[2] * p[2]);

        // R^T in the upper-left 3x3, -R^T*p in the last column.
        // Each inner array is a column.
        [
            [right[0],   up[0],   forward[0], 0.0],
            [right[1],   up[1],   forward[1], 0.0],
            [right[2],   up[2],   forward[2], 0.0],
            [tx,         ty,      tz,         1.0],
        ]
    }
}