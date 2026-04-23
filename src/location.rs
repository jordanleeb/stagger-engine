use crate::archetype::ArchetypeId;

/// Identifies where an entity is stored in the ECS.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct EntityLocation {
    /// The archetype containing the entity.
    archetype: ArchetypeId,

    /// The row of the entity within that archetype.
    row: usize,
}

impl EntityLocation {
    /// Creates a new entity location.
    pub fn new(archetype: ArchetypeId, row: usize) -> Self {
        Self { archetype, row }
    }

    /// Returns the archetype containing the entity.
    pub fn archetype(&self) -> ArchetypeId {
        self.archetype
    }

    /// Returns the row of the entity within that archetype.
    pub fn row(&self) -> usize {
        self.row
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_stores_archetype_and_row() {
        let location = EntityLocation::new(3, 7);

        assert_eq!(location.archetype(), 3);
        assert_eq!(location.row(), 7);
    }
}
