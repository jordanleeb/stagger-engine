# Stagger Engine Plan

## Phase 1
- [x] Entity allocator
- [x] Component registry
- [x] Basic world structure

## Phase 2
- [x] Archetype signatures
- [x] Archetype entity storage
- [x] Component storage
- [x] Entity location tracking
- [x] World archetype management
- [x] Entity movement between archetypes
- [x] Component column integration
- [x] Add/remove component API
- [x] O(1) HashMap archetype index
- [x] Archetype edge graph for O(1) structural transitions
- [x] Zero-sized marker component support
- [x] QueryFilter and archetype matching
- [x] Query, QueryIter, and RowRef with lifetime-safe world borrows
- [x] QueryBuilder fluent API
- [ ] Systems

## Phase 3
- [ ] Physics engine
- [ ] Ragdoll prototype
- [ ] Balance controller

## Phase 4
- [ ] ECS-integrated rendering architecture
- [ ] Window and event loop
- [ ] Renderer backend
- [ ] Transform-driven rendering
- [ ] Camera component and render system
- [ ] Debug rendering
- [ ] Mesh and material components