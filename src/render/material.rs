/// Surface color for a renderable entity.
///
/// Attach this component alongside a `Mesh` and `Transform` to make
/// an entity renderable. The base color is multiplied against the
/// per-vertex color in the shader.
pub struct Material {
    /// RGB base color. Each channel is in the range 0.0 to 1.0.
    pub color: [f32; 3],
}

impl Material {
    /// Creates a material with the given base color.
    pub fn new(color: [f32; 3]) -> Self {
        Self { color }
    }

    /// Creates a white material that leaves per-vertex color unchanged.
    pub fn white() -> Self {
        Self {
            color: [1.0, 1.0, 1.0],
        }
    }
}
