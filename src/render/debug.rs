/// A marker drawn at the end of a line.
pub enum EndMarker {
    /// An arrow cone pointing in the line direction.
    Cone { length: f32, radius: f32 },

    /// A box centered at the line endpoint.
    Box { half_extents: [f32; 3] },

    /// A sphere centered at the line endpoint.
    Sphere { radius: f32 },
}

/// A single debug draw request accumulated during the update phase.
///
/// Pushed into `DebugDraw` by any system that needs to visualize
/// physics or gameplay state. The debug render system drains the
/// list at the end of each frame and tesselates each task into
/// GPU geometry.
pub enum DebugTask {
    /// A line segment between two points with an optional end marker.
    Line {
        start: [f32; 3],
        end: [f32; 3],
        color: [f32; 3],
        end_marker: Option<EndMarker>,
    },

    /// A solid transparent oriented box.
    Box {
        center: [f32; 3],
        half_extents: [f32; 3],
        rotation: [f32; 3],
        color: [f32; 4],
    },

    /// A solid transparent sphere.
    Sphere {
        center: [f32; 3],
        radius: f32,
        color: [f32; 4],
    },

    /// A solid transparent capsule defined by two endpoint centers and a radius.
    Capsule {
        start: [f32; 3],
        end: [f32; 3],
        radius: f32,
        color: [f32; 4],
    },

    /// A contact point visualized as a 3D cross with an arrowed normal line.
    Contact {
        position: [f32; 3],
        normal: [f32; 3],
        color: [f32; 3],
    },

    /// A raycast visualized as a line from origin to endpoint.
    ///
    /// If `hit` is `true`, a marker is drawn at the endpoint.
    Raycast {
        origin: [f32; 3],
        end: [f32; 3],
        color: [f32; 3],
        hit: bool,
    },
}

/// Accumulates debug draw requests from systems each frame.
///
/// Insert this as a resource into the world. Any system can borrow
/// it mutably and push tasks during the update phase. The debug render
/// system drains the list at the end of the frame and clears it for
/// the next tick.
pub struct DebugDraw {
    /// The accumulated draw tasks for this frame.
    tasks: Vec<DebugTask>,
}

impl DebugDraw {
    /// Creates an empty debug draw buffer.
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    /// Returns the accumulated tasks for this frame.
    ///
    /// Called by the debug render system to consume the frame's requests.
    pub fn tasks(&self) -> &[DebugTask] {
        &self.tasks
    }

    /// Clears all accumulated tasks.
    ///
    /// Called by the debug render system after each frame.
    pub fn clear(&mut self) {
        self.tasks.clear();
    }

    /// Draws a line between two points with an optional end marker.
    pub fn draw_line(
        &mut self,
        start: [f32; 3],
        end: [f32; 3],
        color: [f32; 3],
        end_marker: Option<EndMarker>,
    ) {
        self.tasks.push(DebugTask::Line {
            start,
            end,
            color,
            end_marker,
        });
    }

    /// Draws a solid transparent oriented box.
    ///
    /// `rotation` is a set of Euler angles in radians applied in XYZ order,
    /// matching the convention used by `Transform`.
    pub fn draw_box(
        &mut self,
        center: [f32; 3],
        half_extents: [f32; 3],
        rotation: [f32; 3],
        color: [f32; 4],
    ) {
        self.tasks.push(DebugTask::Box {
            center,
            half_extents,
            rotation,
            color,
        });
    }

    /// Draws a solid transparent sphere.
    pub fn draw_sphere(&mut self, center: [f32; 3], radius: f32, color: [f32; 4]) {
        self.tasks.push(DebugTask::Sphere {
            center,
            radius,
            color,
        });
    }

    /// Draws a solid transparent capsule between two endpoint centers.
    pub fn draw_capsule(&mut self, start: [f32; 3], end: [f32; 3], radius: f32, color: [f32; 4]) {
        self.tasks.push(DebugTask::Capsule {
            start,
            end,
            radius,
            color,
        });
    }

    /// Draws a contact poit as a 3D cross with an arrowed normal line.
    pub fn draw_contact(&mut self, position: [f32; 3], normal: [f32; 3], color: [f32; 3]) {
        self.tasks.push(DebugTask::Contact {
            position,
            normal,
            color,
        });
    }

    /// Draws a raycast as a line from origin to endpoint.
    ///
    /// If `hit` is true, a marker is drawn at the endpoint.
    pub fn draw_raycast(&mut self, origin: [f32; 3], end: [f32; 3], color: [f32; 3], hit: bool) {
        self.tasks.push(DebugTask::Raycast {
            origin,
            end,
            color,
            hit,
        });
    }

    /// Takes ownership of all accumulated tasks, leaving the buffer empty.
    ///
    /// Called by the debug render system to consume the frame's requests.
    /// Uses `std::mem::take` so no separate clear call is needed.
    pub fn take(&mut self) -> Vec<DebugTask> {
        std::mem::take(&mut self.tasks)
    }
}
