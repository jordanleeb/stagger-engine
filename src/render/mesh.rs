use std::collections::HashMap;
use wgpu::util::DeviceExt;

/// Unique identifier for an uploaded mesh.
pub type MeshId = u32;

/// One vertex in a mesh.
///
/// The layout of this struct must match the vertex buffer layout
/// passed to the pipeline and the @location attributes in the shader.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// Position in object space.
    pub position: [f32; 3],

    /// RGB color for this vertex.
    pub color: [f32; 3],
}

impl Vertex {
    /// Returns the wgpu vertex buffer layout for this type.
    ///
    /// This tells the pipeline how to interpret the raw bytes in the
    /// vertex buffer: where each attribute starts and what type it is.
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    shader_location: 0,
                    offset: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    shader_location: 1,
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

/// An uploaded mesh living entirely on the GPU.
///
/// Created by `MeshStore::upload`. The vertex data is no longer
/// accessible from the CPU after upload.
pub struct GpuMesh {
    /// The GPU buffer holding the vertex data.
    buffer: wgpu::Buffer,

    /// The number of vertices in the buffer.
    vertex_count: u32,
}

impl GpuMesh {
    /// Returns the GPU vertex buffer.
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// Returns the number of vertices in the buffer.
    pub fn vertex_count(&self) -> u32 {
        self.vertex_count
    }
}

/// Owns all uploaded GPU meshes and assigns them unique IDs.
///
/// Insert this as a resource into the world. The render system looks
/// up `GpuMesh` values by the `MeshId` stored in each entity's `Mesh`
/// component.
pub struct MeshStore {
    /// All uploaded meshes keyed by their assigned ID.
    meshes: HashMap<MeshId, GpuMesh>,

    /// The next unused mesh ID.
    next_id: MeshId,
}

impl MeshStore {
    /// Creates an empty mesh store with no uploaded meshes.
    pub fn new() -> Self {
        Self {
            meshes: HashMap::new(),
            next_id: 0,
        }
    }

    /// Uploads vertex data to the GPU and returns the assigned `MeshId`.
    ///
    /// The CPU-side slice is not retained after this call. The data
    /// lives only in the returned GPU buffer from this point on.
    pub fn upload(&mut self, device: &wgpu::Device, vertices: &[Vertex]) -> MeshId {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertex buffer"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let id = self.next_id;
        self.next_id += 1;

        self.meshes.insert(id, GpuMesh {
            buffer,
            vertex_count: vertices.len() as u32,
        });

        id
    }

    /// Returns the uploaded mesh for a given ID.
    ///
    /// Returns `None` if the ID has not been assigned.
    pub fn get(&self, id: MeshId) -> Option<&GpuMesh> {
        self.meshes.get(&id)
    }
}

/// Identifies which uploaded mesh an entity uses for rendering.
///
/// Attach this component alongside a `Transform` and `Material` to
/// make an entity renderable. The render system looks up the
/// corresponding `GpuMesh` in the `MeshStore` resource each frame.
pub struct Mesh {
    /// The ID of the uploaded mesh to render.
    pub id: MeshId,
}

impl Mesh {
    /// Creates a mesh component referencing the given uploaded mesh.
    pub fn new(id: MeshId) -> Self {
        Self { id }
    }
}